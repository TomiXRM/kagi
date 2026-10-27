//! Accepted Analyze baseline is independent from the retained InputState draft.
use super::{AnalyzeRequest, AnalyzeResult};
use gpui::SharedString;

pub(crate) struct AnalyzeState {
    pub baseline: SharedString,
    pub accepted_revision: u64,
    pub error: Option<SharedString>,
    pub saved_feedback: bool,
}
impl AnalyzeState {
    pub fn new(baseline: SharedString) -> Self { Self { baseline, accepted_revision: 0, error: None, saved_feedback: false } }
    /// Only a successful reset of the still-current captured draft may replace
    /// input text. Save acknowledgements never write the editor.
    pub fn acknowledge(&mut self, result: AnalyzeResult, current: &AnalyzeRequest) -> Option<SharedString> {
        match result {
            AnalyzeResult::Saved(request) if request.revision >= self.accepted_revision => {
                self.accepted_revision = request.revision;
                self.baseline = request.text;
                self.error = None;
                self.saved_feedback = true;
            }
            AnalyzeResult::Reset { request, text } if request.revision >= self.accepted_revision => {
                self.accepted_revision = request.revision;
                self.baseline = text.clone();
                self.error = None;
                self.saved_feedback = true;
                if current == &request { return Some(text); }
            }
            AnalyzeResult::Failed { request, error } if request.revision >= self.accepted_revision => { self.error = Some(error); }
            _ => {}
        }
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request(revision: u64, text: &str) -> AnalyzeRequest { AnalyzeRequest { revision, text: text.to_owned().into() } }
    #[test]
    fn delayed_save_preserves_newer_draft_and_advances_only_captured_baseline() {
        let mut state = AnalyzeState::new("original".into());
        let current = request(2,"newer draft");
        assert!(state.acknowledge(AnalyzeResult::Saved(request(1,"saved draft")),&current).is_none());
        assert_eq!(state.baseline.as_ref(),"saved draft");
        assert_ne!(state.baseline,current.text);
    }
    #[test]
    fn failure_retains_baseline_and_reset_does_not_overwrite_newer_edits() {
        let mut state = AnalyzeState::new("original".into());
        let current = request(2,"new draft");
        state.acknowledge(AnalyzeResult::Failed { request: request(1,"old draft"), error: "permission denied".into() },&current);
        assert_eq!(state.baseline.as_ref(),"original");
        assert_eq!(state.error.as_deref(),Some("permission denied"));
        assert!(state.acknowledge(AnalyzeResult::Reset { request: request(1,"old draft"), text: "defaults".into() },&current).is_none());
        assert_eq!(state.baseline.as_ref(),"defaults");
        assert!(state.error.is_none());
    }
    #[test]
    fn successful_current_reset_replaces_draft_and_older_results_are_ignored() {
        let mut state = AnalyzeState::new("original".into());
        let current = request(3,"draft");
        assert_eq!(state.acknowledge(AnalyzeResult::Reset { request: current.clone(),text:"defaults".into() },&current).as_deref(),Some("defaults"));
        state.acknowledge(AnalyzeResult::Saved(request(2,"stale")),&current);
        state.acknowledge(AnalyzeResult::Failed { request: request(1,"stale"),error:"stale error".into() },&current);
        assert_eq!(state.baseline.as_ref(),"defaults");
        assert!(state.error.is_none());
    }
}
