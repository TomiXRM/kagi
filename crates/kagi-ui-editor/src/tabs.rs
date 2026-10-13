//! Editor buffer tabs: stable identity, bounded width and native horizontal overflow.
use crate::EditorWorkspaceView;
use gpui::{canvas, div, prelude::*, px, rgb, Bounds, Context, Role, SharedString, Size, Window};
use gpui_component::{
    scroll::{Scrollbar, ScrollbarShow},
    tooltip::Tooltip,
};
use kagi_ui_core::theme::{self, theme};
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
};

impl EditorWorkspaceView {
    /// Actual Editor body coordinates, excluding any host-owned compact rail.
    pub fn pane_bounds(&self, window: &Window) -> Bounds<gpui::Pixels> {
        let bounds = self.tab_presentation.pane_bounds.get();
        if bounds.size.width > px(0.) {
            bounds
        } else {
            Bounds::new(gpui::Point::default(), window.viewport_size())
        }
    }

    /// Render-only compression shared with divider input's inverse conversion.
    pub fn pane_width_fraction(&self, available: gpui::Pixels) -> f32 {
        let tree = if self.show_tree { self.tree_w } else { 0. };
        let total = theme::scaled_px(tree + self.hunks_w);
        let dividers = theme::scaled_px(if self.show_tree { 8. } else { 4. });
        let budget = (available - theme::scaled_px(180.) - dividers).max(px(0.));
        if total > budget {
            f32::from(budget) / f32::from(total)
        } else {
            1.
        }
    }

    /// Divider coordinates are already unzoomed, but still presentation-clamped.
    pub fn pane_width_from_rendered(&self, rendered: f32, available: gpui::Pixels) -> f32 {
        rendered / self.pane_width_fraction(available).max(f32::EPSILON)
    }
}

pub(crate) fn pane_widths(
    view: &EditorWorkspaceView,
    available: gpui::Pixels,
) -> (gpui::Pixels, gpui::Pixels) {
    let fraction = view.pane_width_fraction(available);
    let tree = if view.show_tree {
        theme::scaled_px(view.tree_w) * fraction
    } else {
        px(0.)
    };
    (tree, theme::scaled_px(view.hunks_w) * fraction)
}

struct TabIdentity {
    path: PathBuf,
    label: SharedString,
    full_path: SharedString,
}

#[derive(Default)]
pub(crate) struct TabPresentation {
    identities: RefCell<Vec<TabIdentity>>,
    active: RefCell<Option<PathBuf>>,
    zoom: Cell<f32>,
    viewport: Rc<Cell<Size<gpui::Pixels>>>,
    pane_bounds: Rc<Cell<Bounds<gpui::Pixels>>>,
}

