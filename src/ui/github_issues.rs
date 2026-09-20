//! Owner-bound Issue composer reads and writes. No completion reads active repo.
use super::operations::RunPresentation;
use super::*;
use kagi_domain::issue_composer::IssueDraft;
use std::sync::Arc;

fn issue_write_title(ui: &TabUiState, number: Option<u64>, draft: &IssueDraft) -> String {
    let Some(number) = number else {
        return draft.effective_title();
    };
    ui.github_issue_details
        .get(&number)
        .or_else(|| ui.github_issues.iter().find(|issue| issue.number == number))
        .map(|issue| issue.title.clone())
        .unwrap_or_default()
}

impl KagiApp {
    /// Refresh the read-only Issue list for the session that starts the
    /// request. A later request for that session supersedes this completion;
    /// switching tabs never redirects it to the active owner.
    pub fn refresh_github_issues(&mut self, cx: &mut Context<Self>) {
        self.prepare_issue_composer(None, cx);
        let (Some(owner), Some(repo)) = (self.active_session(), self.repo_path.clone()) else {
            return;
        };
        self.refresh_github_issues_for(owner, repo, cx);
    }

    pub(super) fn refresh_github_issues_for(
        &mut self,
        owner: crate::app::SessionId,
        repo: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let (generation, frozen_base_repo) = {
            let Some(ui) = self.ui.get_mut(&owner) else {
                return;
            };
            let frozen_base_repo = ui.issue_composer.base_repo.clone();
            (ui.begin_github_issues_request(), frozen_base_repo)
        };
        cx.notify();
        cx.spawn(async move |this, acx| {
            let result =
                acx.background_executor()
                    .spawn(async move {
                        kagi_git::github::list_issues(&repo, frozen_base_repo.as_deref())
                    })
                    .await;
            let _ = this.update(acx, |app, cx| {
                let owner_is_active = app.active_session() == Some(owner);
                let Some(ui) = app.ui.get_mut(&owner) else {
                    return;
                };
                if ui.finish_github_issues_request(generation, result) && owner_is_active {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Load the selected Issue's body and comments through the same
    /// session-owned background boundary as the list.
    pub fn load_github_issue_detail(&mut self, number: u64, cx: &mut Context<Self>) {
        self.select_github_issue(number);
        self.prepare_issue_composer(Some(number), cx);
        let (Some(owner), Some(repo)) = (self.active_session(), self.repo_path.clone()) else {
            return;
        };
        self.load_github_issue_detail_for(owner, repo, number, cx);
    }

    /// Selecting another thread exits any Composer focus mode before the
    /// selected metadata and Reply destination change together.
    pub(super) fn select_github_issue(&mut self, number: u64) {
        self.with_ui(|ui| {
            ui.selected_github_issue = Some(number);
            for editor in ui.issue_composer.editors.values_mut() {
                editor.focused = false;
            }
        });
    }

    pub(super) fn select_github_issue_tab(
        &mut self,
        tab: kagi_domain::github::IssueListTab,
        cx: &mut Context<Self>,
    ) {
        self.with_ui(|ui| ui.github_issue_tab = tab);
        cx.notify();
    }

    pub(super) fn return_to_issues_home(&mut self, cx: &mut Context<Self>) {
        self.with_ui(TabUiState::clear_github_issue_selection);
        cx.notify();
    }

    pub(super) fn load_github_issue_detail_for(
        &mut self,
        owner: crate::app::SessionId,
        repo: PathBuf,
        number: u64,
        cx: &mut Context<Self>,
    ) {
        let (generation, selected) = {
            let Some(ui) = self.ui.get_mut(&owner) else {
                return;
            };
            let selected = ui.selected_github_issue == Some(number);
            let generation = if selected {
                ui.begin_github_issue_detail_request(number)
            } else {
                ui.github_issue_detail_gen
            };
            (generation, selected)
        };
        cx.notify();
        cx.spawn(async move |this, acx| {
            let result = acx
                .background_executor()
                .spawn(async move { kagi_git::github::issue_detail(&repo, number) })
                .await;
            let _ = this.update(acx, |app, cx| {
                let owner_is_active = app.active_session() == Some(owner);
                let Some(ui) = app.ui.get_mut(&owner) else {
                    return;
                };
                if !selected {
                    // Refresh a posted thread without changing the user's
                    // selected Issue or another thread's loading/error state.
                    if ui.github_issue_detail_gen == generation {
                        if let Ok(issue) = result {
                            ui.github_issue_details.insert(number, issue);
                        }
                    }
                    return;
                }
                if ui.finish_github_issue_detail_request(generation, number, result)
                    && owner_is_active
                {
                    cx.notify();
                }
            });
        })
        .detach();
    }
    pub(super) fn prepare_issue_composer(&mut self, number: Option<u64>, cx: &mut Context<Self>) {
        let (Some(owner), Some(repo)) = (self.active_session(), self.repo_path.clone()) else {
            return;
        };
        let draft_read = {
            let Some(ui) = self.ui.get_mut(&owner) else {
                return;
            };
            let editor = ui.issue_composer.editors.entry(number).or_default();
            if editor.loading || editor.loaded {
                None
            } else {
                editor.loading = true;
                editor.repo = Some(repo.clone());
                let storage_version = kagi_git::drafts::issue_draft_version(&repo, number);
                editor.storage_version = storage_version;
                Some((storage_version, editor.draft.revision))
            }
        };
        cx.notify();

        if let Some((storage_version, revision)) = draft_read {
            let draft_repo = repo.clone();
            cx.spawn(async move |this, acx| {
                let draft = acx
                    .background_executor()
                    .spawn(async move { kagi_git::drafts::load_issue_draft(&draft_repo, number) })
                    .await;
                let _ = this.update(acx, |app, cx| {
                    let Some(editor) = app
                        .ui
                        .get_mut(&owner)
                        .and_then(|ui| ui.issue_composer.editors.get_mut(&number))
                    else {
                        return;
                    };
                    editor.loading = false;
                    editor.loaded = true;
                    if editor.draft.revision == revision
                        && editor.storage_version == storage_version
                    {
                        if let Some((title, body)) = draft {
                            editor.draft.update(title, body);
                            editor.sync_inputs = true;
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    pub(super) fn save_issue_draft_for(
        &mut self,
        owner: crate::app::SessionId,
        repo: PathBuf,
        number: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self
            .ui
            .get_mut(&owner)
            .and_then(|ui| ui.issue_composer.editors.get_mut(&number))
        else {
            return;
        };
        let revision = editor.draft.revision;
        editor.saving = true;
        let storage_version = kagi_git::drafts::queue_issue_draft(
            &repo,
            number,
            &editor.draft.title,
            &editor.draft.body,
        );
        editor.storage_version = storage_version;
        self.schedule_issue_draft_flush(owner, repo, number, revision, storage_version, cx);
    }

    fn schedule_issue_draft_flush(
        &mut self,
        owner: crate::app::SessionId,
        repo: PathBuf,
        number: Option<u64>,
        revision: u64,
        storage_version: u64,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, acx| {
            acx.background_executor()
                .timer(std::time::Duration::from_millis(250))
                .await;
            let flush_repo = repo.clone();
            let result = acx
                .background_executor()
                .spawn(async move {
                    kagi_git::drafts::flush_issue_draft_if_version(
                        &flush_repo,
                        number,
                        storage_version,
                    )
                })
                .await;
            let _ = this.update(acx, |app, cx| {
                let error = result.err().map(|e| e.to_string());
                if let Some(editor) = app
                    .ui
                    .get_mut(&owner)
                    .and_then(|ui| ui.issue_composer.editors.get_mut(&number))
                {
                    if editor.draft.revision == revision
                        && editor.storage_version == storage_version
                    {
                        editor.saving = false;
                        editor.save_error = error.clone();
                    }
                }
                if let Some(error) = error {
                    app.report_unknown_notice(&repo, error);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn start_issue_write(&mut self, number: Option<u64>, cx: &mut Context<Self>) {
        let (Some(owner), Some(repo)) = (self.active_session(), self.repo_path.clone()) else {
            return;
        };
        let Some(ui) = self.ui.get(&owner) else {
            return;
        };
        let Some(base_repo) = ui.issue_composer.base_repo.clone() else {
            return;
        };
        let Some(editor) = ui.issue_composer.editors.get(&number) else {
            return;
        };
        let draft = editor.draft.clone();
        let storage_version = editor.storage_version;
        let title = issue_write_title(ui, number, &draft);
        let op_name = if number.is_some() {
            "issue-comment"
        } else {
            "issue-create"
        };
        let op = if number.is_some() {
            i18n::Op::IssueComment
        } else {
            i18n::Op::IssueCreate
        };
        if self.reject_transport_hold(&repo, op_name) {
            self.present_app_notice();
            cx.notify();
            return;
        }
        if self.reject_if_busy(cx) {
            return;
        }
        let plan = Arc::new(match number {
            Some(number) => kagi_git::github::plan_issue_comment(number, &title, &draft.body),
            None => kagi_git::github::plan_issue_create(&base_repo, &title, &draft.body),
        });
        if !plan.blockers.is_empty() {
            klog!("refused: {} plan has blockers, not executing", op_name);
            self.record_op(
                op_name,
                plan.current.clone(),
                OpOutcome::Refused {
                    blockers: plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo,
                cx,
            );
            self.report_plan_failure(
                op,
                plan.blockers
                    .iter()
                    .map(i18n::plan::plan_note_text)
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            return;
        }
        let rp = repo.clone();
        let bg_plan = plan.clone();
        self.finish_run(
            cx,
            op_name,
            op,
            plan,
            repo,
            move || {
                Ok(match number {
                    Some(number) => kagi_git::github::issue_comment(
                        &rp,
                        &base_repo,
                        number,
                        &draft.body,
                        &bg_plan,
                    ),
                    None => kagi_git::github::issue_create(
                        &rp,
                        &base_repo,
                        &title,
                        &draft.body,
                        &bg_plan,
                    ),
                })
            },
            |_| None,
            move |done| match done {
                Ok(
                    kagi_git::OperationOutcome::IssueCreate { .. }
                    | kagi_git::OperationOutcome::IssueComment { .. },
                ) => {
                    klog!("executed: {}", op_name);
                    RunPresentation::none().issue_write(number, storage_version)
                }
                _ => RunPresentation::none(),
            },
        );
    }

    /// Settlement precedes the presentation guard, but only consumes the sent
    /// revision. New text typed while a post was in flight is a different draft.
    pub(crate) fn settle_issue_write(
        &mut self,
        owner: crate::app::SessionId,
        repo: PathBuf,
        number: Option<u64>,
        version: u64,
        cx: &mut Context<Self>,
    ) {
        if kagi_git::drafts::clear_issue_draft_if_version(&repo, number, version) {
            let next = kagi_git::drafts::issue_draft_version(&repo, number);
            let mut owner_revision = None;
            // A closed/reopened session can be displaying the same saved draft.
            // Consume only this exact storage version, never a new edit, and
            // never derive the storage address from the active tab.
            for (session, ui) in &mut self.ui {
                if let Some(editor) = ui.issue_composer.editors.get_mut(&number) {
                    if editor.repo.as_ref() == Some(&repo) && editor.storage_version == version {
                        editor.draft.clear_if_revision(editor.draft.revision);
                        editor.storage_version = next;
                        editor.sync_inputs = true;
                        if number.is_none() {
                            editor.body_revealed = false;
                        }
                        if *session == owner {
                            owner_revision = Some(editor.draft.revision);
                        }
                    }
                }
            }
            self.schedule_issue_draft_flush(
                owner,
                repo.clone(),
                number,
                owner_revision.unwrap_or(0),
                next,
                cx,
            );
        }
        let owner_kept_issues_open = self.ui.get(&owner).is_some_and(|ui| {
            ui.github_issues_loading || ui.github_issues_loaded || ui.github_issues_error.is_some()
        });
        if owner_kept_issues_open {
            self.refresh_github_issues_for(owner, repo.clone(), cx);
            if let Some(number) = number {
                self.load_github_issue_detail_for(owner, repo, number, cx);
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_domain::github::{Issue, IssueState};

    fn issue(number: u64, title: &str) -> Issue {
        Issue {
            number,
            title: title.into(),
            state: IssueState::Open,
            url: String::new(),
            author: String::new(),
            assignees: Vec::new(),
            labels: Vec::new(),
            body: String::new(),
            comments: Vec::new(),
            comment_count: 0,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn issue_write_title_uses_create_fallback_but_real_reply_title() {
        let mut ui = TabUiState::default();
        ui.github_issues.push(issue(7, "list title"));
        ui.github_issue_details.insert(7, issue(7, "detail title"));
        ui.github_issues.push(issue(8, "fallback title"));
        let draft = IssueDraft {
            body: "reply body must not become its title".into(),
            ..Default::default()
        };

        assert_eq!(
            issue_write_title(&ui, None, &draft),
            draft.effective_title()
        );
        assert_eq!(issue_write_title(&ui, Some(7), &draft), "detail title");
        assert_eq!(issue_write_title(&ui, Some(8), &draft), "fallback title");
        assert_eq!(issue_write_title(&ui, Some(9), &draft), "");
    }
}
