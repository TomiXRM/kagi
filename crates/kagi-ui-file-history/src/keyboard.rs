//! List focus survives visits to the History header and its read actions.
use crate::FileHistoryView;
use gpui::{actions, prelude::*, App, Context, Div, FocusHandle, KeyBinding, Stateful, Window};
use kagi_ui_core::keyboard_nav::{RowFocus, RowKeys, RowList, RowScroll};
use std::{cell::RefCell, rc::Rc};

actions!(kagi_file_history, [ReturnToHistoryRow]);
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(
            "shift-tab",
            ReturnToHistoryRow,
            Some("KagiHistoryFirstHeader"),
        ),
        KeyBinding::new("tab", ReturnToHistoryRow, Some("KagiHistoryLastHeader")),
    ]);
}
#[derive(Default)]
pub(crate) struct HistoryKeyboard {
    keys: RowKeys,
    focus: RefCell<RowFocus>,
    pub rows: RefCell<Option<Rc<RowList>>>,
    pub headers: RefCell<Vec<FocusHandle>>,
}
impl HistoryKeyboard {
    pub fn seed(&mut self, history: &kagi_domain::file_history::FileHistory) {
        self.keys = history
            .entries
            .iter()
            .enumerate()
            .map(|(ix, entry)| {
                (
                    entry
                        .commit
                        .as_ref()
                        .map(|c| c.full_hash.clone())
                        .unwrap_or_else(|| "WIP".into()),
                    ix,
                )
            })
            .collect();
    }
    pub fn prepare(&self, visible: bool, scroll: RowScroll, window: &mut Window, cx: &mut App) {
        let mut headers = self.headers.borrow_mut();
        while headers.len() < 5 {
            headers.push(cx.focus_handle());
        }
        if !visible {
            self.focus
                .borrow_mut()
                .yield_focus(headers.first(), window, cx);
            self.rows.borrow_mut().take();
            return;
        }
        let rows =
            self.focus
                .borrow_mut()
                .rows(self.keys.clone(), &scroll, headers.first(), window, cx);
        *self.rows.borrow_mut() = Some(Rc::new(rows));
    }
    pub fn return_to_row(&self, scroll: &RowScroll, window: &mut Window, cx: &mut App) -> bool {
        if self
            .rows
            .borrow()
            .as_ref()
            .is_none_or(|rows| rows.is_empty())
        {
            return false;
        }
        self.focus.borrow().focus_remembered(scroll, window, cx);
        true
    }
    #[cfg(feature = "gui-e2e")]
    pub fn header_focused(&self, slot: usize, window: &Window, cx: &App) -> bool {
        self.headers
            .borrow()
            .get(slot)
            .is_some_and(|h| h.contains_focused(window, cx))
    }
}
pub(crate) fn header_boundary(
    slot: usize,
    content: impl IntoElement,
    view: &FileHistoryView,
    cx: &mut Context<FileHistoryView>,
) -> Stateful<Div> {
    let context = if slot == 0 {
        "KagiHistoryFirstHeader"
    } else if slot == 4 {
        "KagiHistoryLastHeader"
    } else {
        "KagiHistoryHeader"
    };
    gpui::div()
        .id(("history-header-key", slot))
        .track_focus(&view.keyboard.headers.borrow()[slot])
        .key_context(context)
        .on_action(cx.listener(|view, _: &ReturnToHistoryRow, window, cx| {
            if !view.keyboard.return_to_row(
                &RowScroll::Uniform(view.list_scroll.clone(), view.last_built_range.clone()),
                window,
                cx,
            ) {
                cx.propagate();
            }
            cx.notify();
        }))
        .child(content)
}

pub(crate) fn row_name(
    entry: &kagi_domain::file_history::FileHistoryEntry,
    model: &kagi_ui_core::commit_row::CommitRowModel,
) -> gpui::SharedString {
    use kagi_domain::{
        file_history::FileHistoryEntryKind, text_safety::append_sanitized_control_bytes,
    };
    use kagi_ui_core::i18n::Msg;
    let path = entry.change.path_after.to_string_lossy();
    let status = crate::change_type_label(entry.change.change_type);
    let (template, args): (&str, &[&str]) = if entry.kind == FileHistoryEntryKind::Wip {
        (Msg::A11yWipRow.t(), &[path.as_ref(), status])
    } else {
        (
            Msg::A11yCommitRow.t(),
            &[
                &model.subject,
                &model.author,
                &model.date_relative,
                &model.short_hash,
            ],
        )
    };
    let mut label = String::with_capacity(
        template.len()
            + args.iter().map(|a| a.len()).sum::<usize>()
            + path.len()
            + status.len()
            + 4,
    );
    let mut parts = template.split("{}");
    label.push_str(parts.next().unwrap_or_default());
    for (arg, part) in args.iter().zip(parts) {
        append_sanitized_control_bytes(&mut label, arg);
        label.push_str(part);
    }
    if entry.kind != FileHistoryEntryKind::Wip {
        label.push_str(" · ");
        label.push_str(status);
        label.push(' ');
        append_sanitized_control_bytes(&mut label, &path);
    }
    label.into()
}
