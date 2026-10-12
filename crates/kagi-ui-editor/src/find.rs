//! Locale adapter for the component-owned Find/Replace bar; no second matcher.
use gpui_component::input::SearchPanelLabels;
use kagi_ui_core::i18n::Msg;

pub(super) fn labels() -> SearchPanelLabels {
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
