//! Command palette (issue #352): fuzzy-search the whole [`COMMANDS`] registry
//! in one overlay. Disabled commands are shown greyed-out with their
//! [`commands::command_state_in`] reason as a subtitle — the differentiator over Sublime
//! Merge, which just hides them (see issue #352 §2).
//!
//! The **pure** parts live at the top of this file (fuzzy matcher + row-model
//! builder) so they unit-test without a live GPUI window; the `impl KagiApp`
//! block at the bottom is the overlay itself (opened via `Cmd/Ctrl+P`,
//! registry id `view.commandPalette`) and needs human verification.
//!
//! Keybinding: `Cmd+Shift+P` is the conventional palette key but is already
//! taken by `view.togglePrMode`, so the nearest free key — `Cmd/Ctrl+P` — is
//! used (see `COMMANDS` entry, verified free against every `KeyBinding::new`).

use gpui::{div, prelude::*, rgb, App, Context, MouseButton, SharedString, Window};
use gpui_component::input::{Input, InputState};
use kagi_ui_core::tooltip::HoverTooltip as _;

use super::commands::{self, CommandState, MenuOverlay, COMMANDS};
use super::i18n::{self, Lang};
use super::theme::{self, theme};
use super::KagiApp;

pub use super::palette_scroll::PaletteScroll;

/// Sections are derived from existing action kinds, never selectable entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteSection {
    Actions,
    Appearance,
    Language,
}

impl PaletteSection {
    pub fn id(self) -> &'static str {
        match self {
            Self::Actions => "actions",
            Self::Appearance => "appearance",
            Self::Language => "language",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Actions => i18n::Msg::CommandPaletteActions.t(),
            Self::Appearance => i18n::Msg::SettingsAppearance.t(),
            Self::Language => i18n::Msg::SettingsLanguage.t(),
        }
    }
}

impl PaletteRow {
    pub fn section(&self) -> PaletteSection {
        match &self.action {
            PaletteAction::Command(_) => PaletteSection::Actions,
            PaletteAction::SetTheme(_) => PaletteSection::Appearance,
            PaletteAction::SetLang(_) => PaletteSection::Language,
        }
    }

