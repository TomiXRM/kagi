//! The diff header row every diff embedding shares: `[leading] file name
//! [trailing] view-mode toggle +N −M` (ADR-0124, #809).

use gpui::prelude::*;
use gpui::{div, rgb, Context, SharedString};
use kagi_ui_core::header_fit::{header_button, HeaderFit};

use super::theme::theme;

/// What a diff embedding puts in the diff header besides the file name, the
/// view-mode toggle and the +N −M stats.
#[derive(Default)]
pub(crate) struct DiffHeader {
    /// #809: measures the row so its buttons fall back to icons rather than
    /// being clipped. `None` keeps the labels unconditionally.
    pub(crate) fit: Option<HeaderFit>,
    pub(crate) leading: Option<gpui::AnyElement>,
    pub(crate) trailing: Option<gpui::AnyElement>,
    /// Labels of the `leading` / `trailing` buttons, for the fit's labelled
    /// copy (the header adds its own toggle).
    pub(crate) labels: Vec<SharedString>,
    /// #351: per-row gutter markers and expansions the embedding adds to the
    /// list below the header (the PR diff's review threads).
    pub(crate) overlay: Option<std::rc::Rc<dyn super::row_overlay::RowOverlay>>,
}

impl DiffHeader {
    /// Only a leading element, with no fit (the PR conflict jump nav).
    pub(crate) fn leading(leading: Option<gpui::AnyElement>) -> Self {
        Self {
            leading,
            ..Self::default()
        }
    }

    /// The header row for a diff titled `title` with `stats` (`+N −M`).
    pub(crate) fn render<V: 'static>(
        self,
        title: SharedString,
        stats: SharedString,
        cx: &mut Context<V>,
    ) -> gpui::AnyElement {
        let Self {
            fit,
            leading,
            trailing,
            mut labels,
            ..
        } = self;
        // ADR-0124: unified ⇄ side-by-side toggle. The label names the mode
        // the click switches TO; the flag is global (kagi-ui-core atomic) and
        // persisted, so every diff embedding follows the same mode.
        let (mode_label, mode_icon) = if super::theme::diff_split() {
            (
                super::i18n::Msg::DiffViewUnified.t(),
                "icons/square-menu.svg",
            )
        } else {
            (super::i18n::Msg::DiffViewSplit.t(), "icons/columns-2.svg")
        };
        // #809: without a fit (the editor / PR embeddings) the labels always
        // show.
        let compact = fit.as_ref().is_some_and(HeaderFit::compact);
        // Small outline, matching the Back/History buttons it sits beside
        // (see `header_button` — ghost buttons vanish against the header bar).
        let mode_toggle = header_button("diff-mode-toggle", mode_label, mode_icon, compact)
            .on_click(cx.listener(|_this, _ev, _window, cx| {
                let on = !super::theme::diff_split();
                super::theme::set_diff_split(on);
                klog!("diff-mode: {}", if on { "split" } else { "unified" });
                cx.notify();
            }));
        labels.push(SharedString::from(mode_label));
        let stats_el = div()
            .text_sm()
            .text_color(rgb(theme().text_sub))
            .child(stats.clone());
        let mode_toggle = match &fit {
            Some(fit) => fit.control("diff-mode-toggle", mode_toggle),
            None => mode_toggle.into_any_element(),
        };
        // The stats give way last: once the buttons are icons, they truncate.
        let stats_el = match &fit {
            Some(fit) => fit.text_control("main-diff-stats", stats_el.truncate()),
            None => stats_el.flex_shrink_0().into_any_element(),
        };
        let probes = fit
            .as_ref()
            .map(|fit| fit.probes(&labels, std::slice::from_ref(&stats)));

        div()
            .id("main-diff-header")
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .flex_shrink_0()
            .px_3()
            .py_1()
            .gap_2()
            // Icon-only buttons sit closer together.
            .when(compact, |el| el.gap_1())
            // Darker than the `surface` this used to be, so the outlined
            // buttons above read as raised controls against the chrome
            // instead of blending into it (user report).
            .bg(rgb(theme().bg_base))
            // ← Back button (only for the standalone main diff; the File
            // History view embeds this diff and has its own Back).
            .when_some(leading, |el, btn| el.child(btn))
            // File name: gives up its width before any button does
            // (`truncate` clips, so its flex min width is 0).
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(rgb(theme().text_main))
                    .truncate()
                    .child(title),
            )
            // History button (导线 #3)
            .when_some(trailing, |el, btn| el.child(btn))
            .child(mode_toggle)
            .child(stats_el)
            .children(probes.into_iter().flatten())
            .into_any_element()
    }
}
