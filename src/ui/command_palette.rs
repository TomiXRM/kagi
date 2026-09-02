//! Command palette (issue #352): fuzzy-search the whole [`COMMANDS`] registry
//! in one overlay. Disabled commands are shown greyed-out with their
//! [`command_state`] reason as a subtitle — the differentiator over Sublime
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

use gpui::{div, prelude::*, rgb, Context, MouseButton, SharedString, Window};
use gpui_component::input::{Input, InputState};

use super::commands::{self, command_state, CommandState, MenuOverlay, COMMANDS};
use super::i18n::{self, Lang};
use super::theme::{self, theme};
use super::KagiApp;

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

/// One rendered palette entry: a command paired with its localized label, its
/// display keystroke, and its live enabled/disabled state. `disabled_reason` is
/// `Some` exactly when the command is greyed out (mirrors [`CommandState`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteRow {
    pub id: &'static str,
    pub label: String,
    pub keystroke: Option<String>,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
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
                id: cmd.id,
                label,
                keystroke: keystroke_of(cmd.id),
                enabled,
                disabled_reason,
            },
        ));
    }
    // Empty query → registry order (idx asc). Non-empty → score desc, idx asc.
    if query.is_empty() {
        scored.sort_by_key(|(_, idx, _)| *idx);
    } else {
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    }
    scored.into_iter().map(|(_, _, row)| row).collect()
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
pub fn rows_for(app: &KagiApp, query: &str) -> Vec<PaletteRow> {
    let lang = i18n::lang();
    build_rows(
        query,
        |id| command_label(id, lang).to_string(),
        |id| command_state(app, id),
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
        if self.command_palette_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx).placeholder(i18n::Msg::CommandPalettePlaceholder.t())
            });
            // Re-render (and reset the highlight to the top hit) on every
            // keystroke; `Change` carries no `Window`, which is fine — filtering
            // only needs `cx.notify()`.
            cx.subscribe(&input, |this, _input, event, cx| {
                if matches!(event, gpui_component::input::InputEvent::Change) {
                    this.command_palette_selected = 0;
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
        if let Some(input) = &self.command_palette_input {
            input.update(cx, |st, cx| st.focus(window, cx));
        }
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
        let rows = rows_for(self, &query);
        let Some(row) = rows.get(self.command_palette_selected) else {
            return;
        };
        if !row.enabled {
            return;
        }
        let id = row.id;
        self.menu_overlay = None;
        self.handle_menu_command(id, window, cx);
        cx.notify();
    }

    /// Move the palette highlight by `delta`, clamped to the current result set.
    fn step_command_palette_selection(&mut self, delta: i64, cx: &Context<Self>) {
        let query = self.command_palette_query(cx);
        let n = rows_for(self, &query).len();
        if n == 0 {
            self.command_palette_selected = 0;
            return;
        }
        let cur = self.command_palette_selected as i64;
        self.command_palette_selected = (cur + delta).clamp(0, n as i64 - 1) as usize;
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
        let rows = rows_for(self, &query);
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

        let mut panel = div()
            .occlude()
            .id("command-palette")
            .on_key_down(on_key)
            .w(theme::scaled_px(560.0))
            .max_h(theme::scaled_px(480.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(theme::scaled_px(8.0))
            .border_1()
            .border_color(rgb(theme().selected))
            .bg(rgb(theme().panel))
            .shadow_lg();

        // Search input.
        if let Some(input) = &self.command_palette_input {
            panel = panel.child(
                div()
                    .p(theme::scaled_px(8.0))
                    .border_b_1()
                    .border_color(rgb(theme().selected))
                    .child(Input::new(input).appearance(true)),
            );
        }

        // Results list.
        let mut list = div()
            .id("command-palette-list")
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .max_h(theme::scaled_px(420.0));

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

        for (i, row) in rows.iter().enumerate() {
            list = list.child(self.render_palette_row(i, row, i == selected, cx));
        }

        let panel_el = panel.child(list).into_any_element();
        self.wrap_command_palette(panel_el, cx)
    }

    fn render_palette_row(
        &self,
        index: usize,
        row: &PaletteRow,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let id = row.id;
        let enabled = row.enabled;
        let click = cx.listener(move |this, _: &gpui::MouseDownEvent, window, cx| {
            // MouseDown (not click): the dismiss scrim would unmount the overlay
            // on the same down-event before a click could complete — same fix as
            // the branch picker (see commands.rs).
            this.command_palette_selected = index;
            if enabled {
                this.menu_overlay = None;
                this.handle_menu_command(id, window, cx);
            }
            cx.stop_propagation();
            cx.notify();
        });

        let label_color = if enabled {
            theme().text_main
        } else {
            theme().text_muted
        };

        // Left: label (+ disabled reason subtitle). Right: keystroke.
        let mut left = div().flex().flex_col().child(
            div()
                .text_sm()
                .text_color(rgb(label_color))
                .child(SharedString::from(row.label.clone())),
        );
        if let Some(reason) = &row.disabled_reason {
            left = left.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(reason.clone())),
            );
        }

        let mut r = div()
            .id(("palette-row", index))
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px_4()
            .py(theme::scaled_px(6.0))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, click);
        if is_selected {
            r = r.bg(rgb(theme().selected));
        }
        r = r.hover(|s| s.bg(rgb(theme().selected))).child(left);

        if let Some(ks) = &row.keystroke {
            r = r.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(ks.clone())),
            );
        }
        r.into_any_element()
    }

    /// Centre the palette over a dim, click-to-dismiss backdrop (mirrors
    /// `wrap_overlay`, kept local so it can align the panel to the top third).
    fn wrap_command_palette(
        &self,
        panel: gpui::AnyElement,
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
            .child(div().h(theme::scaled_px(80.0)))
            .child(panel)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── fuzzy matcher ──────────────────────────────────────────────────

    #[test]
    fn fuzzy_empty_query_matches_anything() {
        assert_eq!(fuzzy_match("", "anything"), Some(0));
    }

    #[test]
    fn fuzzy_subsequence_matches_and_is_case_insensitive() {
        // "nb" is a subsequence of "New Branch…" (N…B).
        assert!(fuzzy_match("nb", "New Branch…").is_some());
        assert!(fuzzy_match("NB", "new branch…").is_some());
        // "push" is a contiguous substring.
        assert!(fuzzy_match("push", "Push").is_some());
    }

    #[test]
    fn fuzzy_non_subsequence_returns_none() {
        // A char not present at all → None.
        assert_eq!(fuzzy_match("zzz", "New Branch"), None);
        // Order matters: 'h' only appears at the very end, so no 'b' follows it.
        assert_eq!(fuzzy_match("hb", "New Branch"), None);
    }

    #[test]
    fn fuzzy_word_start_beats_scattered() {
        // Both contain the letters, but a word-start / contiguous hit must
        // outrank a scattered one. This is what makes ranking meaningful.
        let contiguous = fuzzy_match("branch", "New Branch").unwrap();
        // Same letters, but interleaved with 'x' so none are contiguous and only
        // the first sits at a word start.
        let scattered = fuzzy_match("branch", "bxrxaxnxcxh").unwrap();
        assert!(
            contiguous > scattered,
            "contiguous {contiguous} should beat scattered {scattered}"
        );
    }

    // ── row-model builder ──────────────────────────────────────────────

    fn all_enabled(_id: &'static str) -> CommandState {
        CommandState::Enabled
    }
    fn no_keys(_id: &'static str) -> Option<String> {
        None
    }

    #[test]
    fn build_rows_empty_query_enumerates_all_commands() {
        let rows = build_rows("", |id| id.to_string(), all_enabled, no_keys);
        assert_eq!(rows.len(), COMMANDS.len());
        // Registry order is preserved.
        for (row, cmd) in rows.iter().zip(COMMANDS.iter()) {
            assert_eq!(row.id, cmd.id);
        }
    }

    #[test]
    fn build_rows_fuzzy_filters_to_subsequence_matches() {
        // Label the commands by their own English registry label so we search
        // real labels. "push" should surface repo.push and only matching rows.
        let rows = build_rows(
            "push",
            |id| command_label(id, Lang::En).to_string(),
            all_enabled,
            no_keys,
        );
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|r| fuzzy_match("push", &r.label).is_some()));
        assert!(rows.iter().any(|r| r.id == "repo.push"));
        // A command with no 'push' subsequence is excluded.
        assert!(rows.iter().all(|r| r.id != "repo.fetch"));
    }

    #[test]
    fn build_rows_disabled_command_carries_command_state_reason() {
        let state_of = |id: &'static str| {
            if id == "repo.push" {
                CommandState::Disabled("no repo open")
            } else {
                CommandState::Enabled
            }
        };
        let rows = build_rows("", |id| id.to_string(), state_of, no_keys);
        let push = rows.iter().find(|r| r.id == "repo.push").unwrap();
        assert!(!push.enabled);
        assert_eq!(push.disabled_reason.as_deref(), Some("no repo open"));
        // An enabled row has no reason.
        let other = rows.iter().find(|r| r.id == "repo.fetch").unwrap();
        assert!(other.enabled);
        assert_eq!(other.disabled_reason, None);
    }

    #[test]
    fn build_rows_keystroke_is_carried_for_the_right_edge() {
        let keys = |id: &'static str| {
            if id == "repo.push" {
                Some("Cmd+Shift+K".to_string())
            } else {
                None
            }
        };
        let rows = build_rows("", |id| id.to_string(), all_enabled, keys);
        let push = rows.iter().find(|r| r.id == "repo.push").unwrap();
        assert_eq!(push.keystroke.as_deref(), Some("Cmd+Shift+K"));
    }

    #[test]
    fn selected_command_id_resolves_from_row() {
        // "test: selecting a command runs it" — the run path is
        // `rows[selected].id → handle_menu_command`. Assert the row model
        // carries the id that the click/Enter handlers dispatch.
        let rows = build_rows(
            "settings",
            |id| command_label(id, Lang::En).to_string(),
            all_enabled,
            no_keys,
        );
        assert_eq!(rows.first().map(|r| r.id), Some("app.settings"));
    }

    // ── i18n: EN + JA both render ──────────────────────────────────────

    #[test]
    fn command_label_renders_en_and_ja() {
        // A localizable label differs between languages…
        let en = command_label("app.settings", Lang::En);
        let ja = command_label("app.settings", Lang::Ja);
        assert_eq!(en, "Settings…");
        assert_eq!(ja, "設定…");
        assert_ne!(en, ja);
        // …while a domain word stays English in both (ADR-0048).
        assert_eq!(command_label("repo.push", Lang::En), "Push");
        assert_eq!(command_label("repo.push", Lang::Ja), "Push");
    }

    #[test]
    fn every_command_has_a_ja_label_or_is_an_english_domain_word() {
        // Guard: no command renders as its raw id in Japanese (that would be a
        // missing translation, not a deliberate domain word).
        for cmd in COMMANDS {
            let ja = command_label(cmd.id, Lang::Ja);
            assert_ne!(ja, cmd.id, "command {} has no JA label", cmd.id);
        }
    }
}
