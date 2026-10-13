//! Commit Panel template/input synchronization; state remains on CommitPanelView.
use super::*;

impl CommitPanelView {
    /// Whether the body currently carries the template's comment block.
    ///
    /// Derived from the text rather than tracked in a flag, so the toggle can
    /// never disagree with what is actually in the box (the user can delete the
    /// comments by hand).
    pub fn template_active(&self, cx: &gpui::App) -> bool {
        self.body_input
            .as_ref()
            .map(|i| {
                i.read(cx)
                    .value()
                    .lines()
                    .any(kagi_domain::message::is_comment_line)
            })
            .unwrap_or(false)
    }

    /// Add or remove the `commit.template` block in the body, and remember the
    /// choice for the next open.
    pub fn toggle_template(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        let Some(body) = self.body_input.clone() else {
            return;
        };
        let current = body.read(cx).value().to_string();
        let on = !self.template_active(cx);
        let next = if on {
            let Some(tpl) = self.commit_template.clone() else {
                return;
            };
            let written = kagi_domain::message::strip_template_comments(&current);
            if written.trim().is_empty() {
                tpl
            } else {
                format!("{}\n\n{}", written.trim_end(), tpl)
            }
        } else {
            kagi_domain::message::strip_template_comments(&current)
        };
        body.update(cx, |s, cx| s.set_value(next, window, cx));
        crate::ui::settings::write_setting(KEY_COMMIT_TEMPLATE, Some(if on { "1" } else { "0" }));
        klog!("commit template: {}", if on { "on" } else { "off" });
        cx.notify();
    }
    /// Render-time input sync: push a queued smart-commit message into the Input
    /// (correction #2 — was the parent `render.rs:196` block), then run the
    /// per-branch draft autosave (correction #1 — was the parent
    /// `sync_modal_inputs` half). Runs on this entity's own render path with
    /// `&mut Window`, so the parent never reads the child's input each frame.
    pub fn sync_inputs(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        self.sync_input_language(window, cx);
        if std::mem::take(&mut self.pending_message_clear) {
            for input in [self.title_input.clone(), self.body_input.clone()]
                .into_iter()
                .flatten()
            {
                input.update(cx, |state, cx| state.set_value("", window, cx));
            }
        }
        if let Some(msg) = self.pending_smart_msg.take() {
            self.replace_smart_message(&msg, window, cx);
        }

        // Observe edits separately: failed saves must not advance the baseline,
        // but also must not start an endless render-time retry loop.
        let value = self.effective_commit_message(cx);
        if value == self.draft_edit_value {
            return;
        }
        self.draft_edit_value = value;
        self.draft_save_gen = self.draft_save_gen.wrapping_add(1);
        let gen = self.draft_save_gen;
        let branch = self.draft_branch(cx);
        cx.spawn(async move |this, acx| {
            acx.background_executor()
                .timer(std::time::Duration::from_millis(250))
                .await;
            let _ = this.update(acx, |view, cx| {
                if view.draft_save_gen == gen {
                    view.persist_draft(&branch, cx);
                }
            });
        })
        .detach();
    }

    /// Smart is an ordinary reversible Input edit, unlike loading/consumption.
    pub(crate) fn replace_smart_message(
        &mut self,
        msg: &str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let (title, body) = kagi_git::split_title_body(msg);
        if let Some(input) = self.title_input.clone() {
            input.update(cx, |state, cx| state.replace_all(title, window, cx));
        }
        if let Some(input) = self.body_input.clone() {
            let existing = input.read(cx).value().to_string();
            let comments = existing
                .lines()
                .skip_while(|line| !line.trim_start().starts_with('#'))
                .collect::<Vec<_>>()
                .join("\n");
            let next = match (body.trim().is_empty(), comments.trim().is_empty()) {
                (_, true) => body,
                (true, false) => existing,
                (false, false) => format!("{}\n\n{}", body.trim_end(), comments),
            };
            input.update(cx, |state, cx| state.replace_all(next, window, cx));
        }
        self.state.commit_msg = msg.to_string();
        cx.notify();
    }

    fn draft_branch(&self, cx: &gpui::App) -> String {
        let tab_branch = self
            .app
            .read_with(cx, |app, _| {
                app.reads
                    .get(Some(self.owner))
                    .status_summary
                    .branch
                    .clone()
            })
            .unwrap_or_default();
        crate::ui::worktree_wip::draft_branch(
            self.foreign.as_ref().map(|(label, _)| label.as_ref()),
            &tab_branch,
        )
    }

    /// Explicit recovery path: save unchanged dirty text after storage recovers.
    pub(crate) fn retry_draft_save(&mut self, cx: &mut gpui::Context<Self>) {
        self.draft_edit_value = self.effective_commit_message(cx);
        self.draft_save_gen = self.draft_save_gen.wrapping_add(1);
        self.persist_draft(&self.draft_branch(cx), cx);
    }

    fn persist_draft(&mut self, branch: &str, cx: &mut gpui::Context<Self>) {
        match kagi_git::save_draft(&self.repo_path, branch, &self.draft_edit_value, "plain") {
            Ok(()) => {
                self.last_draft_value.clone_from(&self.draft_edit_value);
                self.draft_save_error = None;
                if !self.last_draft_value.trim().is_empty() {
                    klog!("draft: saved {}", branch);
                }
            }
            Err(error) => {
                self.draft_save_error = Some(error.to_string());
            }
        }
        cx.notify();
    }

    fn sync_input_language(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        // ── Language switch → rebuild the inputs for their placeholders. ──
        // `InputState` has no placeholder setter, so the text is carried across
        // a fresh pair. Rare (a menu action), and it only costs the caret.
        let lang = kagi_ui_core::i18n::lang();
        if lang != self.input_lang {
            self.input_lang = lang;
            if let Some(old) = self.title_input.clone() {
                let text = old.read(cx).value().to_string();
                let fresh =
                    cx.new(|cx| InputState::new(window, cx).placeholder(Msg::CommitTitle.t()));
                fresh.update(cx, |s, cx| s.set_value(text, window, cx));
                self.title_input = Some(fresh);
            }
            if let Some(old) = self.body_input.clone() {
                let text = old.read(cx).value().to_string();
                let fresh = cx.new(|cx| {
                    InputState::new(window, cx)
                        .multi_line(true)
                        .placeholder(Msg::CommitBody.t())
                });
                fresh.update(cx, |s, cx| s.set_value(text, window, cx));
                self.body_input = Some(fresh);
            }
        }
    }
}
