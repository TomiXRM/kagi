//! The diff header row every diff embedding shares: `[leading] file name
//! [trailing] Unified | Split +N −M` (ADR-0124, #809, #1235).

use gpui::prelude::*;
use gpui::{div, rgb, Context, SharedString};
use gpui_component::Selectable as _;
use kagi_ui_core::header_fit::{header_button, HeaderFit};
use kagi_ui_core::tooltip::HoverTooltip as _;

use super::theme::theme;

/// What a diff embedding puts in the diff header besides the file name, the
/// view-mode segments and the +N −M stats.
#[derive(Default)]
pub(crate) struct DiffHeader {
    /// #809: measures the row so its buttons fall back to icons rather than
    /// being clipped. `None` keeps the labels unconditionally.
    pub(crate) fit: Option<HeaderFit>,
    pub(crate) leading: Option<gpui::AnyElement>,
    pub(crate) trailing: Option<gpui::AnyElement>,
    /// Labels of the `leading` / `trailing` buttons, for the fit's labelled
    /// copy (the header adds both mode labels).
    pub(crate) labels: Vec<SharedString>,
    /// #351: per-row gutter markers and expansions the embedding adds to the
    /// list below the header (the PR diff's review threads).
    pub(crate) overlay: Option<std::rc::Rc<dyn super::row_overlay::RowOverlay>>,
    /// Retiring PR layout, used only for a same-source mode transition.
    pub(crate) previous_overlay: Option<std::rc::Rc<super::row_overlay::Projection>>,
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
        // #809: without a fit (the editor / PR embeddings) the labels always
        // show.
        let compact = fit.as_ref().is_some_and(HeaderFit::compact);
        let modes = mode_segments(fit.as_ref(), compact, &mut labels, cx);
        let stats_el = div()
            .text_sm()
            .text_color(rgb(theme().text_sub))
            .child(stats.clone());
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
            .child(title_identity(title))
            // History button (导线 #3)
            .when_some(trailing, |el, btn| el.child(btn))
            .child(modes)
            .child(stats_el)
            .children(probes.into_iter().flatten())
            .into_any_element()
    }
}

/// Layout shortens paint only; hover and AX retain the same complete identity.
fn title_identity(title: SharedString) -> impl IntoElement {
    // Most paths are clean. Keep their SharedString instead of allocating a
    // second copy on every render; use the shared sanitizer for control bytes.
    let title = if title
        .chars()
        .any(|ch| ch.is_control() && ch != '\t' && ch != '\n')
    {
        super::safe_text(&title)
    } else {
        title
    };
    let role = gpui::Role::Label;
    let tooltip = title.clone();
    #[cfg(feature = "gui-e2e")]
    super::super::e2e::diff_presentation::record_header(&title, Some(role), Some(&title));
    div()
        .id("main-diff-title")
        .role(role)
        .aria_label(title.clone())
        .aria_description(title.clone())
        .flex_1()
        .text_sm()
        .text_color(rgb(theme().text_main))
        .truncate()
        .relative()
        .child(super::super::e2e::measure_inside("main-diff-title"))
        .child(title)
        .hover_tooltip_builder("main-diff-title-hover-help", move |window, cx| {
            #[cfg(feature = "gui-e2e")]
            super::super::e2e::diff_presentation::record_header_tooltip(&tooltip);
            gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
        })
}

fn mode_segments<V: 'static>(
    fit: Option<&HeaderFit>,
    compact: bool,
    labels: &mut Vec<SharedString>,
    cx: &mut Context<V>,
) -> gpui::AnyElement {
    let split = super::theme::diff_split();
    let mut modes = div().flex().flex_row().flex_shrink_0().gap_1();
    for (id, label, icon, target) in [
        (
            "diff-mode-unified",
            super::i18n::Msg::DiffViewUnified.t(),
            "icons/square-menu.svg",
            false,
        ),
        (
            "diff-mode-split",
            super::i18n::Msg::DiffViewSplit.t(),
            "icons/columns-2.svg",
            true,
        ),
    ] {
        let selected = split == target;
        let button = header_button(id, label, icon, compact, |button| {
            button
                .selected(selected)
                .on_click(cx.listener(move |_this, _ev, _window, cx| {
                    if super::theme::diff_split() != target {
                        super::theme::set_diff_split(target);
                        klog!("diff-mode: {}", if target { "split" } else { "unified" });
                        cx.notify();
                    }
                }))
        });
        #[cfg(feature = "gui-e2e")]
        super::super::e2e::diff_presentation::record_mode(id, gpui::Role::Button, label, selected);
        modes = modes.child(match fit {
            Some(fit) => fit.control(id, button),
            None => button.into_any_element(),
        });
        // Measuring both labels keeps the existing title → icons → stats
        // priority. The probe's gap_2 is conservative for this gap_1 group.
        labels.push(SharedString::from(label));
    }
    modes.into_any_element()
}
