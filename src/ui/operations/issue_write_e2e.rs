//! Test-only Issue terminal injection. Admission, settlement, and delivery stay real.

use super::*;

impl KagiApp {
    pub fn start_failed_issue_create_for_e2e(
        &mut self,
        repo_path: PathBuf,
        cx: &mut Context<Self>,
    ) -> bool {
        const ERROR: &str = "injected: the server refused the issue";
        let plan = Arc::new(kagi_git::github::plan_issue_create(
            "example/fixture",
            "fixture issue",
            "fixture body",
        ));
        let report_plan = plan.clone();
        let report_repo = repo_path.clone();
        self.finish_run(
            cx,
            "issue-create",
            crate::ui::i18n::Op::IssueCreate,
            plan,
            repo_path,
            move || {
                let result = Err(kagi_git::GitError::Other(ERROR.into()));
                let entry = kagi_git::oplog::OpLogEntry::new(
                    "issue-create",
                    report_repo.display().to_string(),
                    report_plan.current.clone(),
                    kagi_git::oplog::OpOutcome::Failed {
                        error: ERROR.into(),
                    },
                )
                .with_worktree(Some(report_repo.display().to_string()));
                Ok(RunReport {
                    result,
                    recording: kagi_git::backend::recording::finalize(entry),
                    stash: None,
                })
            },
            |_| None,
            |_| RunPresentation::none(),
        )
    }
}
