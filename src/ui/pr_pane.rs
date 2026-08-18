//! Pull Requests pane (GitHub Phase 1b) — a center takeover like Branch
//! Cleanup: filter chips, a stack-ordered table, and a read-only "peek".
//!
//! Peek = the Compare pane over merge-base(base, head) → head (GitHub's
//! three-dot diff), built from the fetched `origin/*` tips — nothing is
//! checked out, which is the point for reading an agent's PR mid-run.

use gpui::{div, prelude::*, px, rgb, Context, SharedString};
use kagi_domain::github::{stack_order, CiState, PrGroup, PullRequest, ReviewState};

use super::i18n::Msg;
use super::theme::{self, theme};
use super::types::ToastKind;
use super::{CompareTarget, CompareView, FooterStatus, KagiApp};

/// Which PRs the pane lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrFilter {
    #[default]
    Mine,
    ReviewRequested,
    All,
}

const PAD: f32 = 16.0;
const ROW_H: f32 = 26.0;

impl KagiApp {
    pub fn toggle_pr_pane(&mut self, cx: &mut Context<Self>) {
        if self.pr_pane_open {
            self.pr_pane_open = false;
        } else {
            if self.repo_path.is_none() {
                return;
            }
            self.pr_pane_open = true;
            // With no login the Mine filter degrades to "has a local branch";
            // start on All so an empty pane doesn't look broken.
            if self.github_login.is_none() && self.pr_pane_filter == PrFilter::Mine {
                self.pr_pane_filter = PrFilter::All;
            }
            klog!("pr-pane: opened");
        }
        cx.notify();
    }

    pub fn close_pr_pane(&mut self, cx: &mut Context<Self>) {
        self.pr_pane_open = false;
        cx.notify();
    }

    /// Read-only PR peek: Compare pane over merge-base(base, head) → head using
    /// the fetched remote tips. Both branches must exist as `origin/…`.
    pub fn open_pr_peek(&mut self, pr: &PullRequest, cx: &mut Context<Self>) {
        let tip = |name: &str| {
            self.active_view
                .remote_branches
                .iter()
                .find(|rb| rb.name == name)
                .map(|rb| rb.target.clone())
        };
        let (Some(base_tip), Some(head_tip)) = (tip(&pr.base), tip(&pr.head)) else {
            self.push_toast(
                ToastKind::Info,
                SharedString::from(format!(
                    "{}: {} / {}",
                    Msg::PrBranchNotFetched.t(),
                    pr.base,
                    pr.head
                )),
                cx,
            );
            return;
        };
        let Some(session) = self.repo_session.as_ref() else {
            return;
        };
        let repo = session.backend();
        let base = repo.merge_base(&base_tip, &head_tip).unwrap_or(base_tip);
        match repo.compare_commits(&base, &head_tip) {
            Ok(files) => {
                klog!("pr-peek: #{} files={}", pr.number, files.len());
                if let Some(row) = self.row_for_commit_id(&head_tip) {
                    if self.selected != Some(row) {
                        self.select(row);
                    }
                }
                self.main_diff = None;
                let view = CompareView {
                    base,
                    target: CompareTarget::Commit(head_tip),
                    files,
                    title: SharedString::from(format!("#{} {}", pr.number, pr.head)),
                };
                self.show_compare(view, cx);
            }
            Err(e) => {
                klog!("pr-peek: error: {}", e);
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("PR peek failed: {}", e)));
            }
        }
    }
}