    fn description(&self) -> String {
        i18n::command_palette_description(
            i18n::lang(),
            self.section().label(),
            self.disabled_reason.as_deref(),
        )
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Pure: fuzzy matcher
// ──────────────────────────────────────────────────────────────────────────

/// Self-implemented subsequence fuzzy match (no new dependency — the repo
/// minimises deps, issue #352 §5). Returns `None` when `query`'s characters do
/// **not** appear in `text` in order (case-insensitive); otherwise `Some(score)`
/// where a higher score is a better match.
///
/// Scoring rewards the matches a human reads as "closer": contiguous runs and
/// matches at a word start (string start or just after a separator) score much
/// higher than scattered letters, and longer text is very mildly penalised so a
/// short exact-ish hit outranks the same letters buried in a long label.
pub fn fuzzy_match(query: &str, text: &str) -> Option<i64> {
    let q: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();

    let mut score: i64 = 0;
    let mut qi = 0usize;
    let mut prev: Option<usize> = None;
    for (ti, tc) in t.iter().enumerate() {
        if qi < q.len() && *tc == q[qi] {
            score += 1;
            if let Some(p) = prev {
                if p + 1 == ti {
                    score += 5; // contiguous run
                }
            }
            let word_start = ti == 0 || matches!(t[ti - 1], ' ' | '-' | '_' | '.' | '…' | '/');
            if word_start {
                score += 10;
            }
            prev = Some(ti);
            qi += 1;
        }
    }
    if qi == q.len() {
        Some(score - (t.len() as i64) / 10)
    } else {
        None
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Pure: row-model builder
// ──────────────────────────────────────────────────────────────────────────

/// What running a palette row does. Static registry commands dispatch through
/// [`KagiApp::handle_menu_command`]; the two dynamic submenus (theme, language)
/// carry their target directly so **every** theme slug / language is invocable,
/// not just the handful that happen to have a static `theme.*` / `lang.*`
/// command (issue #373 — the static registry only listed 6 of the built-in
/// themes). A theme slug is owned: custom themes exist only at runtime (#922).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    /// Dispatch a static [`COMMANDS`] registry id via `handle_menu_command`.
    Command(&'static str),
    /// Switch to the theme with this slug (`set_theme`).
    SetTheme(SharedString),
    /// Switch the UI language (`set_lang`).
    SetLang(Lang),
}

/// One rendered palette entry: a command paired with its localized label, its
/// display keystroke, and its live enabled/disabled state. `disabled_reason` is
/// `Some` exactly when the command is greyed out (mirrors [`CommandState`]).
/// `action` is what Enter/click runs — a static command or a dynamic
/// theme/language switch (issue #373). `id` is the registry id, theme slug or
/// language slug.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteRow {
    pub id: SharedString,
    pub label: String,
    pub keystroke: Option<String>,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub action: PaletteAction,
}

/// Build the (filtered, ranked) palette rows over the whole [`COMMANDS`] table.
///
/// Pure — the app state is injected via closures so this tests without a live
/// `KagiApp`:
/// - `label_of` → the localized label shown for a command id,
/// - `state_of` → the command's [`CommandState`] (enabled / disabled+reason),
/// - `keystroke_of` → the display keystroke, right-aligned in the UI.
///
/// An empty `query` returns **every** command in registry order (issue #352
/// acceptance: "enumerates ALL `COMMANDS`"). A non-empty query keeps only rows
/// whose label is a fuzzy subsequence match, best score first (stable on ties).
/// Disabled commands are always included when they match — showing *why* a
/// command is unavailable is the point (issue #352 §4).
pub fn build_rows(
    query: &str,
    label_of: impl Fn(&'static str) -> String,
    state_of: impl Fn(&'static str) -> CommandState,
    keystroke_of: impl Fn(&'static str) -> Option<String>,
) -> Vec<PaletteRow> {
    let query = query.trim();
    let mut scored: Vec<(i64, usize, PaletteRow)> = Vec::new();
    for (idx, cmd) in COMMANDS.iter().enumerate() {
        // The static `theme.*` / `lang.*` commands only cover a subset of the
        // built-in themes; the palette lists them dynamically below (issue
        // #373) so skip them here to avoid duplicate rows.
        if is_dynamic_command(cmd.id) {
            continue;
        }
        let label = label_of(cmd.id);
        let score = match fuzzy_match(query, &label) {
            Some(s) => s,
            None => continue,
        };
        let (enabled, disabled_reason) = match state_of(cmd.id) {
            CommandState::Enabled => (true, None),
            CommandState::Disabled(reason) => (false, Some(reason.to_string())),
        };
        scored.push((
            score,
            idx,
            PaletteRow {
                id: SharedString::new_static(cmd.id),
                label,
                keystroke: keystroke_of(cmd.id),
                enabled,
                disabled_reason,
                action: PaletteAction::Command(cmd.id),
            },
        ));
    }
    // Dynamic rows (issue #373): one row per registered theme (built-in and
    // custom, #922) and per language, sourced from the same lists the menus
    // use (`theme::themes_by_name`, `Lang::ALL`). `idx` continues past
    // `COMMANDS.len()` so the empty-query order is commands, then themes,
    // then languages.
    let mut idx = COMMANDS.len();
    let theme_prefix = i18n::Msg::SettingsTheme.t();
    for t in theme::themes_by_name() {
        let label = format!("{theme_prefix}: {}", t.display_name());
        if let Some(score) = fuzzy_match(query, &label) {
            let slug = SharedString::from(t.slug.clone());
            scored.push((
                score,
                idx,
                PaletteRow {
                    id: slug.clone(),
                    label,
                    keystroke: None,
                    enabled: true,
                    disabled_reason: None,
                    action: PaletteAction::SetTheme(slug),
                },
            ));
        }
        idx += 1;
    }
    let lang_prefix = i18n::Msg::SettingsLanguage.t();
    for &l in Lang::ALL {
        let label = format!("{lang_prefix}: {}", l.display_name());
        if let Some(score) = fuzzy_match(query, &label) {
            scored.push((
                score,
                idx,
                PaletteRow {
                    id: SharedString::new_static(l.slug()),
                    label,
                    keystroke: None,
                    enabled: true,
                    disabled_reason: None,
                    action: PaletteAction::SetLang(l),
                },
            ));
        }
        idx += 1;
    }
    // Empty query → registry order (idx asc). Non-empty → score desc, idx asc.
    if query.is_empty() {
        scored.sort_by_key(|(_, idx, _)| *idx);
    } else {
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    }
    scored.into_iter().map(|(_, _, row)| row).collect()
}

/// Whether a static registry command is superseded by a dynamic palette row
/// (issue #373): the `theme.*` / `lang.*` ids are enumerated from
/// `theme::themes_by_name` / `Lang::ALL` instead, so they must not also appear
/// as static command rows.
fn is_dynamic_command(id: &str) -> bool {
    id.starts_with("theme.") || id.starts_with("lang.")
}

/// The localized label for a command id: English `label` from the registry, or
/// its Japanese translation when the UI language is `Ja` (domain words such as
/// Fetch / Pull / Push stay English in both, per ADR-0048).
pub fn command_label(id: &str, lang: Lang) -> &'static str {
    let en: &'static str = commands::command(id).map(|c| c.label).unwrap_or("");
    match lang {
        Lang::En => en,
        Lang::Ja => i18n::command_label_ja(id).unwrap_or(en),
    }
}

/// Live palette rows for the given query, resolved against the current app +
/// language. Thin wrapper over [`build_rows`] used by the renderer.
pub fn rows_for(app: &KagiApp, query: &str, cx: &App) -> Vec<PaletteRow> {
    let lang = i18n::lang();
    build_rows(
        query,
        |id| command_label(id, lang).to_string(),
        |id| commands::command_state_in(app, id, cx),
        |id| commands::effective_keystroke(id).map(|k| commands::display_keystroke(&k)),
    )
}

// ──────────────────────────────────────────────────────────────────────────
// UI overlay (needs human verification — the GUI cannot be exercised headless)
// ──────────────────────────────────────────────────────────────────────────

impl KagiApp {
    /// Open the command palette (registry id `view.commandPalette`,
    /// `Cmd/Ctrl+P`). Lazily builds the search `InputState`, focuses it, resets
    /// the selection, and shows the overlay.
    pub fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        klog!("palette: open");
        let scroll = self
            .command_palette_scroll
            .get_or_insert_with(PaletteScroll::new);
        scroll.set_offset(Default::default());
        if self.command_palette_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx).placeholder(i18n::Msg::CommandPalettePlaceholder.t())
            });
            // Reset and reveal the top hit on query changes. The handle applies
            // the one-shot request against the next frame's filtered children;
            // neither a Window nor a uniform row height is needed here.
            cx.subscribe(&input, |this, _input, event, cx| {
                if matches!(event, gpui_component::input::InputEvent::Change) {
                    this.command_palette_selected = 0;
                    if let Some(scroll) = &this.command_palette_scroll {
                        scroll.scroll_to_item(0);
                    }
                    cx.notify();
                }
            })
            .detach();
            self.command_palette_input = Some(input);
        } else if let Some(input) = &self.command_palette_input {
            input.update(cx, |st, cx| {
                st.set_value("", window, cx);
            });
        }
        self.command_palette_selected = 0;
        self.command_palette_pointer = Some(window.mouse_position());
        if let Some(scroll) = &self.command_palette_scroll {
            scroll.scroll_to_item(0);
        }
        self.capture_overlay_return_focus(window, cx);
        if let Some(input) = &self.command_palette_input {
            input.update(cx, |st, cx| st.focus(window, cx));
        }
        self.with_ui(|ui| ui.retire_issue_conversation_scope());
        self.menu_overlay = Some(MenuOverlay::CommandPalette);
        cx.notify();
    }