pub(crate) fn measure_pane_bounds(
    view: &EditorWorkspaceView,
    cx: &Context<EditorWorkspaceView>,
) -> impl gpui::IntoElement {
    let geometry = view.tab_presentation.pane_bounds.clone();
    let entity = cx.weak_entity();
    canvas(
        move |bounds, window, _| {
            let previous = geometry.replace(bounds);
            if previous.origin.x != bounds.origin.x || previous.size.width != bounds.size.width {
                let entity = entity.clone();
                window.on_next_frame(move |window, cx| {
                    let _ = entity.update(cx, |_, cx| cx.notify());
                    window.refresh();
                });
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

fn distinguishing_label(path: &Path, tabs: &[PathBuf]) -> String {
    for count in 1..=path.components().count() {
        let label = displayed_suffix(path, count);
        if !tabs
            .iter()
            .any(|other| other != path && displayed_suffix(other, count) == label)
        {
            return label.into_owned();
        }
    }
    path.to_string_lossy().into_owned()
}

fn displayed_suffix(path: &Path, count: usize) -> Cow<'_, str> {
    let mut components = path.components();
    let skip = components.clone().count().saturating_sub(count);
    for _ in 0..skip {
        components.next();
    }
    components.as_path().to_string_lossy()
}

impl TabPresentation {
    fn sync(&self, view: &EditorWorkspaceView) {
        let mut identities = self.identities.borrow_mut();
        let changed = identities.len() != view.open_tabs.len()
            || !identities
                .iter()
                .zip(&view.open_tabs)
                .all(|(identity, path)| identity.path == *path);
        if changed {
            *identities = view
                .open_tabs
                .iter()
                .map(|path| TabIdentity {
                    path: path.clone(),
                    label: distinguishing_label(path, &view.open_tabs).into(),
                    full_path: view
                        .repo_path
                        .join(path)
                        .to_string_lossy()
                        .into_owned()
                        .into(),
                })
                .collect();
        }
        let mut active = self.active.borrow_mut();
        let zoom_changed = self.zoom.replace(theme::zoom()) != theme::zoom();
        if changed || *active != view.open_path || zoom_changed {
            if let Some(index) = view
                .open_tabs
                .iter()
                .position(|path| Some(path) == view.open_path.as_ref())
            {
                view.tab_scroll.scroll_to_item(index);
            }
            active.clone_from(&view.open_path);
        }
    }
}

pub(crate) fn render_tab_strip(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> Option<gpui::AnyElement> {
    view.tab_presentation.sync(view);
    #[cfg(feature = "gui-e2e")]
    view.tab_probes.0.borrow_mut().clear();
    if view.open_tabs.is_empty() {
        return None;
    }
    let mut strip = div()
        .id("ews-tabs")
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .min_w(px(0.))
        .w_full()
        .overflow_x_scroll()
        .track_scroll(&view.tab_scroll)
        .bg(rgb(theme().panel));
    for (index, identity) in view.tab_presentation.identities.borrow().iter().enumerate() {
        strip = strip.child(render_tab(view, index, identity, cx));
    }
    let viewport = view.tab_presentation.viewport.clone();
    let handle = view.tab_scroll.clone();
    let selected = view
        .open_tabs
        .iter()
        .position(|path| Some(path) == view.open_path.as_ref());
    // Resizing a pane/window re-reveals its selected tab, but ordinary redraws
    // never undo a manual scroll to inspect an inactive tab.
    let resize = canvas(
        move |bounds, window, _| {
            if viewport.replace(bounds.size) != bounds.size {
                if let Some(selected) = selected {
                    handle.scroll_to_item(selected);
                    window.on_next_frame(|window, _| window.refresh());
                }
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full();
    let persistent = gpui_component::Theme::global(cx).scrollbar_show != ScrollbarShow::Scrolling;
    Some(
        div()
            .id("ews-tabs-area")
            .relative()
            .flex_shrink_0()
            .min_w(px(0.))
            .w_full()
            .when(persistent, |area| area.pb(px(16.)))
            .child(strip)
            .child(resize)
            .child(
                Scrollbar::horizontal(&view.tab_scroll)
                    .when(persistent, |bar| bar.scrollbar_show(ScrollbarShow::Always)),
            )
            .into_any_element(),
    )
}

fn render_tab(
    view: &EditorWorkspaceView,
    index: usize,
    identity: &TabIdentity,
    cx: &mut Context<EditorWorkspaceView>,
) -> gpui::AnyElement {
    let active = view.open_path.as_ref() == Some(&identity.path);
    let dirty = view.tab_dirty(&identity.path);
    let activate_path = identity.path.clone();
    let tab = div()
        .id(("ews-tab", index))
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .gap_1()
        .px_2()
        .min_h(theme::scaled_px(24.))
        .min_w(theme::scaled_px(120.))
        .max_w(theme::scaled_px(180.))
        .cursor_default()
        .text_xs();
    apply_identity(tab, view, identity, active)
        .bg(rgb(if active {
            theme().bg_base
        } else {
            theme().panel
        }))
        .when(!active, |el| el.hover(|s| s.bg(rgb(theme().surface))))
        .on_click(cx.listener(move |view, _, _, cx| view.activate_tab(activate_path.clone(), cx)))
        .when(dirty, |el| {
            el.child(
                div()
                    .flex_shrink_0()
                    .text_color(rgb(theme().color_warning))
                    .child("\u{25cf}"),
            )
        })
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_color(rgb(if active {
                    theme().text_main
                } else {
                    theme().text_sub
                }))
                .child(identity.label.clone()),
        )
        .child(render_close(view, index, identity.path.clone(), cx))
        .when(cfg!(feature = "gui-e2e"), |el| {
            #[cfg(feature = "gui-e2e")]
            let el = el.child(crate::tab_probes::measure(&view.tab_probes, index));
            el
        })
        .into_any_element()
}

/// Keep probe evidence at the same boundary that installs actual AX/Tooltip data.
fn apply_identity(
    tab: gpui::Stateful<gpui::Div>,
    _view: &EditorWorkspaceView,
    identity: &TabIdentity,
    active: bool,
) -> gpui::Stateful<gpui::Div> {
    let role = Role::Tab;
    let name = identity.full_path.clone();
    let tooltip = identity.full_path.clone();
    #[cfg(feature = "gui-e2e")]
    _view.tab_probes.0.borrow_mut().push(crate::EditorTabProbe {
        path: identity.path.to_string_lossy().into_owned(),
        label: identity.label.to_string(),
        full_path: name.to_string(),
        tooltip: tooltip.to_string(),
        bounds: gpui::Bounds::default(),
        role,
        close_bounds: gpui::Bounds::default(),
    });
    tab.role(role)
        .aria_label(name.clone())
        .aria_description(name)
        .aria_selected(active)
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
}

fn render_close(
    _view: &EditorWorkspaceView,
    index: usize,
    path: PathBuf,
    cx: &mut Context<EditorWorkspaceView>,
) -> gpui::AnyElement {
    div()
        .id(("ews-tab-x", index))
        .relative()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .min_w(theme::scaled_px(24.))
        .min_h(theme::scaled_px(24.))
        .rounded_sm()
        .text_color(rgb(theme().text_muted))
        .hover(|s| {
            s.bg(rgb(theme().selected))
                .text_color(rgb(theme().text_main))
        })
        .on_click(cx.listener(move |view, _, _, cx| {
            cx.stop_propagation();
            view.request_close_tab(path.clone(), cx);
        }))
        .child("\u{00d7}")
        .when(cfg!(feature = "gui-e2e"), |el| {
            #[cfg(feature = "gui-e2e")]
            let el = el.child(crate::tab_probes::measure_close(&_view.tab_probes, index));
            el
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_labels_use_shortest_unique_suffix() {
        let paths: Vec<PathBuf> = [
            "src/a.rs",
            "tests/a.rs",
            "one/src/b.rs",
            "two/src/b.rs",
            "独自.rs",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect();
        let labels: Vec<_> = paths
            .iter()
            .map(|path| distinguishing_label(path, &paths))
            .collect();
        assert_eq!(
            labels,
            [
                "src/a.rs",
                "tests/a.rs",
                "one/src/b.rs",
                "two/src/b.rs",
                "独自.rs"
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn duplicate_lossy_basenames_use_displayed_suffixes() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let paths = [
            PathBuf::from(OsString::from_vec(b"x/\xff.rs".to_vec())),
            PathBuf::from(OsString::from_vec(b"y/\xfe.rs".to_vec())),
        ];
        assert_eq!(distinguishing_label(&paths[0], &paths), "x/\u{fffd}.rs");
        assert_eq!(distinguishing_label(&paths[1], &paths), "y/\u{fffd}.rs");
    }
}
