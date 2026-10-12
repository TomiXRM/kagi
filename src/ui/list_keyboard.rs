//! Retained row focus for small choosers and the session-owned Issue feed.
use super::keyboard_nav::{RowFocus, RowKeys, RowList, RowScroll};
use gpui::{App, FocusHandle, ListState, Window};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
pub struct ListKeyboard {
    keys: RefCell<RowKeys>,
    focus: RefCell<RowFocus>,
}

impl std::fmt::Debug for ListKeyboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListKeyboard").finish_non_exhaustive()
    }
}

impl ListKeyboard {
    pub(super) fn rows<'a>(
        &self,
        keys: impl ExactSizeIterator<Item = (&'a str, usize)> + Clone,
        scroll: &ListState,
        fallback: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) -> Rc<RowList> {
        let mut retained = self.keys.borrow_mut();
        let unchanged = retained.len() == keys.len()
            && retained
                .iter()
                .zip(keys.clone())
                .all(|((key, index), (other, ix))| key == other && *index == ix);
        if !unchanged {
            *retained = keys.map(|(key, index)| (key.to_owned(), index)).collect();
        }
        self.focus
            .borrow_mut()
            .rows(
                retained.clone(),
                &RowScroll::List(scroll.clone()),
                fallback,
                window,
                cx,
            )
            .into()
    }
    pub(super) fn focus_first(
        &self,
        scroll: &ListState,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        self.focus
            .borrow()
            .focus_first(&RowScroll::List(scroll.clone()), window, cx)
    }
    pub(super) fn yield_focus(
        &self,
        fallback: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.focus.borrow_mut().yield_focus(fallback, window, cx);
    }

    #[cfg(feature = "gui-e2e")]
    pub(super) fn focused(&self, window: &Window) -> Option<String> {
        self.focus.borrow().focused(window).map(str::to_owned)
    }
}

#[cfg(feature = "gui-e2e")]
impl super::KagiApp {
    pub fn list_row_focused_for_e2e(&self, list: &str, window: &Window) -> Option<String> {
        match list {
            "issue-keyboard-list" => self.ui().issue_rows.focused(window),
            "inspector-files-scroll" => self.ui().inspector_model.keyboard.focused(window),
            "home-recent-list" => self.home_github.recent.keyboard.focused(window),
            "branch-picker-list" => match &self.menu_overlay {
                Some(super::commands::MenuOverlay::BranchPicker { rows, .. }) => {
                    rows.keyboard.focused(window)
                }
                _ => None,
            },
            _ => None,
        }
    }
}