/// The takeover pane.
pub fn render_pr_pane(app: &mut KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let login = app.github_login.clone();
    let local: Vec<String> = app
        .active_view
        .branches
        .iter()
        .map(|(n, _)| n.clone())
        .collect();
    let all = app.github_prs.clone();
    let filter = app.pr_pane_filter;
    let visible: Vec<PullRequest> = all
        .iter()
        .filter(|p| match filter {
            PrFilter::All => true,
            PrFilter::Mine => p.group_for(login.as_deref(), &local) == PrGroup::Mine,
            PrFilter::ReviewRequested => {
                p.group_for(login.as_deref(), &local) == PrGroup::ReviewRequested
            }
        })
        .cloned()
        .collect();
    let order = stack_order(&visible);

    // ── Header: title, count, filter chips, close ─────────────
    let chip =
        |id: &'static str, label: String, this_filter: PrFilter, cx: &mut Context<KagiApp>| {
            let on = filter == this_filter;
            let handler = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                this.pr_pane_filter = this_filter;
                cx.notify();
            });
            div()
                .id(id)
                .px_2()
                .py_1()
                .rounded(theme::scaled_px(4.))
                .text_xs()
                .border_1()
                .border_color(rgb(if on {
                    theme().color_branch
                } else {
                    theme().selected
                }))
                .text_color(rgb(if on {
                    theme().color_branch
                } else {
                    theme().text_sub
                }))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(theme().surface)))
                .on_click(handler)
                .child(SharedString::from(label))
        };
    let count_of = |f: PrFilter| {
        all.iter()
            .filter(|p| match f {
                PrFilter::All => true,
                PrFilter::Mine => p.group_for(login.as_deref(), &local) == PrGroup::Mine,
                PrFilter::ReviewRequested => {
                    p.group_for(login.as_deref(), &local) == PrGroup::ReviewRequested
                }
            })
            .count()
    };
    let close = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.close_pr_pane(cx);
    });
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px(theme::scaled_px(PAD))
        .py_3()
        .child(
            div()
                .text_xl()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(Msg::PrPaneTitle.t())),
        )
        .child(chip(
            "pr-filter-mine",
            format!("{} ({})", Msg::PrGroupMine.t(), count_of(PrFilter::Mine)),
            PrFilter::Mine,
            cx,
        ))
        .child(chip(
            "pr-filter-review",
            format!(
                "{} ({})",
                Msg::PrGroupReview.t(),
                count_of(PrFilter::ReviewRequested)
            ),
            PrFilter::ReviewRequested,
            cx,
        ))
        .child(chip(
            "pr-filter-all",
            format!("{} ({})", Msg::PrGroupAll.t(), count_of(PrFilter::All)),
            PrFilter::All,
            cx,
        ))
        .child(div().flex_1())
        .child(
            div()
                .id("pr-pane-close")
                .px_2()
                .py_1()
                .rounded(theme::scaled_px(4.))
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .cursor_pointer()
                .hover(|s| {
                    s.bg(rgb(theme().surface))
                        .text_color(rgb(theme().text_main))
                })
                .on_click(close)
                .child(SharedString::from("✕")),
        );

    // ── Column header ─────────────────────────────────────────
    let col = |w: f32, text: &str| {
        div()
            .w(theme::scaled_px(w))
            .flex_shrink_0()
            .overflow_hidden()
            .text_xs()
            .text_color(rgb(theme().text_label))
            .child(SharedString::from(text.to_string()))
    };
    let col_header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px(theme::scaled_px(PAD))
        .py_1()
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(col(64., "#"))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_xs()
                .text_color(rgb(theme().text_label))
                .child(SharedString::from(Msg::PrColTitle.t())),
        )
        .child(col(110., Msg::PrColAuthor.t()))
        .child(col(220., Msg::PrColBranches.t()))
        .child(col(40., "CI"))
        .child(col(80., Msg::PrColReview.t()));

    // ── Rows (stack-ordered, indented by depth) ───────────────
    let mut list = div()
        .id("pr-pane-list")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    if visible.is_empty() {
        list = list.child(
            div()
                .px(theme::scaled_px(PAD))
                .py_3()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::PrPaneEmpty.t())),
        );
    }
    for (ix, depth) in order {
        let pr = &visible[ix];
        list = list.child(render_pr_row(pr, depth, cx));
    }

    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(rgb(theme().bg_base))
        .child(header)
        .child(col_header)
        .child(list)
        .into_any_element()
}

fn render_pr_row(pr: &PullRequest, depth: usize, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let (ci_glyph, ci_color) = match pr.ci {
        CiState::Success => ("\u{2713}", theme().color_success),
        CiState::Failure => ("\u{2717}", theme().color_blocker),
        CiState::Pending => ("\u{25CF}", theme().color_warning),
        CiState::None => ("\u{25CB}", theme().text_muted),
    };
    let (rv_text, rv_color) = match pr.review {
        ReviewState::Approved => (Msg::PrReviewApproved.t(), theme().color_success),
        ReviewState::ChangesRequested => (Msg::PrReviewChanges.t(), theme().color_warning),
        ReviewState::ReviewRequired => (Msg::PrReviewRequired.t(), theme().text_sub),
        ReviewState::None => ("", theme().text_muted),
    };
    let pr_peek = pr.clone();
    let click = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.open_pr_peek(&pr_peek, cx);
        cx.notify();
    });
    let pr_menu = pr.clone();
    let menu = cx.listener(
        move |this: &mut KagiApp, e: &gpui::MouseDownEvent, _w, cx| {
            this.pr_menu = Some((pr_menu.clone(), e.position));
            cx.stop_propagation();
            cx.notify();
        },
    );
    let title = if pr.is_draft {
        format!("{} ({})", pr.title, Msg::PrDraft.t())
    } else {
        pr.title.clone()
    };
    let cell = |w: f32| {
        div()
            .w(theme::scaled_px(w))
            .flex_shrink_0()
            .overflow_hidden()
            .truncate()
    };
    div()
        .id(("pr-pane-row", pr.number as usize))
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .h(theme::scaled_px(ROW_H))
        .px(theme::scaled_px(PAD))
        .text_sm()
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(click)
        .on_mouse_down(gpui::MouseButton::Right, menu)
        .when(pr.is_draft, |el| el.opacity(0.6))
        .child(
            cell(64.)
                .text_color(rgb(theme().color_branch))
                .child(SharedString::from(format!("#{}", pr.number))),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_row()
                .items_center()
                // Stack depth: indent + a "└" so the chain reads as a tree.
                .pl(theme::scaled_px(depth as f32 * 18.))
                .when(depth > 0, |el| {
                    el.child(
                        div()
                            .flex_shrink_0()
                            .mr_1()
                            .text_color(rgb(theme().text_muted))
                            .child(SharedString::from("\u{2514}")),
                    )
                })
                .child(
                    div()
                        .truncate()
                        .text_color(rgb(theme().text_main))
                        .child(SharedString::from(title)),
                ),
        )
        .child(
            cell(110.)
                .text_color(rgb(theme().text_sub))
                .child(SharedString::from(pr.author.clone())),
        )
        .child(
            cell(220.)
                .text_color(rgb(theme().text_sub))
                .child(SharedString::from(format!(
                    "{} \u{2192} {}",
                    pr.head, pr.base
                ))),
        )
        .child(
            cell(40.)
                .text_color(rgb(ci_color))
                .child(SharedString::from(ci_glyph)),
        )
        .child(
            cell(80.)
                .text_color(rgb(rv_color))
                .child(SharedString::from(rv_text.to_string())),
        )
        .into_any_element()
}
