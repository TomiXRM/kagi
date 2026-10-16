//! Commit Inspector panel — W2-INSPECTOR / ADR-0015 / W7-INSPECTOR2
//!
//! W7-INSPECTOR2 layout (top → bottom):
//!   1. Title          (commit summary, wraps to 2 lines max + truncate)
//!   2. Meta row       (avatar · author name · committed date · short-hash chip)
//!   3. Actions row    (Create branch · Cherry-pick — compact; Copy SHA = hash chip click)
//!   4. Message box    (independent vertical scroll, resizable)
//!      ── InspectorSplit divider (drag to change message:files ratio) ──
//!   5. Counts row     (N modified · N added · N deleted · N renamed)
//!   6. Changed files  (tree or flat path list, Path⇄Tree toggle, scroll)
//!
//! The message box and files box default to a 1:1 height ratio
//! (`KagiApp.inspector_split = 0.5`); the divider clamps it to 0.2..=0.8.
//! The hash chip shows the full SHA in a tooltip and copies it on click
//! (replacing the old always-on Metadata column of Parents / full SHA).

use gpui::{
    canvas, div, prelude::*, px, relative, rgb, App, Bounds, Context, IntoElement, MouseButton,
    Pixels, SharedString, Window,
};

use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::Sizable as _;

use kagi_git::{is_url, sanitize_trailer_value, CommitId};
use kagi_ui_core::tooltip::HoverTooltip as _;

// Badge sort order is shared with the graph column — one definition only
// (a second copy silently drifts and the two views disagree).
use super::badges::badge_priority;
// issue #414: PR title/url in the inspector tooltip are gh-sourced — sanitize.
use super::render_helpers::safe_text;

use super::{
    avatar::{avatar_color, avatar_initial},
    commit_list::{BadgeKind, RefBadge},
    context_menu::CommitAction,
    detail_panel::CommitDetail,
    inspector_model::InspectorModel,
    DividerDrag, DividerGhost, DividerKind, KagiApp,
};

// W9-THEME: all colours come from `theme()` (see theme.rs). No local palette.
use super::i18n::{self, Msg};
use super::theme::{self, theme};

const MAX_BADGE_CHARS: usize = 20;

// W7-INSPECTOR2: message/files split clamp bounds (mirrors mod.rs; the drag
// handler in mod.rs is the source of truth — these guard the flex_basis only).
const INSPECTOR_SPLIT_MIN: f32 = 0.2;
const INSPECTOR_SPLIT_MAX: f32 = 0.8;
pub(super) const INSPECTOR_SPLIT_DIVIDER_H: f32 = 4.0;

// ─────────────────────────────────────────────────────────────────────────────
// Public entry-point
// ─────────────────────────────────────────────────────────────────────────────

