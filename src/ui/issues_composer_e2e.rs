//! Window-bearing test access to the production Composer input subscription.
//! No transport writes or replacement implementation of draft editing.

use gpui::{App, Context, Focusable, Window};
use kagi_domain::issue_composer::IssueDraft;

use super::i18n::{self, Lang};
use super::issues_composer::IssueEditor;
use super::KagiApp;

type IssueListResult =
    Result<kagi_domain::github::IssueListSnapshot, kagi_git::github::PrFetchError>;

thread_local! {
    static ISSUE_LIST_FETCH: std::cell::RefCell<Option<gpui::Task<IssueListResult>>> =
        const { std::cell::RefCell::new(None) };
}

pub(super) fn take_issue_list_fetch() -> Option<gpui::Task<IssueListResult>> {
    ISSUE_LIST_FETCH.with(|slot| slot.borrow_mut().take())
}

impl KagiApp {
    /// Replace only the transport future; production owner/generation and
    /// viewport-triggered request paths still run.
    pub fn queue_issue_list_fetch_for_e2e(task: gpui::Task<IssueListResult>) {
        ISSUE_LIST_FETCH.with(|slot| assert!(slot.borrow_mut().replace(task).is_none()));
    }

    pub fn issue_view_recomputations_for_e2e() -> usize {
        super::issues_mode::issue_view_recomputations()
    }

    pub fn set_issue_input_lang_for_e2e(
        &mut self,
        lang: Lang,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        i18n::set_lang(lang);
        self.sync_issue_inputs(window, cx);
    }

    pub fn issue_placeholder_lang_for_e2e(&self, number: Option<u64>) -> Option<Lang> {
        self.ui()
            .issue_composer
            .editors
            .get(&number)
            .and_then(|editor| editor.placeholder_lang)
    }

    /// Seed one Issue per navigator filter, with completed detail evidence for
    /// the row that the scenario opens. This drives only production filtering
    /// and rendering; no alternate E2E row implementation exists.
    pub fn seed_issue_navigation_for_e2e(&mut self, cx: &mut Context<Self>) {
        use kagi_domain::github::{Issue, IssueLabel, IssueState};

        let make =
            |number, title: &str, author: &str, assignees: &[&str], updated_at: &str| Issue {
                number,
                title: title.into(),
                state: IssueState::Open,
                url: format!("https://github.com/example/fixture/issues/{number}"),
                author: author.into(),
                assignees: assignees.iter().map(|name| (*name).into()).collect(),
                labels: vec![IssueLabel {
                    name: if matches!(number, 1 | 3) {
                        "bug"
                    } else {
                        "docs"
                    }
                    .into(),
                    color: "aabbcc".into(),
                    description: String::new(),
                }],
                body: format!("body for #{number}"),
                comments: Vec::new(),
                comment_count: number as usize,
                created_at: format!("2026-09-{:02}T00:00:00Z", 5 - number),
                updated_at: updated_at.into(),
            };
        let issues = vec![
            make(1, "assigned", "bob", &["alice"], "2026-09-17T00:00:00Z"),
            make(2, "created", "alice", &[], "2026-09-18T00:00:00Z"),
            make(3, "mentioned", "carol", &[], "2026-09-19T00:00:00Z"),
            make(4, "recent", "dave", &[], "2026-09-20T00:00:00Z"),
        ];
        self.github_login = Some("alice".into());
        let ui = self.ui_mut().expect("fixture session");
        ui.github_issues_gen = ui.github_issues_gen.wrapping_add(1);
        ui.github_issues = issues;
        ui.github_issue_mentions = vec![3];
        ui.github_issues_loaded = true;
        ui.github_issues_loading = false;
        ui.github_issues_loading_more = false;
        ui.github_issues_cursor = None;
        ui.github_issues_list.reset(0);
        ui.github_issues_error = None;
        ui.github_issue_details
            .insert(4, make(4, "recent", "dave", &[], "2026-09-20T00:00:00Z"));
        cx.notify();
    }

