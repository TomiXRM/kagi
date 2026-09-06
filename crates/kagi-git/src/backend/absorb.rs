//! Dedicated absorb boundary: trust → preflight → savepoint → execute → verify → record.
use super::*;

impl Backend {
    /// Execute a confirmed distribution plan. Safety gates are mandatory and
    /// owned here; callers must not separately opt in to `verify_absorb`.
    pub fn execute_absorb(
        &self,
        plan: &kagi_domain::absorb::AbsorbPlan,
    ) -> Result<kagi_domain::absorb::AbsorbOutcome, GitError> {
        self.absorb_recorded(plan).0
    }

    fn absorb_recorded(
        &self,
        plan: &kagi_domain::absorb::AbsorbPlan,
    ) -> (
        Result<kagi_domain::absorb::AbsorbOutcome, GitError>,
        recording::Recording,
    ) {
        let mut executed = false;
        let mut after = plan.current.clone();
        let result: Result<_, GitError> = (|| {
            self.require_trust()?;
            ops::preflight_absorb(&self.repo, plan)?;
            self.auto_savepoint("absorb");
            let outcome = ops::execute_absorb(&self.repo, plan)?;
            executed = true;
            #[cfg(test)]
            let outcome = verification_fixture(outcome, plan);
            ops::verify_absorb(&self.repo, &outcome)?;
            after = self.current_state()?;
            Ok(outcome)
        })();
        let outcome = match &result {
            Ok(_) => crate::oplog::OpOutcome::Success {
                after: after.clone(),
            },
            Err(error) if executed => crate::oplog::OpOutcome::Partial {
                after: self.current_state().unwrap_or_else(|_| ops::StateSummary {
                    head: "unknown after absorb".into(),
                    dirty: "could not read state after mutation".into(),
                }),
                error: error.to_string(),
            },
            Err(error) => crate::oplog::OpOutcome::Failed {
                error: error.to_string(),
            },
        };
        let recording = self.record_run_oplog("absorb", &plan.current, outcome);
        (result, recording)
    }
}

// Compiled only into this crate's unit-test binary; no production fault API,
// environment flag or policy switch can select this state.
#[cfg(test)]
thread_local! {
    static FAULT: std::cell::Cell<Option<AbsorbFault>> = const { std::cell::Cell::new(None) };
}
#[cfg(test)]
#[derive(Clone, Copy)]
enum AbsorbFault {
    WrongReportedTip,
}
#[cfg(test)]
fn verification_fixture(
    mut outcome: kagi_domain::absorb::AbsorbOutcome,
    plan: &kagi_domain::absorb::AbsorbPlan,
) -> kagi_domain::absorb::AbsorbOutcome {
    if matches!(
        FAULT.with(|fault| fault.take()),
        Some(AbsorbFault::WrongReportedTip)
    ) {
        outcome.new_head = plan.head_at_plan.clone();
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absorb_verification_failure_is_not_success_and_is_recorded_as_partial() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(tmp.path()).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();
        repo.set_head("refs/heads/topic").unwrap();
        std::fs::write(tmp.path().join("a.txt"), "alpha\nbeta\ngamma\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let signature = repo.signature().unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "base", &tree, &[])
            .unwrap();
        std::fs::write(tmp.path().join("a.txt"), "alpha\nBETA\ngamma\n").unwrap();
        let backend = Backend::open_with_policy(tmp.path(), ExecutionPolicy::human(false)).unwrap();
        let plan = backend.plan_absorb(10).unwrap();
        assert!(!plan.is_noop());
        FAULT.with(|fault| fault.set(Some(AbsorbFault::WrongReportedTip)));
        let (result, recording) = backend.absorb_recorded(&plan);
        let error = result.unwrap_err();
        assert!(error.to_string().contains("does not match rebuilt commit"));
        let actual = backend.head_commit_id().unwrap().0;
        assert_ne!(actual, plan.head_at_plan, "the mutation really happened");
        // Inspect this invocation's receipt, not a process-global log location:
        // other unit tests may change KAGI_LOG_DIR while running in parallel.
        assert!(matches!(
            &recording.entry().outcome,
            crate::oplog::OpOutcome::Partial { after, .. }
                if after.head == backend.current_state().unwrap().head
        ));
    }
}
