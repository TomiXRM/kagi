//! Immutable historical content; the live InputState is never involved.
use crate::{EditorWorkspaceView, RightPaneTab};
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, Role, SharedString};
use gpui_component::{
    button::{Button, ButtonVariants},
    text::TextView,
    Sizable,
};
use kagi_ui_core::{
    i18n::Msg,
    theme::{self, theme},
};

// Forward stateful AX methods to the pinned Button's own interaction owner
// (same boundary as core::header_fit's NamedHeaderButton).
#[derive(gpui::IntoElement)]
struct SnapshotButton(Button);
impl gpui::InteractiveElement for SnapshotButton {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.0.interactivity()
    }
}
impl gpui::StatefulInteractiveElement for SnapshotButton {}
impl gpui::RenderOnce for SnapshotButton {
    fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl gpui::IntoElement {
        self.0.render(window, cx)
    }
}

/// Fence longer than every literal backtick run, retaining all source bytes.
/// Prepared once on publication, never reparsed/encoded on ordinary redraws.
pub(crate) fn literal_source(text: &str, language: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for byte in text.bytes() {
        run = if byte == b'`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence_len = (longest + 1).max(3);
    let mut source = String::with_capacity(text.len() + language.len() + 2 * fence_len + 2);
    source.extend(std::iter::repeat_n('`', fence_len));
    source.push_str(language);
    source.push('\n');
    source.push_str(text);
    source.push('\n');
    source.extend(std::iter::repeat_n('`', fence_len));
    source
}

#[cfg(feature = "gui-e2e")]
#[derive(Clone)]
pub struct SnapshotControl {
    pub role: Role,
    pub name: SharedString,
    pub bounds: gpui::Bounds<gpui::Pixels>,
}

fn identity(view: &EditorWorkspaceView) -> SharedString {
    let hash = view.selected_history_commit.as_deref().unwrap_or_default();
    let path = view
        .history
        .as_ref()
        .and_then(|history| history.entry_by_hash(hash))
        .map(|entry| entry.change.path_after.as_path())
        .or(view.open_path.as_deref())
        .unwrap_or(std::path::Path::new(""));
    format!("{} · {}", path.display(), hash).into()
}

fn indicator(identity: &SharedString, cx: &mut Context<EditorWorkspaceView>) -> AnyElement {
    let name: SharedString = format!("{} · {}", Msg::EditorSnapshotReadOnly.t(), identity).into();
    let tooltip = name.clone();
    let label = div()
        .id("ews-snapshot-readonly")
        .role(Role::Label)
        .aria_label(name.clone())
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
        })
        .min_w(px(0.))
        .truncate()
        .text_xs()
        .text_color(rgb(theme().text_sub))
        .child(Msg::EditorSnapshotReadOnly.t());
    #[cfg(feature = "gui-e2e")]
    let label = {
        let entity = cx.entity().downgrade();
        label.relative().child(
            gpui::canvas(
                move |bounds, _, cx| {
                    _ = entity.update(cx, |view, _| {
                        view.snapshot_indicator = Some(SnapshotControl {
                            role: Role::Label,
                            name: name.clone(),
                            bounds,
                        })
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    };
    #[cfg(not(feature = "gui-e2e"))]
    let _ = cx;
    label.into_any_element()
}

fn open_current(cx: &mut Context<EditorWorkspaceView>) -> AnyElement {
    let name: SharedString = Msg::EditorOpenCurrent.t().into();
    let button = Button::new("ews-snapshot-open-current")
        .ghost()
        .small()
        .label(name.clone())
        .on_click(cx.listener(|view, _, window, cx| {
            view.selected_history_commit = None;
            view.set_right_tab(RightPaneTab::Diff, window, cx);
            if let Some(editor) = &view.editor {
                use gpui::Focusable;
                window.focus(&editor.read(cx).focus_handle(cx), cx);
            }
            cx.notify();
        }));
    let button = SnapshotButton(button)
        .role(Role::Button)
        .aria_label(name.clone());
    let button = div().flex_shrink_0().relative().child(button);
    #[cfg(feature = "gui-e2e")]
    let button = {
        let entity = cx.entity().downgrade();
        button.child(
            gpui::canvas(
                move |bounds, _, cx| {
                    _ = entity.update(cx, |view, _| {
                        view.snapshot_current = Some(SnapshotControl {
                            role: Role::Button,
                            name: name.clone(),
                            bounds,
                        })
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    };
    button.into_any_element()
}

pub(crate) fn render(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    let identity = identity(view);
    div()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .flex_col()
                .flex_shrink_0()
                .px_2()
                .py_1()
                .bg(rgb(theme().panel))
                .child(indicator(&identity, cx))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .text_xs()
                                .text_color(rgb(theme().text_muted))
                                .child(identity),
                        )
                        .child(open_current(cx)),
                ),
        )
        .child(super::panes::render_snapshot_pane(view, cx))
        .into_any_element()
}

pub(crate) fn text(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    use gpui_component::ActiveTheme as _;
    let Some(state) = &view.snapshot_text else {
        return crate::placeholder_text(Msg::EditorWorkspaceLoading.t()).into_any_element();
    };
    let base = px(theme::rem_size_px() * 0.875);
    let style = gpui_component::text::TextViewStyle {
        heading_base_font_size: base,
        code_block: gpui::StyleRefinement::default().text_size(base),
        is_dark: theme().dark,
        highlight_theme: cx.theme().highlight_theme.clone(),
        ..Default::default()
    };
    div()
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .flex()
        .flex_col()
        .font_family(theme::MONO_FONT)
        .child(
            TextView::new(state)
                .selectable(true)
                .scrollable(true)
                .style(style)
                .h_full(),
        )
        .into_any_element()
}