    /// Seed only completed read evidence, before entering Issues. This avoids
    /// requiring a real GitHub repository lookup for editor-only GUI coverage.
    pub fn seed_issue_composer_for_e2e(&mut self, cx: &mut Context<Self>) {
        let repo = self.repo_path.clone().expect("fixture repository");
        let storage_version = kagi_git::drafts::issue_draft_version(&repo, None);
        let state = &mut self.ui_mut().expect("fixture session").issue_composer;
        state.base_repo = Some("example/fixture".into());
        state.repo_error = None;
        state.editors.insert(
            None,
            IssueEditor {
                loaded: true,
                repo: Some(repo),
                storage_version,
                ..Default::default()
            },
        );
        cx.notify();
    }

    /// Add a selected Issue and its Reply editor without performing a GitHub
    /// read. The production render path still creates the actual InputState.
    pub fn seed_issue_reply_for_e2e(&mut self, number: u64, cx: &mut Context<Self>) {
        let repo = self.repo_path.clone().expect("fixture repository");
        let storage_version = kagi_git::drafts::issue_draft_version(&repo, Some(number));
        let ui = self.ui_mut().expect("fixture session");
        ui.selected_github_issue = Some(number);
        ui.issue_composer.editors.insert(
            Some(number),
            IssueEditor {
                loaded: true,
                repo: Some(repo),
                storage_version,
                ..Default::default()
            },
        );
        cx.notify();
    }

    /// Replace a seeded Issue's body, so a scenario can draw a real Markdown
    /// document through the production Thread path (#751). The body is the
    /// caller's: the fixture under `tests/support` is the one document every
    /// surface is checked against.
    pub fn set_issue_body_for_e2e(&mut self, number: u64, body: &str, cx: &mut Context<Self>) {
        let ui = self.ui_mut().expect("fixture session");
        let issue = ui
            .github_issue_details
            .get_mut(&number)
            .expect("seeded Issue detail");
        issue.body = body.to_string();
        cx.notify();
    }

