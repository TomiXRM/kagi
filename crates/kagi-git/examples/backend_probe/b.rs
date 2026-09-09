//! P3 semantic-matrix probe for #627.
//!
//! The integration owner registers `SemanticMatrix` in the root dispatcher.

use kagi_git::benchmark::{HarnessError, ProbeContext, ProbeOperation};
use serde_json::{json, Value};

pub struct SemanticMatrix;

impl ProbeOperation for SemanticMatrix {
    fn name(&self) -> &'static str {
        "semantic-matrix"
    }

    fn mutates_fixture(&self) -> bool {
        true
    }

    fn requires_pristine_copy_per_iteration(&self) -> bool {
        true
    }

    fn execute(&self, context: &ProbeContext<'_>) -> Result<Value, HarnessError> {
        Ok(json!({
            "backend": context.backend(),
            "candidate": context.candidate(),
            "repo": context.repo(),
            "result": "scenario-dispatch-required"
        }))
    }
}
