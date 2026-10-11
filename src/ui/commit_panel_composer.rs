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

        // ── Queued smart-commit message → Input (needs `&mut Window`). ──
        if let Some(msg) = self.pending_smart_msg.take() {
            let (title, body) = kagi_git::split_title_body(&msg);
            if let Some(input) = self.title_input.clone() {
                input.update(cx, |state, cx| state.set_value(title, window, cx));
            }
            if let Some(input) = self.body_input.clone() {
                input.update(cx, |state, cx| state.set_value(body, window, cx));
            }
        }

        // ── Commit-message draft autosave (T-COMMIT-007 / T-COMMIT-009) ──
        if self.title_input.is_some() || self.body_input.is_some() {
            let v = self.effective_commit_message(cx);
            if v != self.last_draft_value {
                self.last_draft_value = v;
                self.draft_save_gen = self.draft_save_gen.wrapping_add(1);
                let gen = self.draft_save_gen;
                let mode = "plain".to_string();
                let repo_path = self.repo_path.clone();
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
                // A queued checkout can switch HEAD during the debounce. The
                // draft still belongs to the branch on which editing occurred.
                let branch = crate::ui::worktree_wip::draft_branch(
                    self.foreign.as_ref().map(|(label, _)| label.as_ref()),
                    &tab_branch,
                );
                cx.spawn(async move |this, acx| {
                    acx.background_executor()
                        .timer(std::time::Duration::from_millis(250))
                        .await;
                    let _ = this.update(acx, |view, _cx| {
                        if view.draft_save_gen != gen {
                            return;
                        }
                        let msg = view.last_draft_value.clone();
                        if msg.trim().is_empty() {
                            let _ = kagi_git::clear_draft(&repo_path, &branch);
                        } else {
                            let _ = kagi_git::save_draft(&repo_path, &branch, &msg, &mode);
                            klog!("draft: saved {}", branch);
                        }
                    });
                })
                .detach();
            }
        }
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