    /// Exercise InputState's normal edit/Change event, not a direct draft write.
    /// `set_value` deliberately suppresses Change and would miss the subscription.
    pub fn insert_issue_body_for_e2e(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_issue_inputs(window, cx);
        let input = self
            .ui()
            .issue_composer
            .editors
            .get(&None)
            .and_then(|editor| editor.body_input.clone())
            .expect("Composer body input created by window-bearing render");
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.replace(text.to_owned(), window, cx);
        });
    }

    /// Replace the whole Composer body, the way a formatter would: through
    /// the production InputState, so the draft subscription still runs.
    /// `insert_issue_body_for_e2e` types at the cursor and therefore appends
    /// — that is what it is for; this is for a scenario that needs to start
    /// from a known document (#751).
    pub fn replace_issue_body_for_e2e(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_issue_inputs(window, cx);
        let input = self
            .ui()
            .issue_composer
            .editors
            .get(&None)
            .and_then(|editor| editor.body_input.clone())
            .expect("Composer body input created by window-bearing render");
        input.update(cx, |state, cx| {
            state.replace_all(text.to_owned(), window, cx);
        });
    }

    /// Put the New Issue Composer back where a scenario found it: empty
    /// title, empty body, body hidden again. A scenario that exercises the
    /// pristine Composer has to leave it pristine, or every measure after it
    /// quietly asserts against a dirty draft (#751).
    pub fn reset_issue_composer_for_e2e(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_issue_inputs(window, cx);
        let (title, body) = self
            .ui()
            .issue_composer
            .editors
            .get(&None)
            .map(|editor| (editor.title_input.clone(), editor.body_input.clone()))
            .expect("seeded Composer");
        for input in title.into_iter().chain(body) {
            input.update(cx, |state, cx| state.replace_all(String::new(), window, cx));
        }
        if let Some(editor) = self
            .ui_mut()
            .and_then(|ui| ui.issue_composer.editors.get_mut(&None))
        {
            editor.body_revealed = false;
        }
        cx.notify();
    }

    pub fn issue_composer_snapshot_for_e2e(&self) -> (IssueDraft, bool) {
        let editor = self
            .ui()
            .issue_composer
            .editors
            .get(&None)
            .expect("seeded Composer");
        (editor.draft.clone(), editor.focused)
    }

    /// Focus the production New Issue title input before the harness sends a
    /// normal Enter key. The production InputEvent subscription owns the
    /// reveal/focus transition being tested.
    pub fn focus_issue_title_for_e2e(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_issue_inputs(window, cx);
        let input = self
            .ui()
            .issue_composer
            .editors
            .get(&None)
            .and_then(|editor| editor.title_input.clone())
            .expect("New Issue title input created by window-bearing render");
        input.update(cx, |state, cx| state.focus(window, cx));
    }

    /// Observe both halves of the production Enter transition: persistent
    /// body visibility and transfer of window focus to the real body input.
    pub fn issue_composer_enter_state_for_e2e(
        &self,
        window: &Window,
        cx: &App,
    ) -> (bool, bool, String, String) {
        let editor = self
            .ui()
            .issue_composer
            .editors
            .get(&None)
            .expect("seeded Composer");
        let body_focused = editor
            .body_input
            .as_ref()
            .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window));
        let title_value = editor
            .title_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();
        let body_value = editor
            .body_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();
        (editor.body_revealed, body_focused, title_value, body_value)
    }

    pub fn issue_inputs_focused_for_e2e(
        &self,
        number: Option<u64>,
        window: &Window,
        cx: &App,
    ) -> bool {
        let Some(editor) = self.ui().issue_composer.editors.get(&number) else {
            return false;
        };
        editor
            .body_input
            .iter()
            .chain(editor.title_input.iter())
            .any(|input| input.read(cx).focus_handle(cx).is_focused(window))
    }

    /// Settle the currently queued New Issue revision through the production
    /// completion path, without dispatching a GitHub write in this harness.
    pub fn settle_issue_write_for_e2e(&mut self, cx: &mut Context<Self>) {
        let owner = self.active_session().expect("fixture session");
        let repo = self.repo_path.clone().expect("fixture repository");
        let version = self
            .ui()
            .issue_composer
            .editors
            .get(&None)
            .expect("seeded Composer")
            .storage_version;
        self.settle_issue_write(owner, repo, None, version, cx);
    }

    pub fn issue_preview_for_e2e(&self) -> bool {
        self.ui()
            .issue_composer
            .editors
            .get(&None)
            .is_some_and(|editor| editor.preview)
    }

    pub fn issue_reply_preview_for_e2e(&self, number: u64) -> bool {
        self.ui()
            .issue_composer
            .editors
            .get(&Some(number))
            .is_some_and(|editor| editor.preview)
    }

    /// Put the Reply's real source input on the focus path before dispatching
    /// the scoped FocusIssueEditor key binding.
    pub fn focus_issue_reply_input_for_e2e(
        &mut self,
        number: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_issue_inputs(window, cx);
        let input = self
            .ui()
            .issue_composer
            .editors
            .get(&Some(number))
            .and_then(|editor| editor.body_input.clone())
            .expect("Reply body input created by window-bearing render");
        input.update(cx, |state, cx| state.focus(window, cx));
    }

    pub fn issue_reply_focused_for_e2e(&self, number: u64) -> bool {
        self.ui()
            .issue_composer
            .editors
            .get(&Some(number))
            .is_some_and(|editor| editor.focused)
    }

    pub fn issue_reply_has_title_input_for_e2e(&self, number: u64) -> bool {
        self.ui()
            .issue_composer
            .editors
            .get(&Some(number))
            .is_some_and(|editor| editor.title_input.is_some())
    }

    /// Exercise the Reply body's production Change subscription after the
    /// New Issue-only title InputState was removed.
    pub fn insert_issue_reply_body_for_e2e(
        &mut self,
        number: u64,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_issue_inputs(window, cx);
        let input = self
            .ui()
            .issue_composer
            .editors
            .get(&Some(number))
            .and_then(|editor| editor.body_input.clone())
            .expect("Reply body input created by window-bearing render");
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.replace(text.to_owned(), window, cx);
        });
    }

    pub fn issue_reply_draft_for_e2e(&self, number: u64) -> IssueDraft {
        self.ui()
            .issue_composer
            .editors
            .get(&Some(number))
            .expect("seeded Reply Composer")
            .draft
            .clone()
    }

    /// Exercise the production selection transition without starting a real
    /// GitHub detail request in the GUI harness.
    pub fn select_issue_for_e2e(
        &mut self,
        number: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(focus) = self.root_focus.clone() {
            window.focus(&focus, cx);
        }
        self.select_github_issue(number);
        cx.notify();
    }

    pub fn selected_issue_for_e2e(&self) -> Option<u64> {
        self.ui().selected_github_issue
    }
}
