//! Locale adapter for every component-owned Find/Replace bar; no second matcher.
use crate::i18n::Msg;
use gpui_component::input::SearchPanelLabels;

pub fn labels() -> SearchPanelLabels {
    SearchPanelLabels {
        previous: Msg::EditorFindPrevious.t().into(),
        next: Msg::EditorFindNext.t().into(),
        replace: Msg::EditorFindReplace.t().into(),
        replace_all: Msg::EditorFindReplaceAll.t().into(),
        close: Msg::EditorFindClose.t().into(),
        case_sensitive: Msg::EditorFindCase.t().into(),
        replace_mode: Msg::EditorFindReplaceMode.t().into(),
        search_field: Msg::EditorFindField.t().into(),
        replace_field: Msg::EditorFindReplacementField.t().into(),
    }
}