    /// Current query text in the palette input (empty when closed / uncreated).
    fn command_palette_query(&self, cx: &Context<Self>) -> String {
        self.command_palette_input
            .as_ref()
            .map(|e| e.read(cx).value().to_string())
            .unwrap_or_default()
    }

    /// Run the highlighted command (Enter). No-op on a disabled row. Closes the
    /// palette first so the command's own modal/overlay is not fought over.
    pub fn run_selected_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.command_palette_query(cx);
        let rows = rows_for(self, &query, cx);
        // Use the same clamped index as the highlight if live results shrink.
        let selected = self
            .command_palette_selected
            .min(rows.len().saturating_sub(1));
        let Some(row) = rows.get(selected) else {
            return;
        };
        let action = row.action.clone();
        self.activate_palette_row(selected, &action, row.enabled, window, cx);
    }

    /// One result identity and enabled gate for keyboard, pointer and AX entry.
    fn activate_palette_row(
        &mut self,
        index: usize,
        action: &PaletteAction,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.command_palette_selected = index;
        if enabled {
            self.close_command_palette(window, cx);
            self.run_palette_action(action, window, cx);
        }
        cx.notify();
    }

    /// Dispatch a [`PaletteAction`]: static registry command, or a dynamic
    /// theme / language switch (issue #373).
    fn run_palette_action(
        &mut self,
        action: &PaletteAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(feature = "gui-e2e")]
        super::e2e::palette::executed(action);
        match action {
            PaletteAction::Command(id) => self.handle_menu_command(id, window, cx),
            PaletteAction::SetTheme(slug) => self.set_theme(slug, cx),
            PaletteAction::SetLang(l) => self.set_lang(*l, cx),
        }
    }

    /// Move the palette highlight by `delta`, clamped to the current result set.
    fn step_command_palette_selection(&mut self, delta: i64, cx: &Context<Self>) {
        let query = self.command_palette_query(cx);
        let n = rows_for(self, &query, cx).len();
        if n == 0 {
            self.command_palette_selected = 0;
            return;
        }
        let cur = self.command_palette_selected as i64;
        self.command_palette_selected = (cur + delta).clamp(0, n as i64 - 1) as usize;
        // Event-driven, not render-driven: a mouse scroll stays put until the
        // next navigation, even when that key hits an unchanged end boundary.
        if let Some(scroll) = &self.command_palette_scroll {
            scroll.scroll_to_item(self.command_palette_selected);
        }
    }

    /// Render the command-palette overlay. Returns the wrapped element (dim
    /// backdrop + centred panel). Escape closes it via the shared
    /// `cancel_active_modal` cascade (`menu_overlay = None`).
    pub fn render_command_palette(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let query = self.command_palette_query(cx);
        let rows = rows_for(self, &query, cx);
        let selected = self
            .command_palette_selected
            .min(rows.len().saturating_sub(1));

        // Key handling on the panel (bubbles up from the focused input): Up/Down
        // move the highlight, Enter runs it. `stop_propagation` on Enter keeps
        // the root's Enter handler from also acting (it would check out a
        // commit once the overlay closed).
        let on_key = cx.listener(move |this, ev: &gpui::KeyDownEvent, window, cx| {
            match ev.keystroke.key.as_str() {
                "down" => {
                    this.step_command_palette_selection(1, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
                "up" => {
                    this.step_command_palette_selection(-1, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
                "enter" => {
                    cx.stop_propagation();
                    this.run_selected_command_palette(window, cx);
                }
                _ => {}
            }
        });

        let (width, height, top) = super::modal_shell::search_card_geometry(560., 480., 80.);
        let mut panel = div()
            .occlude()
            .id("command-palette")
            .on_key_down(on_key)
            .w(width)
            .max_h(height)
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(theme::scaled_px(8.0))
            .border_1()
            .border_color(rgb(theme().elevated_border(theme().surface)))
            .map(|panel| super::e2e::probe_chrome_border("palette-frame", panel))
            .bg(rgb(theme().elevated_surface(theme().panel)))
            .shadow_lg();
        #[cfg(feature = "gui-e2e")]
        {
            panel = panel
                .relative()
                .child(super::e2e::measure_inside("palette-card"));
        }

        // Search input.
        if let Some(input) = &self.command_palette_input {
            panel = panel.child(
                div()
                    .p(theme::scaled_px(8.0))
                    .flex_shrink_0()
                    .when(cfg!(feature = "gui-e2e"), |el| {
                        el.relative()
                            .child(super::e2e::measure_inside("palette-input"))
                    })
                    .border_b_1()
                    .border_color(rgb(theme().elevated_border(theme().surface)))
                    .map(|input| super::e2e::probe_chrome_border("palette-divider", input))
                    .child(
                        Input::new(input)
                            .context_menu(kagi_ui_core::input::context_menu(input, false))
                            .aria_label(i18n::Msg::A11yCommandPaletteQuery.t())
                            .appearance(true),
                    ),
            );
        }

        let list = self.render_palette_results(&query, &rows, selected, cx);
        let panel_el = panel.child(list).into_any_element();
        self.wrap_command_palette(panel_el, top, cx)
    }

    fn render_palette_results(
        &self,
        query: &str,
        rows: &[PaletteRow],
        selected: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        // Results list.
        let mut list = div()
            .id("command-palette-list")
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .min_h(gpui::px(0.))
            .flex_shrink(1.);
        list = super::list_a11y::list_box(
            "command-palette-list",
            list,
            i18n::Msg::CommandPalettePlaceholder.t(),
        );
        if let Some(scroll) = &self.command_palette_scroll {
            list = list.track_scroll(scroll.handle());
        }

        if rows.is_empty() {
            list = list.child(
                div()
                    .px_4()
                    .py_3()
                    .text_sm()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(i18n::Msg::CommandPaletteNoResults.t())),
            );
        }

        super::e2e::palette::begin_sections();
        if query.trim().is_empty() {
            let mut start = 0;
            while start < rows.len() {
                let section = rows[start].section();
                let end = start
                    + rows[start..]
                        .iter()
                        .take_while(|row| row.section() == section)
                        .count();
                let group =
                    palette_section(section, start..end).children((start..end).map(|i| {
                        self.render_palette_row(i, rows.len(), &rows[i], i == selected, cx)
                    }));
                let group = group.into_any_element();
                let group = match &self.command_palette_scroll {
                    Some(scroll) => scroll.measure_group(start, group),
                    None => group,
                };
                list = list.child(group);
                start = end;
            }
        } else {
            list =
                list.children(rows.iter().enumerate().map(|(i, row)| {
                    self.render_palette_row(i, rows.len(), row, i == selected, cx)
                }));
        }

        match &self.command_palette_scroll {
            Some(scroll) => scroll.wrap(list.into_any_element(), rows.len()),
            None => list.into_any_element(),
        }
    }

    fn render_palette_row(
        &self,
        index: usize,
        count: usize,
        row: &PaletteRow,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let action = row.action.clone();
        let enabled = row.enabled;
        let pointer_action = action.clone();
        let owner = cx.entity().downgrade();
        let click = cx.listener(move |this, _: &gpui::MouseDownEvent, window, cx| {
            // Keep MouseDown: the dismiss scrim can unmount before mouse-up.
            this.activate_palette_row(index, &pointer_action, enabled, window, cx);
            cx.stop_propagation();
        });

        let label: SharedString = row.label.clone().into();
        let tooltip = label.clone();
        let mut r = super::list_a11y::list_option(
            "command-palette-list",
            div().id(("palette-row", index)),
            index,
            count,
            label.clone(),
            is_selected,
        )
        .aria_description(super::e2e::palette::description(index, row.description()))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .px_4()
        .py(theme::scaled_px(6.0))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, click)
        .on_a11y_action(gpui::AccessibleAction::Click, move |_, window, cx| {
            let _ = owner.update(cx, |this, cx| {
                this.activate_palette_row(index, &action, enabled, window, cx);
            });
        })
        .on_mouse_move(
            cx.listener(move |this, ev: &gpui::MouseMoveEvent, _window, cx| {
                if this.command_palette_pointer == Some(ev.position) {
                    return;
                }
                this.command_palette_pointer = Some(ev.position);
                if this.command_palette_selected != index {
                    this.command_palette_selected = index;
                    cx.notify();
                }
            }),
        );
        if is_selected {
            r = r.bg(theme().selected_for_window(self.window_active));
        }
        r = r.child(palette_row_label(
            index,
            row,
            label,
            is_selected,
            self.window_active,
        ));

        if let Some(ks) = &row.keystroke {
            r = r.child(palette_row_shortcut(
                index,
                ks,
                is_selected,
                self.window_active,
            ));
        }
        let row = r
            .hover_tooltip_builder(("palette-row-help", index), move |window, cx| {
                gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
            })
            .into_any_element();
        match &self.command_palette_scroll {
            Some(scroll) => scroll.measure_row(index, row),
            None => row,
        }
    }

    /// Centre the palette over a dim, click-to-dismiss backdrop (mirrors
    /// `wrap_overlay`, kept local so it can align the panel to the top third).
    fn wrap_command_palette(
        &self,
        panel: gpui::AnyElement,
        top: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let dismiss = cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
            this.menu_overlay = None;
            cx.stop_propagation();
            cx.notify();
        });
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .on_mouse_move(
                cx.listener(|this, ev: &gpui::MouseMoveEvent, _window, _cx| {
                    // Track movement outside rows too, without changing the target.
                    this.command_palette_pointer = Some(ev.position);
                }),
            )
            .flex_col()
            .items_center()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(rgb(theme().bg_base))
                    .opacity(0.55)
                    .on_mouse_down(MouseButton::Left, dismiss),
            )
            .child(div().h(top).flex_shrink_0())
            .child(panel)
            .into_any_element()
    }
}

