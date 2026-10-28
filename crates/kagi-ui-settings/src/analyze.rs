//! Analyze pane and accepted baseline, independent from the retained InputState draft.
use super::{
    kit_style::{ink, metrics, pt},
    *,
};
use kagi_ui_core::theme::theme;
use kagi_ui_core::tooltip::HoverTooltip as _;

pub(crate) struct AnalyzeState {
    pub baseline: SharedString,
    pub accepted_revision: u64,
    pub error: Option<SharedString>,
    pub saved_feedback: bool,
}
impl AnalyzeState {
    pub fn new(baseline: SharedString) -> Self {
        Self {
            baseline,
            accepted_revision: 0,
            error: None,
            saved_feedback: false,
        }
    }
    /// Only a successful reset of the still-current captured draft may replace
    /// input text. Save acknowledgements never write the editor.
    pub fn acknowledge(
        &mut self,
        result: AnalyzeResult,
        current: &AnalyzeRequest,
    ) -> Option<SharedString> {
        match result {
            AnalyzeResult::Saved(request) if request.revision >= self.accepted_revision => {
                self.accepted_revision = request.revision;
                self.baseline = request.text;
                self.error = None;
                self.saved_feedback = true;
            }
            AnalyzeResult::Reset { request, text }
                if request.revision >= self.accepted_revision =>
            {
                self.accepted_revision = request.revision;
                self.baseline = text.clone();
                self.error = None;
                self.saved_feedback = true;
                if current == &request {
                    return Some(text);
                }
            }
            AnalyzeResult::Failed { request, error }
                if request.revision >= self.accepted_revision =>
            {
                self.error = Some(error);
            }
            _ => {}
        }
        None
    }
}
impl SettingsView {
    pub(crate) fn analyze(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let dirty = self.analyze_dirty(cx);
        let request = self.analyze_request(cx);
        let reset = self.button(
            "settings-analyze-reset",
            format!("{}…", Msg::SettingsAnalyzeIgnoreReset.t()).into(),
            true,
            ink::card(),
            SettingsIntent::ResetAnalyze(request.clone()),
            window,
            cx,
        );
        let save = self.button(
            "settings-analyze-save",
            Msg::SettingsAnalyzeIgnoreSave.t().into(),
            dirty,
            ink::card(),
            SettingsIntent::SaveAnalyze(request),
            window,
            cx,
        );
        let status = if dirty {
            Msg::SettingsAnalyzeIgnoreUnsaved
        } else if self.analyze.saved_feedback {
            Msg::SettingsAnalyzeIgnoreSaved
        } else {
            Msg::SettingsAnalyzeIgnoreClean
        };
        let help = div()
            .id("settings-analyze-help")
            .relative()
            .min_w_0()
            .flex()
            .flex_col()
            .role(Role::Label)
            .aria_label(Msg::SettingsAnalyzeIgnoreDesc.t())
            .aria_description(Msg::SettingsAnalyzeIgnoreDesc.t())
            .child(Msg::SettingsAnalyzeIgnoreDesc.t())
            .children(cfg!(feature = "gui-e2e").then(|| {
                self.measured(
                    "settings-analyze-help".into(),
                    Role::Label,
                    Msg::SettingsAnalyzeIgnore.t().into(),
                    Msg::SettingsAnalyzeIgnoreDesc.t().into(),
                    false,
                    false,
                    false,
                )
            }))
            .hover_tooltip(
                "settings-analyze-help-full",
                Msg::SettingsAnalyzeIgnoreDesc.t(),
            );
        let input = div()
            .id("settings-analyze-input")
            .relative()
            .min_w_0()
            .min_h(pt(220.))
            .child(self.multiline_input(window, cx))
            .children(cfg!(feature = "gui-e2e").then(|| {
                self.measured(
                    "settings-analyze-input".into(),
                    Role::TextInput,
                    Msg::A11yAnalyzeIgnorePatterns.t().into(),
                    self.input.read(cx).value().to_string(),
                    false,
                    false,
                    false,
                )
            }));
        let status_text = StyledText::new(status.t());
        #[cfg(feature = "gui-e2e")]
        let status_text_probe = Some(self.measured_text(
            "settings-analyze-status-text".into(),
            status.t().into(),
            &status_text,
        ));
        #[cfg(not(feature = "gui-e2e"))]
        let status_text_probe = None::<AnyElement>;
        let status = div()
            .id("settings-analyze-status")
            .relative()
            .min_w_0()
            .max_w_full()
            .flex()
            .flex_col()
            .role(Role::Status)
            .text_size(pt(metrics::SECONDARY))
            .text_color(ink::secondary())
            .child(status_text)
            .children(status_text_probe)
            .children(cfg!(feature = "gui-e2e").then(|| {
                self.measured(
                    "settings-analyze-status".into(),
                    Role::Status,
                    status.t().into(),
                    status.t().into(),
                    false,
                    false,
                    false,
                )
            }));
        let content = div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(pt(8.))
            .child(
                self.group("settings-analyze-group")
                    .child(div().min_w_0().w_full().py(pt(12.)).child(input))
                    .child(
                        div()
                            .min_w_0()
                            .w_full()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(pt(8.))
                            .py(pt(10.))
                            .child(status)
                            .child(
                                div()
                                    .min_w_0()
                                    .max_w_full()
                                    .ml_auto()
                                    .flex()
                                    .flex_wrap()
                                    .justify_end()
                                    .gap(pt(8.))
                                    .child(reset)
                                    .child(save),
                            ),
                    ),
            )
            .when_some(self.analyze.error.clone(), |content, error| {
                content.child(
                    div()
                        .id("settings-analyze-error")
                        .relative()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .role(Role::Status)
                        .text_color(rgb(theme().color_blocker))
                        .children(cfg!(feature = "gui-e2e").then(|| {
                            self.measured(
                                "settings-analyze-error".into(),
                                Role::Status,
                                error.to_string(),
                                error.to_string(),
                                false,
                                false,
                                false,
                            )
                        }))
                        .child(error),
                )
            });
        div()
            .id("settings-analyze-section")
            .relative()
            .min_w_0()
            .flex()
            .flex_col()
            .child(self.section(
                Msg::SettingsAnalyzeIgnore,
                Some(help.into_any_element()),
                content,
            ))
            .children(cfg!(feature = "gui-e2e").then(|| {
                self.measured(
                    "settings-analyze-section".into(),
                    Role::Group,
                    Msg::SettingsAnalyzeIgnore.t().into(),
                    String::new(),
                    false,
                    false,
                    false,
                )
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{AnalyzeRequest, AnalyzeResult, AnalyzeState};
    fn request(revision: u64, text: &str) -> AnalyzeRequest {
        AnalyzeRequest {
            revision,
            text: text.to_owned().into(),
        }
    }
    #[test]
    fn delayed_save_preserves_newer_draft_and_advances_only_captured_baseline() {
        let mut state = AnalyzeState::new("original".into());
        let current = request(2, "newer draft");
        assert!(state
            .acknowledge(AnalyzeResult::Saved(request(1, "saved draft")), &current)
            .is_none());
        assert_eq!(state.baseline.as_ref(), "saved draft");
        assert_ne!(state.baseline, current.text);
    }
    #[test]
    fn failure_retains_baseline_and_reset_does_not_overwrite_newer_edits() {
        let mut state = AnalyzeState::new("original".into());
        let current = request(2, "new draft");
        state.acknowledge(
            AnalyzeResult::Failed {
                request: request(1, "old draft"),
                error: "permission denied".into(),
            },
            &current,
        );
        assert_eq!(state.baseline.as_ref(), "original");
        assert_eq!(state.error.as_deref(), Some("permission denied"));
        assert!(state
            .acknowledge(
                AnalyzeResult::Reset {
                    request: request(1, "old draft"),
                    text: "defaults".into()
                },
                &current
            )
            .is_none());
        assert_eq!(state.baseline.as_ref(), "defaults");
        assert!(state.error.is_none());
    }
    #[test]
    fn successful_current_reset_replaces_draft_and_older_results_are_ignored() {
        let mut state = AnalyzeState::new("original".into());
        let current = request(3, "draft");
        assert_eq!(
            state
                .acknowledge(
                    AnalyzeResult::Reset {
                        request: current.clone(),
                        text: "defaults".into()
                    },
                    &current
                )
                .as_deref(),
            Some("defaults")
        );
        state.acknowledge(AnalyzeResult::Saved(request(2, "stale")), &current);
        state.acknowledge(
            AnalyzeResult::Failed {
                request: request(1, "stale"),
                error: "stale error".into(),
            },
            &current,
        );
        assert_eq!(state.baseline.as_ref(), "defaults");
        assert!(state.error.is_none());
    }
}
