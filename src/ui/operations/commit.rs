//! Commit panel: staging, smart-commit, template handling, and the commit operation.
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 4, P1) as an additional
//! `impl KagiApp` block. Behaviour and signatures are unchanged; a descendant
//! module can access `KagiApp` privates so no visibility was widened.

#![allow(clippy::too_many_arguments)]

use crate::ui::*;

impl KagiApp {
    /// Lazily create the six template-field `InputState`s (requires `&mut
    /// Window`). Order: `[type, scope, summary, body, test, risk]`. The body is
    /// multi-line; the rest are single-line. No-op once created.
    fn ensure_template_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.commit_template_inputs.is_some() {
            return;
        }
        let ty = cx.new(|cx| InputState::new(window, cx).placeholder("type (feat, fix, …)"));
        let scope = cx.new(|cx| InputState::new(window, cx).placeholder("scope (optional)"));
        let summary = cx.new(|cx| InputState::new(window, cx).placeholder("summary"));
        let body = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .auto_grow(2, 8)
                .placeholder("body (optional)")
        });
        let test =
            cx.new(|cx| InputState::new(window, cx).placeholder("Test: how verified (optional)"));
        let risk =
            cx.new(|cx| InputState::new(window, cx).placeholder("Risk: known risks (optional)"));
        self.commit_template_inputs = Some([ty, scope, summary, body, test, risk]);
    }

    /// Read the six template `InputState`s into a [`TemplateFields`].
    /// Returns `default()` when the inputs have not been created yet.
    fn template_fields_from_inputs(&self, cx: &Context<Self>) -> kagi::git::TemplateFields {
        match &self.commit_template_inputs {
            Some([ty, scope, summary, body, test, risk]) => kagi::git::TemplateFields::new(
                ty.read(cx).value().to_string(),
                scope.read(cx).value().to_string(),
                summary.read(cx).value().to_string(),
                body.read(cx).value().to_string(),
                test.read(cx).value().to_string(),
                risk.read(cx).value().to_string(),
            ),
            None => kagi::git::TemplateFields::default(),
        }
    }

    /// Write a [`TemplateFields`] into the six template `InputState`s.
    pub(crate) fn set_template_inputs(
        &mut self,
        fields: &kagi::git::TemplateFields,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_template_inputs(window, cx);
        if let Some([ty, scope, summary, body, test, risk]) = self.commit_template_inputs.clone() {
            ty.update(cx, |s, cx| s.set_value(fields.r#type.clone(), window, cx));
            scope.update(cx, |s, cx| s.set_value(fields.scope.clone(), window, cx));
            summary.update(cx, |s, cx| s.set_value(fields.summary.clone(), window, cx));
            body.update(cx, |s, cx| s.set_value(fields.body.clone(), window, cx));
            test.update(cx, |s, cx| s.set_value(fields.test.clone(), window, cx));
            risk.update(cx, |s, cx| s.set_value(fields.risk.clone(), window, cx));
        }
    }

    /// Toggle between plain and template authoring modes, carrying the content
    /// across so a toggle never loses the user's work (T-COMMIT-009):
    ///
    /// - plain → template: best-effort parse the plain Input into the fields.
    /// - template → plain: assemble the fields and pour the result into the
    ///   plain Input.
    ///
    /// The new mode is mirrored straight into the draft (bumping the autosave
    /// generation) so a mode switch survives a restart.
    pub fn toggle_commit_template_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.commit_template_mode {
            // template → plain: assemble + pour into the plain Input.
            let fields = self.template_fields_from_inputs(cx);
            let assembled = kagi::git::assemble(&fields);
            if self.commit_input.is_none() {
                let st = cx.new(|cx| InputState::new(window, cx).placeholder("Commit message"));
                self.commit_input = Some(st);
            }
            if let Some(input) = self.commit_input.clone() {
                input.update(cx, |s, cx| s.set_value(assembled, window, cx));
                input.update(cx, |s, cx| s.focus(window, cx));
            }
            self.commit_template_mode = false;
        } else {
            // plain → template: parse the plain Input into the fields.
            let plain = self
                .commit_input
                .as_ref()
                .map(|i| i.read(cx).value().to_string())
                .unwrap_or_default();
            let fields = kagi::git::parse_message(&plain);
            self.set_template_inputs(&fields, window, cx);
            self.commit_template_mode = true;
            // Focus the summary field (index 2) — the most-edited one.
            if let Some(inputs) = self.commit_template_inputs.clone() {
                inputs[2].update(cx, |s, cx| s.focus(window, cx));
            }
        }
        // Persist the new mode immediately (with the current effective message).
        self.bump_draft_for_mode_change(cx);
        cx.notify();
    }

    /// Compute the effective single-message text for the current mode: the
    /// assembled template (template mode) or the plain Input value (plain mode).
    /// Used by autosave so a template draft stores its expanded plain text
    /// (ADR-0042).
    pub(crate) fn effective_commit_message(&self, cx: &Context<Self>) -> String {
        if self.commit_template_mode {
            kagi::git::assemble(&self.template_fields_from_inputs(cx))
        } else {
            self.commit_input
                .as_ref()
                .map(|i| i.read(cx).value().to_string())
                .unwrap_or_default()
        }
    }

    /// Force a draft save on the next debounce tick after a mode change, so the
    /// `mode` field is persisted even if the message text is unchanged.
    fn bump_draft_for_mode_change(&mut self, cx: &mut Context<Self>) {
        let msg = self.effective_commit_message(cx);
        self.last_draft_value = msg;
        self.draft_save_gen = self.draft_save_gen.wrapping_add(1);
        let gen = self.draft_save_gen;
        let mode = if self.commit_template_mode {
            "template"
        } else {
            "plain"
        };
        let mode = mode.to_string();
        cx.spawn(async move |this, acx| {
            gpui::Timer::after(Duration::from_millis(250)).await;
            let _ = this.update(acx, |app, _cx| {
                if app.draft_save_gen != gen {
                    return;
                }
                let Some(rp) = app.repo_path.clone() else {
                    return;
                };
                let branch = app.status_summary.branch.clone();
                let msg = app.last_draft_value.clone();
                if msg.trim().is_empty() {
                    let _ = kagi::git::clear_draft(&rp, &branch);
                } else {
                    let _ = kagi::git::save_draft(&rp, &branch, &msg, &mode);
                }
            });
        })
        .detach();
    }

    /// Open the commit panel (triggered by clicking the WIP row).
    ///
    /// Loads the current staging status from the repository.
    /// Clears any existing commit selection so the two views are exclusive.
    pub fn open_commit_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // T026: lazy-create the InputState (requires &mut Window) on first open.
        if self.commit_input.is_none() {
            let input_entity =
                cx.new(|cx| InputState::new(window, cx).placeholder("Commit message"));
            self.commit_input = Some(input_entity);
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                eprintln!("[kagi] open_commit_panel: no repo_path set");
                return;
            }
        };
        let mut panel = CommitPanelState::from_repo(&repo_path);
        // Preserve tree_view toggle if we're reopening an existing panel.
        if let Some(ref existing) = self.commit_panel {
            panel.tree_view = existing.tree_view;
        }
        self.commit_panel = Some(panel);
        self.commit_panel_open = true;
        self.selected = None;
        self.main_diff = None;

        // T-COMMIT-007 / T-COMMIT-009: restore the per-branch draft into an
        // empty input, honouring the persisted mode. A template draft stores its
        // expanded plain text (ADR-0042); on restore we re-parse it back into the
        // structured fields and re-open in template mode.
        if let Some(ref input_entity) = self.commit_input {
            let current = input_entity.read(cx).value().to_string();
            if current.trim().is_empty() {
                let branch = self.status_summary.branch.clone();
                if let Some(d) = kagi::git::load_draft(&repo_path, &branch) {
                    eprintln!("[kagi] draft: loaded {} (mode={})", branch, d.mode);
                    let entity = input_entity.clone();
                    if d.mode == "template" {
                        let fields = kagi::git::parse_message(&d.message);
                        self.set_template_inputs(&fields, window, cx);
                        self.commit_template_mode = true;
                        self.last_draft_value = d.message;
                    } else {
                        entity.update(cx, |state, cx| {
                            state.set_value(d.message, window, cx);
                        });
                        self.commit_template_mode = false;
                        self.last_draft_value = entity.read(cx).value().to_string();
                    }
                }
            }
        }

        // T026: focus the InputState after opening the panel.
        if let Some(ref input_entity) = self.commit_input {
            input_entity.update(cx, |state, cx| {
                state.focus(window, cx);
            });
        }

        // Log for headless verification.
        if let Some(ref p) = self.commit_panel {
            eprintln!(
                "[kagi] commit-panel: unstaged={} staged={}",
                p.unstaged.len(),
                p.staged.len()
            );
        }

        // T-COMMIT-016: probe for a local Ollama server (reachability only;
        // no diff is sent). Runs at most once per repo, off the UI thread.
        self.ensure_smart_commit_detection(cx);
    }

    /// Probe for a reachable local Ollama server in the background.
    ///
    /// Reachability only — a single short GET to `/api/tags`; the staged diff is
    /// **never** sent here.  Runs at most once per repo path, off the UI thread.
    /// On success the panel shows "Local LLM available".  No-op when
    /// `KAGI_OFFLINE=1`.
    fn ensure_smart_commit_detection(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        if self.smart_commit_detected_for.as_deref() == Some(repo_path.as_path()) {
            return;
        }
        self.smart_commit_detected_for = Some(repo_path);

        if message_gen::offline() {
            eprintln!("[kagi] smart-commit: offline (detection skipped)");
            return;
        }

        let host = smart_commit::SmartCommitState::ollama_host();
        let task = cx.background_spawn(async move {
            let available = message_gen::ollama_available(&host);
            let models = if available {
                message_gen::ollama_list_models(&host)
            } else {
                Vec::new()
            };
            (available, models)
        });
        cx.spawn(async move |this, acx| {
            let (available, models) = task.await;
            let _ = this.update(acx, |app, cx| {
                app.smart_commit.ollama_available = available;
                app.smart_commit.detected_models = models;
                eprintln!(
                    "[kagi] smart-commit: ollama_available={} models={}",
                    available,
                    app.smart_commit.detected_models.len()
                );
                cx.notify();
            });
        })
        .detach();
    }

    /// Read the current commit-message Input value (UI) or headless `commit_msg`.
    fn smart_commit_current_msg(&self, cx: &Context<Self>) -> String {
        if let Some(ref input) = self.commit_input {
            input.read(cx).value().to_string()
        } else {
            self.commit_panel
                .as_ref()
                .map(|p| p.commit_msg.clone())
                .unwrap_or_default()
        }
    }

    /// Write `msg` into the commit-message Input (and the headless mirror).
    /// Only overwrites a non-empty existing message after the caller has
    /// decided to (rule-based/LLM both call this to *insert* the draft).
    fn smart_commit_set_msg(&mut self, msg: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = self.commit_input.clone() {
            input.update(cx, |state, cx| {
                state.set_value(msg.to_string(), window, cx);
            });
        }
        if let Some(panel) = self.commit_panel.as_mut() {
            panel.commit_msg = msg.to_string();
        }
    }

    /// "Suggest" button — rule-based draft (always available, never networked).
    ///
    /// Inserts the draft into the message Input.  If the Input already holds a
    /// non-empty message it is left untouched (the user's text wins; ticket:
    /// overwrite only when empty).
    pub fn smart_suggest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };
        let files = repo.collect_staged_files();
        // ADR-0090: style follows the mode (template → Conventional, else Plain).
        let style = if self.commit_template_mode {
            message_gen::Style::ConventionalCommits
        } else {
            message_gen::Style::Plain
        };
        let gi = message_gen::GenInput {
            diff: String::new(),
            lang: self.smart_commit.lang,
            style,
            want_body: self.commit_template_mode,
        };
        let msg = message_gen::rule_based(&gi, &files);
        if std::env::var("KAGI_SMART_SUGGEST").as_deref() == Ok("1") {
            eprintln!("[kagi] smart-suggest: {}", msg);
        }
        let existing = self.smart_commit_current_msg(cx);
        if existing.trim().is_empty() {
            self.smart_commit_set_msg(&msg, window, cx);
            self.smart_commit.status = Some("Rule-based suggestion inserted".to_string());
        } else {
            self.smart_commit.status = Some("Message not empty — kept your text".to_string());
        }
        cx.notify();
    }

    /// "Generate with Local LLM" button.
    ///
    /// Enforces the opt-in gates: if the user has not yet enabled LLM generation
    /// (or never confirmed a model) the consent / model-picker modal is shown
    /// first.  Only when all gates are cleared is the staged diff collected and
    /// sent to loopback Ollama (in the background, with a timeout).  Any failure
    /// falls back **quietly** to the rule-based draft.
    pub fn smart_generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if message_gen::offline() {
            // Offline → straight to rule-based, no modal.
            self.smart_suggest(window, cx);
            return;
        }
        // Gate 1: first-time consent.
        if !self.smart_commit.llm_enabled {
            self.smart_commit.modal = Some(smart_commit::SmartCommitModal::Consent);
            cx.notify();
            return;
        }
        // Gate 2: model selection (1 model still needs confirmation, multiple
        // must be chosen — both surface as the picker on first use).
        if self.smart_commit.model.is_none() {
            self.open_smart_model_picker(cx);
            return;
        }
        self.run_smart_generation(window, cx);
    }

    /// Show the model picker, listing the detected models (`/api/tags`).
    fn open_smart_model_picker(&mut self, cx: &mut Context<Self>) {
        let models = self.smart_commit.detected_models.clone();
        if models.is_empty() {
            // No models installed → nothing to pick; fall back quietly.
            self.smart_commit.status = Some("No local models found — using rule-based".to_string());
            cx.notify();
            return;
        }
        self.smart_commit.modal = Some(smart_commit::SmartCommitModal::ModelPicker { models });
        cx.notify();
    }

    /// Consent dialog confirmed: enable LLM, then proceed to model selection.
    pub fn confirm_smart_consent(&mut self, cx: &mut Context<Self>) {
        self.smart_commit.set_enabled(true);
        self.smart_commit.modal = None;
        eprintln!("[kagi] smart-commit: llm enabled (consent given)");
        // Move on to picking a model (always confirm at least once per ADR).
        if self.smart_commit.model.is_none() {
            self.open_smart_model_picker(cx);
        } else {
            cx.notify();
        }
    }

    /// Model chosen from the picker: persist it and continue to generation.
    pub fn choose_smart_model(
        &mut self,
        model: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.smart_commit.set_model(model.clone());
        self.smart_commit.modal = None;
        eprintln!("[kagi] smart-commit: model selected = {}", model);
        self.run_smart_generation(window, cx);
    }

    /// Dismiss any Smart Commit modal without action.
    pub fn cancel_smart_modal(&mut self, cx: &mut Context<Self>) {
        self.smart_commit.modal = None;
        cx.notify();
    }

    /// Collect the staged diff and dispatch generation on a background thread.
    ///
    /// Sends only the staged diff to loopback Ollama (ureq + global timeout in
    /// the backend).  On any `Err` the result falls back to the rule-based draft
    /// so the UI never blocks or shows a blocking error.
    fn run_smart_generation(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let (Some(model), lang) = (self.smart_commit.model.clone(), self.smart_commit.lang) else {
            return;
        };
        // ADR-0090: the standalone Style toggle is gone — derive it from the
        // mode. Template mode needs a Conventional subject so it can be parsed
        // into the type/scope/summary fields; plain mode uses a plain subject.
        let style = if self.commit_template_mode {
            message_gen::Style::ConventionalCommits
        } else {
            message_gen::Style::Plain
        };
        // Template mode wants a body too (its body field would otherwise be empty).
        let want_body = self.commit_template_mode;
        let host = smart_commit::SmartCommitState::ollama_host();
        let overwrite_ok = self.smart_commit_current_msg(cx).trim().is_empty();

        self.smart_commit.generating = true;
        self.smart_commit.status = Some("Generating with local LLM…".to_string());
        cx.notify();

        let task = cx.background_spawn(async move {
            let repo = match kagi::git::Backend::open(&repo_path) {
                Ok(r) => r,
                Err(_) => return None,
            };
            let files = repo.collect_staged_files();
            let diff = repo.collect_staged_diff();
            let gi = message_gen::GenInput {
                diff,
                lang,
                style,
                want_body,
            };
            // LLM first; on Err fall back to the rule-based draft (quietly).
            let backend = message_gen::MessageBackend::Ollama { host, model };
            let (msg, used_llm) = match message_gen::generate_message(&backend, &gi, &files) {
                Ok(m) => (m, true),
                Err(e) => {
                    eprintln!("[kagi] smart-commit: llm failed ({}) → rule-based", e);
                    (message_gen::rule_based(&gi, &files), false)
                }
            };
            Some((msg, used_llm))
        });

        cx.spawn(async move |this, acx| {
            let out = task.await;
            let _ = this.update(acx, |app, cx| {
                app.smart_commit.generating = false;
                match out {
                    Some((msg, used_llm)) if !msg.trim().is_empty() => {
                        if overwrite_ok {
                            // The Input's set_value needs `&mut Window`, which is
                            // unavailable here. Mirror into the panel state and
                            // queue the message; the next render (which has a
                            // Window) pushes it into the Input.
                            if let Some(panel) = app.commit_panel.as_mut() {
                                panel.commit_msg = msg.clone();
                            }
                            app.pending_smart_msg = Some(msg.clone());
                            app.smart_commit.status = Some(if used_llm {
                                "Generated with local LLM".to_string()
                            } else {
                                "LLM unavailable — used rule-based".to_string()
                            });
                        } else {
                            app.smart_commit.status =
                                Some("Message not empty — kept your text".to_string());
                        }
                    }
                    _ => {
                        app.smart_commit.status =
                            Some("Generation failed — edit manually".to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Stage a single file in the commit panel.
    ///
    /// Calls `stage_file` from T024 and then refreshes the staging status.
    /// Stage every non-conflicted unstaged file (T-UI-002: Stage all).
    pub fn do_stage_all(&mut self) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let paths: Vec<std::path::PathBuf> = match self.commit_panel.as_ref() {
            Some(p) => p
                .unstaged
                .iter()
                .filter(|f| !p.is_conflicted(&f.path))
                .map(|f| f.path.clone())
                .collect(),
            None => return,
        };
        if paths.is_empty() {
            return;
        }
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };
        match repo.stage_files(&paths) {
            Ok(n) => {
                eprintln!("[kagi] staged-all: {} file(s)", n);
                if let Some(panel) = self.commit_panel.as_mut() {
                    panel.reload_status(&repo_path);
                }
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("stage all failed: {}", e)));
            }
        }
    }

    /// Unstage every staged file (T-UI-002: Unstage all).
    pub fn do_unstage_all(&mut self) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let paths: Vec<std::path::PathBuf> = match self.commit_panel.as_ref() {
            Some(p) => p.staged.iter().map(|f| f.path.clone()).collect(),
            None => return,
        };
        if paths.is_empty() {
            return;
        }
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };
        match repo.unstage_files(&paths) {
            Ok(n) => {
                eprintln!("[kagi] unstaged-all: {} file(s)", n);
                if let Some(panel) = self.commit_panel.as_mut() {
                    panel.reload_status(&repo_path);
                }
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("unstage all failed: {}", e)));
            }
        }
    }

    pub fn do_stage_file(&mut self, index: usize) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let path = match self
            .commit_panel
            .as_ref()
            .and_then(|p| p.unstaged.get(index))
        {
            Some(f) => f.path.clone(),
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] stage_file: repo open error: {}", e);
                return;
            }
        };
        if let Err(e) = repo.stage_file(&path) {
            eprintln!("[kagi] stage_file error: {}", e);
        } else {
            eprintln!("[kagi] staged: {}", path.display());
        }
        if let Some(ref mut panel) = self.commit_panel {
            panel.reload_status(&repo_path);
            eprintln!(
                "[kagi] commit-panel: unstaged={} staged={}",
                panel.unstaged.len(),
                panel.staged.len()
            );
        }
    }

    /// Unstage a single file in the commit panel.
    ///
    /// Calls `unstage_file` from T024 and then refreshes the staging status.
    pub fn do_unstage_file(&mut self, index: usize) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let path = match self.commit_panel.as_ref().and_then(|p| p.staged.get(index)) {
            Some(f) => f.path.clone(),
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] unstage_file: repo open error: {}", e);
                return;
            }
        };
        if let Err(e) = repo.unstage_file(&path) {
            eprintln!("[kagi] unstage_file error: {}", e);
        } else {
            eprintln!("[kagi] unstaged: {}", path.display());
        }
        if let Some(ref mut panel) = self.commit_panel {
            panel.reload_status(&repo_path);
            eprintln!(
                "[kagi] commit-panel: unstaged={} staged={}",
                panel.unstaged.len(),
                panel.staged.len()
            );
        }
    }

    /// T-UI-003: Select a file in the commit panel and open it in the main diff pane.
    pub fn select_commit_panel_file(&mut self, file_ref: CommitPanelFileRef) {
        self.open_main_diff_wip(file_ref);
    }

    /// Handle a key-down event for the commit message input.
    ///
    /// Uses the T014 simple pattern: printable chars appended, backspace removes last.
    #[allow(dead_code)]
    pub fn handle_commit_msg_key(&mut self, event: &KeyDownEvent) {
        let panel = match self.commit_panel.as_mut() {
            Some(p) => p,
            None => return,
        };
        let key = &event.keystroke.key;
        let modifiers = &event.keystroke.modifiers;

        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }

        if key == "backspace" {
            panel.commit_msg.pop();
        } else if key == "space" {
            panel.commit_msg.push(' ');
        } else if key.len() == 1 {
            let ch = key.chars().next().unwrap();
            if !ch.is_control() {
                panel.commit_msg.push(ch);
            }
        }
    }

    /// Open the commit plan modal for the current staged files and message.
    ///
    /// Uses `plan_commit` from T024.
    /// T026: reads message from InputState if available, else falls back to commit_panel.commit_msg
    /// (used by the headless KAGI_COMMIT_MSG path).
    pub fn open_commit_plan_modal(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // T026 / T-COMMIT-009: prefer the effective message (assembled template
        // in template mode, else the plain Input); fall back to commit_msg
        // (headless path).
        let msg: String = if self.commit_input.is_some() || self.commit_template_mode {
            self.effective_commit_message(cx)
        } else {
            match self.commit_panel.as_ref() {
                Some(p) => p.commit_msg.clone(),
                None => return,
            }
        };
        if msg.trim().is_empty() {
            return;
        }
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] plan_commit: repo open error: {}", e);
                return;
            }
        };
        match repo.plan_commit(&msg) {
            Ok(plan) => {
                let has_blockers = !plan.blockers.is_empty();
                eprintln!(
                    "[kagi] plan: commit blockers={} warnings={}",
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                if let Some(ref mut panel) = self.commit_panel {
                    panel.plan_modal = Some(CommitPlanModal {
                        plan: std::sync::Arc::new(plan),
                        error: None,
                    });
                }
                // Smooth commit (user request): with no blockers, commit immediately
                // instead of showing a "commit?" confirmation popup. The pre-commit
                // checklist blockers (conflict markers / secrets / large binaries)
                // still surface the modal as a safety net. `start_commit` captures the
                // plan synchronously, so we can drop the modal right after to suppress
                // the popup; success/failure shows in the status footer.
                if !has_blockers {
                    self.start_commit(cx);
                    if let Some(ref mut panel) = self.commit_panel {
                        panel.plan_modal = None;
                    }
                }
            }
            Err(e) => {
                eprintln!("[kagi] plan_commit error: {}", e);
            }
        }
    }

    /// Cancel the commit plan modal.
    pub fn cancel_commit_plan_modal(&mut self) {
        if let Some(ref mut panel) = self.commit_panel {
            panel.plan_modal = None;
        }
    }

    /// W15-ASYNCOPS: UI-path commit — tree-build + write on a background thread.
    /// The message is read from the Input on the main thread; the branch draft is
    /// cleared in the finish step (also main thread). The headless KAGI_* path
    /// executes `execute_commit` directly.
    pub fn start_commit(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let commit_message: String = if self.commit_input.is_some() || self.commit_template_mode {
            self.effective_commit_message(cx)
        } else {
            self.commit_panel
                .as_ref()
                .map(|p| p.commit_msg.clone())
                .unwrap_or_default()
        };
        let plan = match self
            .commit_panel
            .as_ref()
            .and_then(|p| p.plan_modal.as_ref())
        {
            Some(modal) => modal.plan.clone(),
            None => return,
        };
        if !plan.blockers.is_empty() {
            eprintln!("[kagi] refused: commit plan has blockers");
            return;
        }

        // ADR-0068 (T-CONFLICT-FLOW-031): a merge that was continued routes the
        // commit button here with MERGE_HEAD still present.  Create the 2-parent
        // merge commit (HEAD + MERGE_HEAD) + cleanup_state instead of a plain
        // single-parent commit.  This is synchronous (cheap; no tree rebuild on a
        // worker) so the conflict-mode transition stays simple.
        if self.conflict_merge_commit_pending {
            self.finish_merge_commit(&commit_message, &plan, cx);
            return;
        }

        self.busy_op = Some("commit");
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyCommit.t()));
        eprintln!("[kagi] async: commit started");

        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_msg = commit_message.clone();
        // T-UNDOREDO-001: capture branch + tip BEFORE the commit (main thread).
        let history_before = self.head_branch_and_sha();
        let history_summary_line: String = commit_message
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(72)
            .collect();
        let task = cx.background_spawn(async move { commit_blocking(&bg_path, &bg_plan, &bg_msg) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((_new_short, after)) => {
                        eprintln!("[kagi] async: commit finished");
                        // A successful commit clears the branch draft (T-COMMIT-007).
                        let branch = app.status_summary.branch.clone();
                        let _ = kagi::git::clear_draft(&repo_path, &branch);
                        eprintln!("[kagi] draft: cleared {}", branch);
                        app.last_draft_value = String::new();

                        app.record_op(
                            "commit",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        if let (Some((hbranch, before)), Some((_, after_sha))) =
                            (history_before.clone(), app.head_branch_and_sha())
                        {
                            let summary =
                                format!("commit {} '{}'", after_sha.short(), history_summary_line);
                            app.record_history(
                                kagi::git::OperationKind::Commit,
                                &hbranch,
                                before,
                                after_sha,
                                summary,
                            );
                        }
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: commit failed — {}", err_msg);
                        app.record_op(
                            "commit",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        if let Some(ref mut panel) = app.commit_panel {
                            if let Some(ref mut modal) = panel.plan_modal {
                                modal.error = Some(SharedString::from(err_msg.clone()));
                            }
                        }
                        // Surface commit failures in the status footer too, so the
                        // error is visible even for the smooth (no-popup) commit path
                        // where the plan modal isn't shown.
                        app.status_footer = FooterStatus::Failed(SharedString::from(format!(
                            "commit failed: {}",
                            err_msg
                        )));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Create the 2-parent merge commit for the continued-merge flow (ADR-0068 /
    /// T-CONFLICT-FLOW-031): `execute_merge_commit` (HEAD + MERGE_HEAD parents +
    /// cleanup_state), then drop the resolution buffer, clear the merge-pending /
    /// commit-panel state, oplog, and reload (which clears Conflict Mode).
    fn finish_merge_commit(
        &mut self,
        message: &str,
        plan: &std::sync::Arc<OperationPlan>,
        cx: &mut Context<Self>,
    ) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", e)),
                );
                return;
            }
        };
        match repo.execute_merge_commit(message) {
            Ok(id) => {
                eprintln!("[kagi] executed: merge commit {}", id.short());
                let _ = kagi::git::ResolutionBuffer::clear(&repo_path);
                let branch = self.status_summary.branch.clone();
                let _ = kagi::git::clear_draft(&repo_path, &branch);
                self.last_draft_value = String::new();
                let after = StateSummary {
                    head: format!("branch: {} (merge commit {})", branch, id.short()),
                    dirty: "clean".to_string(),
                };
                self.record_op(
                    "merge-commit",
                    plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                // Leave the merge-commit / commit-panel state and re-detect so
                // Conflict Mode clears (MERGE_HEAD is gone after cleanup_state).
                self.conflict_merge_commit_pending = false;
                self.commit_panel_open = false;
                if let Some(panel) = self.commit_panel.as_mut() {
                    panel.plan_modal = None;
                }
                self.reload();
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                eprintln!("[kagi] merge commit failed: {}", err_msg);
                self.record_op(
                    "merge-commit",
                    plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                if let Some(panel) = self.commit_panel.as_mut() {
                    if let Some(modal) = panel.plan_modal.as_mut() {
                        modal.error = Some(SharedString::from(err_msg));
                    }
                }
            }
        }
        cx.notify();
    }
}