fn palette_section(
    section: PaletteSection,
    rows: std::ops::Range<usize>,
) -> gpui::Stateful<gpui::Div> {
    super::e2e::palette::section(
        section.id(),
        section.label(),
        rows,
        div().id(section.id()).flex().flex_col().flex_shrink_0(),
    )
    .child(
        div()
            .px_4()
            .py(theme::scaled_px(6.))
            .text_xs()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(rgb(theme().text_muted))
            .when(cfg!(feature = "gui-e2e"), |heading| {
                heading.relative().child(super::e2e::measure_inside(format!(
                    "palette-heading-{}",
                    section.id()
                )))
            })
            .child(section.label()),
    )
}

fn palette_row_shortcut(
    index: usize,
    keystroke: &str,
    is_selected: bool,
    window_active: bool,
) -> gpui::Div {
    div()
        .flex_shrink_0()
        .text_right()
        .when(cfg!(feature = "gui-e2e"), |el| {
            el.relative().child(super::e2e::measure_inside(format!(
                "palette-shortcut-{index}"
            )))
        })
        .text_xs()
        .text_color(
            if theme().selection_uses_on_accent(is_selected, window_active) {
                theme().selection_secondary_foreground(window_active)
            } else {
                rgb(theme().text_muted).into()
            },
        )
        .child(super::e2e::palette::shortcut(index, keystroke.to_string()))
}

fn palette_row_label(
    index: usize,
    row: &PaletteRow,
    text: SharedString,
    is_selected: bool,
    window_active: bool,
) -> gpui::Div {
    let uses_on_accent = theme().selection_uses_on_accent(is_selected, window_active);
    let label_color = if uses_on_accent {
        theme().selection_foreground(window_active)
    } else {
        rgb(if row.enabled {
            theme().text_main
        } else {
            theme().text_muted
        })
        .into()
    };
    let mut label = div().flex_1().min_w(gpui::px(0.)).flex().flex_col().child(
        div()
            .truncate()
            .when(cfg!(feature = "gui-e2e"), |el| {
                el.relative()
                    .child(super::e2e::measure_inside(format!("palette-label-{index}")))
            })
            .text_sm()
            .text_color(label_color)
            .child(text),
    );
    if let Some(reason) = &row.disabled_reason {
        label = label.child(
            div()
                .text_xs()
                .text_color(if uses_on_accent {
                    theme().selection_secondary_foreground(window_active)
                } else {
                    rgb(theme().text_muted).into()
                })
                .child(SharedString::from(reason.clone())),
        );
    }
    label
}

#[cfg(test)]
#[path = "command_palette_tests.rs"]
mod tests;