/// Render the Commit Inspector right panel.
///
/// Section order (W7-INSPECTOR2):
///   Title → Meta row → Actions → Message box │ divider │ Counts → Changed Files
///
/// `model` — the synced derived model (issue #512); this function only reads
/// it. `tree_view` — when `true` render the tree; when `false` render flat
/// paths. `inspector_split` — message:files height ratio (0.5 = 1:1).
#[allow(clippy::too_many_arguments)]
pub(super) fn render_inspector(
    detail: Option<(CommitDetail, CommitId)>,
    badges: Vec<RefBadge>,
    // GitHub Phase 1: open PRs whose head branch tip is this commit.
    prs_here: Vec<kagi_domain::github::PullRequest>,
    model: &InspectorModel,
    // issue #348: whether the "Generated (N)" section is expanded. Default
    // `false` (folding on).
    generated_expanded: bool,
    // Compare mode (ADR-0026): the banner title; `None` for a plain commit.
    compare_title: Option<SharedString>,
    active_file: Option<usize>,
    window_active: bool,
    tree_view: bool,
    inspector_split: f32,
    // Measured (top, bottom) of the message+files region, written at paint
    // time and read by the InspectorSplit drag handler in mod.rs.
    split_geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    panel_width: f32,
    compact: bool,
    // W11-AVATAR: resolved GitHub avatar images keyed by author email.
    avatar_images: &std::collections::HashMap<String, std::sync::Arc<gpui::Image>>,
    fallback: Option<&gpui::FocusHandle>,
    message_handle: &gpui::ScrollHandle,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let files = model.files();

    // Share one session-owned roving selector between wide and compact Inspector.
    // Activation explicitly sets the mode: reactivating a selected tab is inert.
    let weak = cx.entity().downgrade();
    let tabs = super::keyboard_nav::TabList::new(
        &model.view_focus,
        super::keyboard_nav::slot_keys(2),
        vec![0, 1],
        Some(usize::from(tree_view)),
        super::keyboard_nav::Activation::Automatic,
        fallback.cloned(),
        move |slot, _, _, cx| {
            let _ = weak.update(cx, |app, cx| {
                app.inspector_tree_view = slot == 1;
                cx.notify();
            });
        },
        cx,
    );
    let cell = |slot, id: &'static str, label: &'static str| {
        let selected = tree_view == (slot == 1);
        super::e2e::commit_panel_labels::focus(id, tabs.focus_handle(slot));
        tabs.cell(
            slot,
            label,
            div()
                .id(id)
                .m(px(-super::keyboard_nav::RING))
                .relative()
                .role(super::e2e::commit_panel_labels::role(id, gpui::Role::Tab))
                .aria_label(super::e2e::commit_panel_labels::name(id, label))
                .aria_selected(super::e2e::commit_panel_labels::selected(id, selected))
                .child(super::e2e::measure_inside(id))
                .child(
                    div()
                        .px_2()
                        .py_px()
                        .rounded_sm()
                        .bg(if selected {
                            theme().segment_selected_bg()
                        } else {
                            rgb(theme().surface).into()
                        })
                        .text_xs()
                        .text_color(rgb(if selected {
                            theme().text_main
                        } else {
                            theme().text_sub
                        }))
                        .child(SharedString::from(label)),
                ),
        )
    };
    let toggle_row = tabs
        .list(
            div()
                .id("files-toggle-row")
                .flex()
                .flex_row()
                .items_center()
                .gap_1(),
        )
        .role(super::e2e::commit_panel_labels::role(
            "files-toggle-row",
            gpui::Role::TabList,
        ))
        .aria_label(super::e2e::commit_panel_labels::name(
            "files-toggle-row",
            Msg::A11yInspectorFiles.t(),
        ))
        .child(cell(0, "toggle-path", "Path"))
        .child(cell(1, "toggle-tree", "Tree"));

    // ── Counts row (ChangeKind tally; 0-count kinds omitted) ──────────────
    let counts_row = files.map(|files| {
        let c = files.counts;
        let mut parts: Vec<String> = Vec::new();
        for (n, kind) in [
            (c.modified, "modified"),
            (c.added, "added"),
            (c.deleted, "deleted"),
            (c.renamed, "renamed"),
            (c.typechange, "type-change"),
        ] {
            if n > 0 {
                parts.push(format!("{n} {kind}"));
            }
        }
        let text = if parts.is_empty() {
            SharedString::from(Msg::NoFileChanges.t())
        } else {
            SharedString::from(parts.join("  \u{00B7}  "))
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .mb_1()
            .text_xs()
            .text_color(rgb(theme().text_sub))
            .truncate()
            .child(text)
    });

    let compare_banner = compare_title.map(|title| {
        let close_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
            this.close_compare_view();
            cx.notify();
        });
        div()
            .id("compare-banner")
            .flex()
            .when(cfg!(feature = "gui-e2e"), |banner| {
                banner.relative().child(super::e2e::measure_inside(format!(
                    "compare-banner-{}",
                    title.as_ref()
                )))
            })
            .flex_row()
            .items_center()
            .justify_between()
            .gap_2()
            .mb_1()
            .px_2()
            .py_px()
            .rounded_sm()
            .bg(rgb(theme().surface))
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_sm()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(format!("Comparing: {}", title.as_ref()))),
            )
            .child(
                Button::new("compare-close")
                    .label("×")
                    .ghost()
                    .small()
                    .on_click(close_click),
            )
    });

    // ── Files header: Path⇄Tree toggle (compare banner above when comparing) ─
    let files_header = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_end()
        .mb_1()
        .child(toggle_row);

    let files_list = model.keyboard.render(
        model.files_snapshot(),
        tree_view,
        generated_expanded,
        active_file,
        window_active,
        fallback,
        window,
        cx,
    );

    // ── Bottom box: compare banner / counts / toggle / files list ─────────
    let files_box = div()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .flex_basis(relative(
            (1.0 - inspector_split).clamp(INSPECTOR_SPLIT_MIN, INSPECTOR_SPLIT_MAX),
        ))
        .flex_shrink(1.)
        .px_3()
        .when(cfg!(feature = "gui-e2e"), |frame| {
            frame
                .relative()
                .child(super::e2e::measure_inside("inspector-files-frame"))
        })
        .children(compare_banner)
        .children(counts_row)
        .child(files_header)
        .child(files_list);

    // Compare files are meaningful without a selected/loaded Graph commit.
    let panel = div()
        // The persisted width is unscaled; render it at the active zoom.
        .w(theme::scaled_px(panel_width))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().panel))
        .when(compact, |panel| {
            panel
                .w_full()
                .flex_1()
                .min_h(px(0.))
                .min_w(px(0.))
                .flex_row()
        });
    let Some((d, at)) = detail else {
        return panel.child(files_box.flex_1().flex_basis(relative(1.)).pt_1());
    };

    // ── Short SHA (first 8 hex chars) ────────────────────────────────────
    let short_sha: SharedString =
        SharedString::from(d.full_sha.chars().take(8).collect::<String>());

    // ── Copy SHA handler (full raw SHA — no ZWSP) ─────────────────────────
    let copy_target1 = at.clone();
    let copy_sha_click1 = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.dispatch_commit_action(CommitAction::CopySha, copy_target1.clone(), _window, cx);
    });

    // ── Author name + email (parsed from `author_line`) ───────────────────
    // `author_line` format: "name  <email>  YYYY-MM-DD HH:MM" (detail_panel).
    // We only need the display name (meta row) and email (avatar colour).
    let (author_name, author_email) = parse_author(d.author_line.as_ref());

    // ── Message (single wrapped text element) ─────────────────────────────
    // One text run, not per-line divs: gpui's text layout handles '\n' and
    // soft-wrapping itself. Rendered as escaped HTML through a selectable
    // `TextView`, not as a plain text run: GPUI has no text selection for a
    // plain run, and `TextView` — the one element that does — parses only
    // Markdown or HTML. The reflow + escape is derived per commit in the model.
    let message_html = model.message_html();

    // ── "Create branch here" button ──────────────────────────────────────
    let at_for_create = at.clone();
    let at_for_cherry = at.clone();
    let create_branch_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.dispatch_commit_action(
            CommitAction::CreateBranchHere,
            at_for_create.clone(),
            _window,
            cx,
        );
        cx.notify();
    });
    let create_branch_button = action_button(
        "create-branch-btn",
        "+ Branch here",
        theme().ui_accent(),
        create_branch_click,
    );

    // ── "Cherry-pick onto HEAD" button (T016) ────────────────────────────
    let cherry_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.dispatch_commit_action(CommitAction::CherryPick, at_for_cherry.clone(), _window, cx);
        cx.notify();
    });
    let cherry_pick_button = action_button(
        "cherry-pick-btn",
        "\u{1f352} Cherry-pick",
        theme::theme().accent, // accent (cherry-pick)
        cherry_click,
    );

    // ── Title (commit summary, up to 2 wrapped lines + truncate) ──────────
    let title_text: SharedString = SharedString::from(
        d.full_message
            .as_ref()
            .lines()
            .next()
            .unwrap_or("")
            .to_string(),
    );
    let title_el = div()
        .text_color(rgb(theme().text_main))
        .font_weight(gpui::FontWeight::MEDIUM)
        .mb_1()
        // Wrapped, inside a FIXED 2-line box (not line_clamp(2), and no
        // content-sized height): wrapped-text height measurement lags a frame
        // on zed-main gpui, so any content-sized wrapping element in the
        // header makes everything below it flap one line per frame while the
        // pane is resized. A constant-height box keeps the wrap purely visual;
        // >2-line subjects clip (full subject is the message body's 1st line).
        .whitespace_normal()
        .line_height(gpui::rems(1.3))
        .h(gpui::rems(2.6))
        .overflow_hidden()
        .child(title_text);

    // ── Meta row: avatar · author name · committed date · short-hash chip ──
    // W11-AVATAR: show the resolved GitHub avatar image when available; else
    // the initial-on-colour circle (T020 fallback).
    let avatar_hsla = avatar_color(&author_email);
    let initial = SharedString::from(avatar_initial(&author_name));
    let avatar_el = {
        let circle = div()
            .w(theme::scaled_px(18.))
            .h(theme::scaled_px(18.))
            .flex_shrink_0()
            .rounded_full()
            .overflow_hidden();
        match avatar_images.get(&author_email).cloned() {
            Some(image) => circle.child(
                gpui::img(gpui::ImageSource::Image(image))
                    .size_full()
                    .rounded_full(),
            ),
            None => circle
                .flex()
                .items_center()
                .justify_center()
                .bg(avatar_hsla)
                .text_xs()
                .text_color(rgb(theme().bg_base))
                .child(initial),
        }
    };

    let author_display = SharedString::from(author_name.clone());
    let author_identity = super::inspector_metadata::identity(&author_name, &author_email);
    let hash_name = SharedString::from(i18n::inspector_copy_hash(&short_sha));
    #[cfg(feature = "gui-e2e")]
    super::e2e::inspector_metadata::record(super::e2e::inspector_metadata::Metadata {
        author: author_display.clone(),
        identity: Some(author_identity.clone()),
        tooltip: None,
        hash_role: None,
        hash_name: None,
    });

    // Short-hash chip: tooltip = full SHA (+ committer when it differs from the
    // author), click = Copy SHA (dispatch).  This replaces the old always-on
    // Parents / full-SHA / Committer metadata column.
    let tooltip_text: SharedString = match &d.committer_line {
        Some(c) => SharedString::from(format!(
            "{}\nCommitter: {}",
            d.full_sha.as_ref(),
            c.as_ref()
        )),
        None => d.full_sha.clone(),
    };
    let hash_chip = Button::new("inspector-hash-chip")
        .ghost()
        .xsmall()
        .compact()
        .h(theme::scaled_px(18.))
        .px_1()
        .font_family(super::MONO_FONT)
        .text_color(rgb(theme().text_sub))
        .on_click(copy_sha_click1)
        .child(
            div()
                .child(short_sha)
                .child(super::e2e::measure_inside("inspector-hash-label")),
        );
    let hash_chip =
        super::inspector_metadata::named_hash_button(hash_chip, hash_name, tooltip_text.clone())
            .hover_tooltip("inspector-hash-hover-help", tooltip_text);

    let meta_row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .mb_1()
        .child(avatar_el)
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .id("inspector-author")
                        .role(gpui::Role::Label)
                        .aria_label(author_identity.clone())
                        .flex_shrink(1.)
                        .when(compact, |author| author.flex_1())
                        .min_w(px(0.))
                        .text_sm()
                        .text_color(rgb(theme().text_main))
                        .truncate()
                        .child(author_display)
                        .child(super::e2e::measure_inside("inspector-author"))
                        .hover_tooltip_builder("inspector-author-help", move |_window, cx| {
                            #[cfg(feature = "gui-e2e")]
                            super::e2e::inspector_metadata::record_tooltip(author_identity.clone());
                            cx.new(|_| HashTooltip {
                                sha: author_identity.clone(),
                            })
                            .into()
                        }),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(d.committed_date),
                ),
        )
        .child(super::e2e::measure_control(
            "inspector-hash-chip",
            hash_chip,
        ));

    // #1307: complete co-authors/trailers are prepared once per immutable SHA.
    let coauthors = model.coauthors();
    let coauthor_rows: Vec<gpui::AnyElement> = coauthors
        .iter()
        .enumerate()
        .map(|(i, ca)| {
            let display_name = if ca.name.is_empty() {
                ca.email.clone()
            } else {
                ca.name.clone()
            };
            let name_full = SharedString::from(display_name.clone());

            // 16px avatar reusing the author avatar machinery.
            let ca_initial = SharedString::from(avatar_initial(&display_name));
            let ca_hsla = avatar_color(&ca.email);
            let ca_avatar = {
                let circle = div()
                    .w(theme::scaled_px(16.))
                    .h(theme::scaled_px(16.))
                    .flex_shrink_0()
                    .rounded_full()
                    .overflow_hidden();
                match avatar_images.get(&ca.email).cloned() {
                    Some(image) => circle.child(
                        gpui::img(gpui::ImageSource::Image(image))
                            .size_full()
                            .rounded_full(),
                    ),
                    None => circle
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(ca_hsla)
                        .text_color(rgb(theme().bg_base))
                        .child(div().text_size(px(9.)).child(ca_initial)),
                }
            };

            let identity = super::inspector_metadata::identity(&ca.name, &ca.email);

            let row = div()
                .id(("inspector-coauthor", i))
                .role(gpui::Role::Label)
                .aria_label(identity.clone())
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(ca_avatar)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .truncate()
                        .child(name_full),
                )
                .hover_tooltip_builder(("inspector-coauthor-help", i), move |_window, cx| {
                    cx.new(|_| HashTooltip {
                        sha: identity.clone(),
                    })
                    .into()
                });
            row.into_any_element()
        })
        .collect();

    // "Co-authored by" caption shown above the co-author rows (only when any).
    let coauthors_block: Option<gpui::AnyElement> = if coauthor_rows.is_empty() {
        None
    } else {
        let mut block = div().flex().flex_col().gap_px().mb_1().child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::CoAuthoredBy.t())),
        );
        for r in coauthor_rows {
            block = block.child(r);
        }
        Some(block.into_any())
    };

    // Trailer metadata uses the same complete sanitized source as Copy.
    let trailers = model.trailers();
    #[cfg(feature = "gui-e2e")]
    super::e2e::text_caps::record_message(super::e2e::text_caps::Message {
        source: d.full_message.clone(),
        html: message_html.clone(),
        coauthors: coauthors.len(),
        trailers: trailers.len(),
    });
    let trailer_rows: Vec<gpui::AnyElement> = trailers
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let key = SharedString::from(t.key.clone());
            let value_str = sanitize_trailer_value(&t.value);
            let value_el = if is_url(&t.value) {
                let url = t.value.trim().to_string();
                div()
                    .id(("inspector-trailer-link", i))
                    .cursor_pointer()
                    .text_xs()
                    .text_color(rgb(theme().color_head))
                    .truncate()
                    .child(SharedString::from(value_str))
                    .on_mouse_down(MouseButton::Left, move |_e, _window, cx| {
                        cx.open_url(&url);
                    })
                    .into_any()
            } else {
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(value_str))
                    .into_any()
            };
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .gap_2()
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(key),
                )
                .child(div().flex_1().min_w(px(0.)).child(value_el))
                .when(
                    i + 1 == trailers.len() && cfg!(feature = "gui-e2e"),
                    |row| {
                        row.relative()
                            .child(super::e2e::measure_inside("inspector-trailer-tail"))
                    },
                )
                .into_any()
        })
        .collect();
    let trailers_block: Option<gpui::AnyElement> = if trailer_rows.is_empty() {
        None
    } else {
        let mut block = div().flex().flex_col().gap_px().mb_1().child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::Trailers.t())),
        );
        for r in trailer_rows {
            block = block.child(r);
        }
        Some(block.into_any())
    };

    // ── Ref badges row ────────────────────────────────────────────────────
    let badges_row = {
        let mut row = div()
            .flex()
            .flex_row()
            .items_center()
            .flex_wrap()
            .gap_1()
            .mb_1();
        let mut by_prio = badges;
        by_prio.sort_by_key(|b| badge_priority(&b.kind));
        let mut row_ix = 0usize;
        for badge in &by_prio {
            let color = match badge.kind {
                BadgeKind::HeadBranch => theme().color_head,
                BadgeKind::Branch | BadgeKind::Worktree => theme().color_branch,
                BadgeKind::Remote => theme().color_remote,
                BadgeKind::Tag => theme().color_tag,
            };
            // Same folding + leading where-icons as the graph chips (laptop =
            // local, cloud = remote; see `badge_display`).
            let (display, wh) = super::commit_list::badge_display(badge);
            // HEAD: leading ✓ replaces the laptop icon (same as the graph chips).
            let is_head = display.ends_with(" \u{2713}");
            let display = display.trim_end_matches(" \u{2713}").to_string();
            let where_icons: Vec<&'static str> = [
                (wh.local && !is_head).then_some("icons/laptop.svg"),
                wh.remote.then_some("icons/cloud.svg"),
            ]
            .into_iter()
            .flatten()
            .collect();
            let label: SharedString = if display.chars().count() > MAX_BADGE_CHARS {
                let s: String = display.chars().take(MAX_BADGE_CHARS - 1).collect();
                SharedString::from(format!("{}\u{2026}", s))
            } else {
                SharedString::from(display)
            };
            let tip = SharedString::from(super::commit_list::badge_tooltip(badge));
            let (badge_bg, badge_border, badge_text) = super::theme::badge_style(color);
            row = row.child(
                div()
                    .id(("inspector-ref", row_ix))
                    .px_1()
                    .rounded_sm()
                    .bg(gpui::rgba(badge_bg))
                    .border_1()
                    .border_color(gpui::rgba(badge_border))
                    .text_color(rgb(badge_text))
                    .text_xs()
                    .flex_shrink_0()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .when(is_head, |c| c.child(SharedString::from("\u{2713}")))
                    .children(where_icons.into_iter().map(|path| {
                        gpui::svg()
                            .path(path)
                            .flex_shrink_0()
                            .w(theme::scaled_px(10.))
                            .h(theme::scaled_px(10.))
                            .text_color(rgb(badge_text))
                    }))
                    .child(label)
                    .hover_tooltip_builder(("inspector-ref-help", row_ix), move |w, cx| {
                        gpui_component::tooltip::Tooltip::new(tip.clone()).build(w, cx)
                    }),
            );
            row_ix += 1;
        }
        // PR chips: `#N ✓` in the CI colour, click opens the PR in the browser.
        for pr in &prs_here {
            use kagi_domain::github::CiState;
            let (glyph, color) = match pr.ci {
                CiState::Success => ("\u{2713}", theme().color_success),
                CiState::Failure => ("\u{2717}", theme().color_blocker),
                CiState::Pending => ("\u{25CF}", theme().color_warning),
                CiState::None => ("", theme().text_muted),
            };
            let (badge_bg, badge_border, badge_text) = super::theme::badge_style(color);
            let pr_click = pr.clone();
            let open = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.open_pr_in_browser(&pr_click);
                cx.notify();
            });
            let tip = safe_text(&format!("#{} {}\n{}", pr.number, pr.title, pr.url));
            row = row.child(
                div()
                    .id(("inspector-pr", pr.number as usize))
                    .px_1()
                    .rounded_sm()
                    .bg(gpui::rgba(badge_bg))
                    .border_1()
                    .border_color(gpui::rgba(badge_border))
                    .text_color(rgb(badge_text))
                    .text_xs()
                    .flex_shrink_0()
                    .cursor_pointer()
                    .hover(|s| s.opacity(0.8))
                    .on_click(open)
                    .child(SharedString::from(
                        format!("#{} {}", pr.number, glyph).trim().to_string(),
                    ))
                    .hover_tooltip_builder(
                        ("inspector-pr-help", pr.number as usize),
                        move |w, cx| {
                            gpui_component::tooltip::Tooltip::new(tip.clone()).build(w, cx)
                        },
                    ),
            );
        }
        row
    };

    // ── Compact Actions row (single row) ──────────────────────────────────
    let actions_row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .flex_wrap()
        .mb_1()
        .child(create_branch_button)
        .child(cherry_pick_button);

    // ── Message box (independent scroll, top of the split) ────────────────
    let message_inner = div()
        .w_full()
        .min_w(px(0.))
        .text_color(rgb(theme().text_main))
        .text_sm()
        .whitespace_normal()
        .child(
            // Keyed by SHA so switching commits resets the view's own state
            // instead of carrying the previous commit's selection over.
            gpui_component::text::TextView::html(
                SharedString::from(format!("inspector-message-{}", d.full_sha)),
                message_html,
            )
            .selectable(true),
        )
        // TextView supplies literal selection and inline URL styling, but the
        // pinned kit has no per-link keyboard/AX interface. Keep its explicit
        // link actions in the same independently scrolling message region.
        .children(
            model
                .message_links()
                .iter()
                .enumerate()
                .map(|(index, url)| {
                    let open_id = SharedString::from(format!("inspector-message-link-{index}"));
                    let copy_id =
                        SharedString::from(format!("inspector-message-copy-link-{index}"));
                    let open_url = url.clone();
                    let copy_url = url.clone();
                    let open_name: SharedString =
                        format!("{}: {url}", Msg::InspectorOpenLink.t()).into();
                    let copy_name: SharedString =
                        format!("{}: {url}", Msg::InspectorCopyLink.t()).into();
                    let open = Button::new(open_id.clone())
                        .link()
                        .xsmall()
                        .compact()
                        .flex_1()
                        .min_w(px(0.))
                        .h(theme::scaled_px(24.))
                        .on_click(move |_, _, cx| KagiApp::open_link_url(&open_url, cx))
                        .child(
                            div()
                                .min_w(px(0.))
                                .truncate()
                                .text_decoration_1()
                                .child(url.clone()),
                        );
                    let copy = Button::new(copy_id.clone())
                        .ghost()
                        .xsmall()
                        .compact()
                        .flex_shrink_0()
                        .h(theme::scaled_px(24.))
                        .child(Msg::InspectorCopyLink.t())
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                copy_url.to_string(),
                            ));
                        });
                    #[cfg(feature = "gui-e2e")]
                    let open = {
                        let window_id = window.window_handle().window_id();
                        let name = open_id.clone();
                        open.bounds_probe(move |bounds| {
                            super::e2e::record_control_bounds(window_id, &name, bounds)
                        })
                    };
                    #[cfg(feature = "gui-e2e")]
                    let copy = {
                        let window_id = window.window_handle().window_id();
                        let name = copy_id.clone();
                        copy.bounds_probe(move |bounds| {
                            super::e2e::record_control_bounds(window_id, &name, bounds)
                        })
                    };
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .w_full()
                        .min_w(px(0.))
                        .child(
                            super::inspector_metadata::named_button(
                                open,
                                gpui::Role::Link,
                                open_name,
                                url.clone(),
                                false,
                            )
                            .reveal_on_focus(open_id, message_handle.clone())
                            .hover_tooltip(
                                ("inspector-message-link-hover-help", index),
                                url.clone(),
                            ),
                        )
                        .child(
                            super::inspector_metadata::named_button(
                                copy,
                                gpui::Role::Button,
                                copy_name,
                                url.clone(),
                                false,
                            )
                            .reveal_on_focus(copy_id, message_handle.clone()),
                        )
                }),
        )
        .children(coauthors_block)
        .children(trailers_block);
    // The scroll lives on an inner size_full child, NOT on the flex-sized box:
    // with overflow_y_scroll directly on the flex_basis box, the wrapped-text
    // content size feeds back into the flex solve and the layout flip-flops
    // between two states on every pane-width change (blank-line jitter while
    // dragging). overflow_hidden on the outer box cuts that feedback; the
    // inner scroller gets its definite bounds from the solved box.
    // ── Fixed header region (title, meta, badges, actions) — not scrolled ──
    let header_region = div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .px_3()
        .pt_1()
        .pb_1()
        .relative()
        .child(super::e2e::measure_inside("inspector-header"))
        .child(title_el)
        .child(meta_row)
        .child(badges_row)
        .child(actions_row);
    if compact {
        // Metadata and message share one scroll viewport in the short bottom
        // projection. The desktop header remains fixed in the ordinary pane.
        let details_scroll = div()
            .id("inspector-message-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(message_handle)
            .child(header_region)
            .child(message_inner.px_3());
        let details = div()
            .flex()
            .flex_col()
            .flex_1()
            .flex_basis(px(0.))
            .min_w(px(0.))
            .min_h(px(0.))
            .h_full()
            .overflow_hidden()
            .relative()
            .child(super::e2e::measure_inside("compact-inspector-details"))
            .child(super::render_helpers::with_system_vertical_scrollbar(
                "inspector-message-scrollbar",
                message_handle,
                details_scroll,
                cx,
            ));
        let files = div()
            .flex_1()
            .flex_basis(px(0.))
            .min_w(px(0.))
            .min_h(px(0.))
            .h_full()
            .relative()
            .child(super::e2e::measure_inside("compact-inspector-files"))
            .child(
                files_box
                    .size_full()
                    .flex_basis(relative(1.))
                    .pt_1()
                    .border_l_1()
                    .border_color(rgb(theme().surface)),
            );
        return panel.child(details).child(files);
    }
    let message_scroll = div()
        .id("inspector-message-scroll")
        .size_full()
        .min_h(px(0.))
        .overflow_y_scroll()
        .track_scroll(message_handle)
        .child(message_inner)
        .when(cfg!(feature = "gui-e2e"), |scroll| {
            scroll.child(super::e2e::measure_inside("inspector-message-scroll"))
        });
    let message_box = div()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .flex_basis(relative(
            inspector_split.clamp(INSPECTOR_SPLIT_MIN, INSPECTOR_SPLIT_MAX),
        ))
        .flex_shrink(1.)
        .overflow_hidden()
        .px_3()
        .when(cfg!(feature = "gui-e2e"), |frame| {
            frame
                .relative()
                .child(super::e2e::measure_inside("inspector-message-frame"))
        })
        .child(super::render_helpers::with_system_vertical_scrollbar(
            "inspector-message-scrollbar",
            message_handle,
            message_scroll,
            cx,
        ));

    // ── InspectorSplit divider (absolute-coordinate ratio; see mod.rs) ────
    let split_divider = div()
        .id("inspector-split-divider")
        .group("inspector-split-divider")
        .h(theme::scaled_px(INSPECTOR_SPLIT_DIVIDER_H))
        .flex_shrink_0()
        .w_full()
        .bg(rgb(theme().panel))
        .flex()
        .flex_col()
        .justify_center()
        .child(
            div()
                .w_full()
                .h(px(1.))
                .flex_shrink_0()
                .bg(rgb(theme().surface))
                .group_hover("inspector-split-divider", |s| {
                    s.bg(rgb(theme().ui_accent()))
                })
                .group_drag_over::<DividerDrag>("inspector-split-divider", |s| {
                    s.bg(rgb(theme().ui_accent()))
                }),
        )
        .hover(|s| s.bg(rgb(theme().ui_accent())).cursor_row_resize())
        .drag_over::<DividerDrag>(|style, drag, _, _| {
            if drag.kind == DividerKind::InspectorSplit {
                style.bg(rgb(theme().ui_accent()))
            } else {
                style
            }
        })
        .cursor_row_resize()
        .on_drag(
            DividerDrag {
                kind: DividerKind::InspectorSplit,
            },
            |_drag, _pos, _window, cx| cx.new(|_| DividerGhost),
        );

    // ── Split region: message │ divider │ files ───────────────────────────
    // Grouped under one flex_1 column so the split ratio is relative to the
    // *remaining* height (excluding the variable-height header), and so a
    // measuring canvas can record the region's real window coordinates for
    // the drag handler (static offsets miss the header height — that was the
    // user-visible "jumps ~2cm on drag start" bug).
    let measure = {
        let geom = split_geom.clone();
        canvas(
            move |_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| {},
            move |bounds: Bounds<Pixels>, _prepaint: (), _window: &mut Window, _cx: &mut App| {
                let top = f32::from(bounds.origin.y);
                let bottom = top + f32::from(bounds.size.height);
                geom.set((top, bottom));
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    };
    let split_region = div()
        .flex_1()
        .min_h(px(0.))
        .relative()
        .flex()
        .flex_col()
        .child(measure)
        .child(message_box)
        .child(split_divider)
        .child(files_box);

    // ── Outer panel: header │ split region ────────────────────────────────
    panel.child(header_region).child(split_region)
}

/// Parse `name  <email>  date` (detail_panel format) into `(name, email)`.
///
/// `chars()`-safe: only splits on ASCII markers (`  <`, `>`), never byte-slices
/// into multi-byte sequences, so Japanese / CJK names are preserved.  Falls back
/// to the whole string as the name (and empty email) if the markers are absent.
fn parse_author(line: &str) -> (String, String) {
    if let Some(lt) = line.find("  <") {
        let name = line[..lt].trim().to_string();
        let rest = &line[lt + 3..];
        let email = match rest.find('>') {
            Some(gt) => rest[..gt].to_string(),
            None => String::new(),
        };
        (name, email)
    } else {
        (line.trim().to_string(), String::new())
    }
}

/// Tooltip entity for the short-hash chip — shows the full 40-hex SHA.
struct HashTooltip {
    sha: SharedString,
}

impl gpui::Render for HashTooltip {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut col = div()
            .flex()
            .flex_col()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(rgb(theme().elevated_surface(theme().surface)))
            .when(theme().uses_neutral_dark_chrome(), |tooltip| {
                tooltip
                    .border_1()
                    .border_color(rgb(theme().elevated_border(theme().surface)))
            })
            .text_xs()
            .text_color(rgb(theme().text_main));
        for line in self.sha.as_ref().split('\n') {
            col = col.child(div().child(SharedString::from(line.to_string())));
        }
        col
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Render a clickable action button.
fn action_button(
    id: &'static str,
    label: &'static str,
    color: u32,
    click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let btn = Button::new(id)
        .label(SharedString::from(label))
        .small()
        .on_click(click);
    // The chrome accent marks this primary action; cherry-pick remains neutral.
    let btn = if color == theme().ui_accent() {
        btn.primary()
    } else {
        btn
    };
    super::e2e::measure_control(id, btn)
}

#[cfg(test)]
mod parse_author_tests {
    use super::parse_author;

    /// The split marker is the literal **two-space** `"  <"` that
    /// `detail_panel` writes. A one-space line has no marker, so the whole
    /// line becomes the name and the email is empty — documented here so the
    /// producer and the parser cannot drift apart unnoticed.
    #[test]
    fn splits_only_on_the_two_space_marker() {
        assert_eq!(
            parse_author("Ada Lovelace  <ada@example.com>  2026-01-10"),
            ("Ada Lovelace".to_string(), "ada@example.com".to_string())
        );
        // One space: no marker -> whole line is the name.
        assert_eq!(
            parse_author("Ada Lovelace <ada@example.com>"),
            ("Ada Lovelace <ada@example.com>".to_string(), String::new())
        );
        // No angle brackets at all.
        assert_eq!(
            parse_author("  Ada Lovelace  "),
            ("Ada Lovelace".to_string(), String::new())
        );
        assert_eq!(parse_author(""), (String::new(), String::new()));
        // A `<` inside the name: the first `  <` still wins.
        assert_eq!(
            parse_author("A <B  <a@b>"),
            ("A <B".to_string(), "a@b".to_string())
        );
        // Marker present but no closing `>`: email empty, name still split.
        assert_eq!(
            parse_author("Ada  <ada@example.com"),
            ("Ada".to_string(), String::new())
        );
    }
}
