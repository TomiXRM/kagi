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

/// Bound LF blocks; preserve CRLF verbatim in one block; large text skips syntax.
/// Fence longer than every literal backtick run, retaining source separators.
pub(crate) fn literal_source(text: &str, language: &str) -> String {
    const CHUNK_LINES: usize = 32;
    const HIGHLIGHT_BYTES: usize = 128 * 1024;
    const HIGHLIGHT_LINES: usize = 2048;
    let line_count = text.lines().count();
    let language = if text.len() <= HIGHLIGHT_BYTES && line_count <= HIGHLIGHT_LINES {
        language
    } else {
        ""
    };
    let blocks = line_count.max(1).div_ceil(CHUNK_LINES);
    let mut source = String::with_capacity(text.len() + blocks * (language.len() + 11));
    // Markdown consumes the CR before each closing fence. Splitting a CRLF
    // line boundary would therefore silently replace that separator with LF.
    if text.contains("\r\n") {
        append_block(&mut source, text, language);
        return source;
    }
    let mut start = 0;
    let mut lines = 0;
    for (offset, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            lines += 1;
            if lines == CHUNK_LINES {
                append_block(&mut source, &text[start..=offset], language);
                start = offset + 1;
                lines = 0;
            }
        }
    }
    if start < text.len() || source.is_empty() {
        append_block(&mut source, &text[start..], language);
    }
    source
}

fn append_block(source: &mut String, text: &str, language: &str) {
    // A long delimiter in one chunk must not amplify every other chunk.
    let mut longest = 0;
    let mut run = 0;
    for byte in text.bytes() {
        run = if byte == b'`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence_len = (longest + 1).max(3);
    source.extend(std::iter::repeat_n('`', fence_len));
    source.push_str(language);
    source.push('\n');
    source.push_str(text.strip_suffix('\n').unwrap_or(text));
    source.push('\n');
    source.extend(std::iter::repeat_n('`', fence_len));
    source.push_str("\n\n");
}

#[cfg(feature = "gui-e2e")]
#[derive(Clone)]
pub struct SnapshotControl {
    pub role: Role,
    pub name: SharedString,
    pub bounds: gpui::Bounds<gpui::Pixels>,
    pub tooltip: Option<SharedString>,
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
    let role = Role::Label;
    #[cfg(feature = "gui-e2e")]
    let recorded_tooltip = tooltip.clone();
    #[cfg(feature = "gui-e2e")]
    let tooltip_owner = cx.entity().downgrade();
    let label = div()
        .id("ews-snapshot-readonly")
        .role(role)
        .aria_label(name.clone())
        .tooltip(move |window, cx| {
            #[cfg(feature = "gui-e2e")]
            let _ = tooltip_owner.update(cx, |view, _| {
                view.snapshot_tooltip_shown = Some(tooltip.clone());
            });
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
                            role,
                            name: name.clone(),
                            bounds,
                            tooltip: Some(recorded_tooltip.clone()),
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
    let role = Role::Button;
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
    let button = SnapshotButton(button).role(role).aria_label(name.clone());
    let button = div().flex_shrink_0().relative().child(button);
    #[cfg(feature = "gui-e2e")]
    let button = {
        let entity = cx.entity().downgrade();
        button.child(
            gpui::canvas(
                move |bounds, _, cx| {
                    _ = entity.update(cx, |view, _| {
                        view.snapshot_current = Some(SnapshotControl {
                            role,
                            name: name.clone(),
                            bounds,
                            tooltip: None,
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
        paragraph_gap: gpui::rems(0.),
        code_block: gpui::StyleRefinement::default()
            .text_size(base)
            .p_0()
            .px_3()
            .rounded_none(),
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
