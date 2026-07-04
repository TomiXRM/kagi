//! Commit panel: staging, smart-commit, template handling, and the commit operation.
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 4, P1) as an additional
//! `impl KagiApp` block. Behaviour and signatures are unchanged; a descendant
//! module can access `KagiApp` privates so no visibility was widened.

#![allow(clippy::too_many_arguments)]
use crate::ui::blocking_ops::*;

use crate::ui::*;

impl KagiApp {
    // ADR-0118 (Phase 5.2) / T-ENTITY-COMMITPANEL-001: the input-sync, template
    // toggle, effective-message, and draft-autosave helpers MOVED ONTO the
    // `CommitPanelView` entity (see `commit_panel.rs`) — they operate on the
    // entity's own inputs / draft state. The parent-side methods below delegate to
    // the entity via `self.commit_panel`.

    /// Effective single-message text for the current mode (template assembled vs
    /// plain Input), read off the entity. `""` when no panel is open.
    pub(crate) fn effective_commit_message(&self, cx: &Context<Self>) -> String {
        match self.commit_panel.as_ref() {
            Some(e) => e.read(cx).effective_commit_message(cx),
            None => String::new(),
        }
    }

    /// Whether the panel is in template authoring mode (entity-owned).
    fn cp_template_mode(&self, cx: &Context<Self>) -> bool {
        self.commit_panel
            .as_ref()
            .map(|e| e.read(cx).commit_template_mode)
            .unwrap_or(false)
    }

    /// The panel's plain commit-message `InputState` (entity-owned), if any.
    fn cp_commit_input(&self, cx: &Context<Self>) -> Option<Entity<InputState>> {
        self.commit_panel
            .as_ref()
            .and_then(|e| e.read(cx).commit_input.clone())
    }

    /// Whether the panel has a plain commit Input (entity-owned).
    fn cp_has_commit_input(&self, cx: &Context<Self>) -> bool {
        self.commit_panel
            .as_ref()
            .map(|e| e.read(cx).commit_input.is_some())
            .unwrap_or(false)
    }

    /// Open the commit panel (triggered by clicking the WIP row).
    ///
    /// Loads the current staging status from the repository.
    /// Clears any existing commit selection so the two views are exclusive.
    pub fn open_commit_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                klog!("open_commit_panel: no repo_path set");
                return;
            }
        };
        // Reopening (the entity survives a commit-row click — `select` clears
        // `commit_panel_open` but NOT the entity, ADR-0118 Q4): REUSE the existing
        // entity so the user's in-memory commit message / template inputs / mode
        // are NOT dropped (the draft autosave is debounced 250ms, so a recreate
        // would lose typing that hasn't hit the draft file yet). Only the staging
        // `state` is refreshed from the repo; `commit_input`/template/mode/draft
        // live on the entity and are preserved. `tree_view` is part of `state`, so
        // carry it across the refresh.
        let (entity, is_new) = if let Some(existing) = self.commit_panel.clone() {
            let prev_tree_view = existing.read(cx).state.tree_view;
            let mut panel = CommitPanelState::from_repo(&repo_path);
            panel.tree_view = prev_tree_view;
            existing.update(cx, |v, _| v.state = panel);
            (existing, false)
        } else {
            // First open: build the entity. `cx.weak_entity()` runs on the parent
            // (this method is always called from a deferred/parent path — never
            // from a leased CommitPanelView listener), so creating it here cannot
            // re-lease a leased panel (correction #6).
            let panel = CommitPanelState::from_repo(&repo_path);
            let weak_app = cx.weak_entity();
            (
                cx.new(|_| CommitPanelView::new(panel, weak_app, repo_path.clone())),
                true,
            )
        };
        self.commit_panel = Some(entity.clone());
        self.commit_panel_open = true;
        self.selected = None;
        self.main_diff = None;

        // T026: lazy-create the InputState (requires &mut Window) inside the
        // entity so it stays STABLE across status reloads (IME/focus).
        entity.update(cx, |v, cx| {
            if v.commit_input.is_none() {
                let st = cx.new(|cx| InputState::new(window, cx).placeholder("Commit message"));
                v.commit_input = Some(st);
            }
        });

        // T-COMMIT-007 / T-COMMIT-009: restore the per-branch draft into an
        // empty input, honouring the persisted mode. A template draft stores its
        // expanded plain text (ADR-0042); on restore we re-parse it back into the
        // structured fields and re-open in template mode.
        //
        // FIRST OPEN ONLY (`is_new`): on REOPEN the reused entity already holds
        // the user's in-memory authoring state (plain text OR template fields +
        // mode). The restore keys off the *plain* input being empty, which is also
        // true in template mode — so running it on reopen would clobber unsaved
        // template fields and flip the mode to plain (R2 finding). The draft was
        // already restored on the first open and is preserved on the entity.
        let input_entity = entity.read(cx).commit_input.clone();
        if is_new {
            if let Some(ref input_entity) = input_entity {
                let current = input_entity.read(cx).value().to_string();
                if current.trim().is_empty() {
                    let branch = self.active_view.status_summary.branch.clone();
                    if let Some(d) = kagi_git::load_draft(&repo_path, &branch) {
                        klog!("draft: loaded {} (mode={})", branch, d.mode);
                        let input = input_entity.clone();
                        if d.mode == "template" {
                            let fields = kagi_git::parse_message(&d.message);
                            entity.update(cx, |v, cx| {
                                v.set_template_inputs(&fields, window, cx);
                                v.commit_template_mode = true;
                                v.last_draft_value = d.message;
                            });
                        } else {
                            input.update(cx, |state, cx| {
                                state.set_value(d.message, window, cx);
                            });
                            let loaded = input.read(cx).value().to_string();
                            entity.update(cx, |v, _| {
                                v.commit_template_mode = false;
                                v.last_draft_value = loaded;
                            });
                        }
                    }
                }
            }
        }

        // T026: focus the InputState after opening the panel.
        if let Some(ref input_entity) = input_entity {
            input_entity.update(cx, |state, cx| {
                state.focus(window, cx);
            });
        }

        // Log for headless verification.
        {
            let v = entity.read(cx);
            eprintln!(
                "[kagi] commit-panel: unstaged={} staged={}",
                v.state.unstaged.len(),
                v.state.staged.len()
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
    ///
    /// `pub(crate)` so other open paths (e.g. the Settings overlay) can ensure a
    /// probe has run before they try to render the model picker.
    pub(crate) fn ensure_smart_commit_detection(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        if self.smart_commit_detected_for.as_deref() == Some(repo_path.as_path()) {
            return;
        }
        self.smart_commit_detected_for = Some(repo_path);

        // CLI availability is just a PATH scan (instant, no spawn, no network),
        // so detect it inline even when offline — "is it installed" is unrelated
        // to the offline gate, which is applied later at generate time (ADR-0099).
        self.smart_commit.claude_available =
            message_gen::cli_available(message_gen::CliProvider::ClaudeCode);
        self.smart_commit.codex_available =
            message_gen::cli_available(message_gen::CliProvider::Codex);
        klog!(
            "smart-commit: cli claude={} codex={}",
            self.smart_commit.claude_available,
            self.smart_commit.codex_available
        );

        if message_gen::offline() {
            klog!("smart-commit: offline (ollama detection skipped)");
            cx.notify();
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
                    "[kagi] smart-commit: ollama_available={} models={} claude={} codex={}",
                    available,
                    app.smart_commit.detected_models.len(),
                    app.smart_commit.claude_available,
                    app.smart_commit.codex_available,
                );
                cx.notify();
            });
        })
        .detach();
    }

    /// Force a fresh Ollama probe by clearing the per-repo run-once guard, then
    /// running [`ensure_smart_commit_detection`].
    ///
    /// Used by the Settings overlay: the model picker is only usable once
    /// `detected_models` is populated, and detection otherwise runs lazily from
    /// the commit panel.  Re-probing also lets a server started *after* the panel
    /// was first opened become visible without a restart.
    pub(crate) fn refresh_smart_commit_detection(&mut self, cx: &mut Context<Self>) {
        self.smart_commit_detected_for = None;
        self.ensure_smart_commit_detection(cx);
    }

    /// Read the current commit-message Input value (UI) or headless `commit_msg`.
    fn smart_commit_current_msg(&self, cx: &Context<Self>) -> String {
        match self.commit_panel.as_ref() {
            Some(e) => {
                let v = e.read(cx);
                if let Some(ref input) = v.commit_input {
                    input.read(cx).value().to_string()
                } else {
                    v.state.commit_msg.clone()
                }
            }
            None => String::new(),
        }
    }

    /// Write `msg` into the commit-message Input (and the headless mirror).
    /// Only overwrites a non-empty existing message after the caller has
    /// decided to (rule-based/LLM both call this to *insert* the draft).
    fn smart_commit_set_msg(&mut self, msg: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entity) = self.commit_panel.clone() {
            let input = entity.read(cx).commit_input.clone();
            if let Some(input) = input {
                input.update(cx, |state, cx| {
                    state.set_value(msg.to_string(), window, cx);
                });
            }
            entity.update(cx, |v, _| v.state.commit_msg = msg.to_string());
        }
    }

    /// "Suggest" button — rule-based draft (always available, never networked).
    ///
    /// Inserts the draft into the message Input.  If the Input already holds a
    /// non-empty message it is left untouched (the user's text wins; ticket:
    /// overwrite only when empty).
    pub fn smart_suggest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(_repo_path) = self.repo_path.clone() else {
            return;
        };
        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => return,
        };
        let files = repo.collect_staged_files();
        let template_mode = self.cp_template_mode(cx);
        // ADR-0090: style follows the mode (template → Conventional, else Plain).
        let style = if template_mode {
            message_gen::Style::ConventionalCommits
        } else {
            message_gen::Style::Plain
        };
        let gi = message_gen::GenInput {
            diff: String::new(),
            lang: self.smart_commit.lang,
            style,
            want_body: template_mode,
        };
        let msg = message_gen::rule_based(&gi, &files);
        if std::env::var("KAGI_SMART_SUGGEST").as_deref() == Ok("1") {
            klog!("smart-suggest: {}", msg);
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
        // Gate 2: model selection — Ollama only. CLI providers (ADR-0099) have no
        // model picker; the provider itself is chosen in Settings.
        if matches!(
            self.smart_commit.provider,
            smart_commit::SmartProvider::Ollama
        ) && self.smart_commit.model.is_none()
        {
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
        klog!("smart-commit: llm enabled (consent given)");
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
        klog!("smart-commit: model selected = {}", model);
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
        let lang = self.smart_commit.lang;
        // Only the Ollama backend needs a model; CLI providers ignore it. For
        // Ollama, bail if no model has been chosen yet (the picker handles that
        // upstream in `smart_generate`).
        let model = match self.smart_commit.provider {
            smart_commit::SmartProvider::Ollama => match self.smart_commit.model.clone() {
                Some(m) => m,
                None => return,
            },
            smart_commit::SmartProvider::Cli(_) => String::new(),
        };
        // ADR-0090: the standalone Style toggle is gone — derive it from the
        // mode. Template mode needs a Conventional subject so it can be parsed
        // into the type/scope/summary fields; plain mode uses a plain subject.
        let template_mode = self.cp_template_mode(cx);
        let style = if template_mode {
            message_gen::Style::ConventionalCommits
        } else {
            message_gen::Style::Plain
        };
        // Template mode wants a body too (its body field would otherwise be empty).
        let want_body = template_mode;
        let host = smart_commit::SmartCommitState::ollama_host();
        let provider = self.smart_commit.provider;
        let overwrite_ok = self.smart_commit_current_msg(cx).trim().is_empty();
        // T-ENTITY-COMMITPANEL-001 (correction #5): bump the entity's generation
        // guard and capture it. A stale result whose `gen` no longer matches the
        // entity's is dropped (tightens the racy `overwrite_ok` check).
        let cp_entity = self.commit_panel.clone();
        let gen = match cp_entity.as_ref() {
            Some(e) => e.update(cx, |v, _| {
                v.gen = v.gen.wrapping_add(1);
                v.gen
            }),
            None => return,
        };

        self.smart_commit.generating = true;
        self.smart_commit.status = Some(match provider {
            smart_commit::SmartProvider::Ollama => "Generating with local LLM…".to_string(),
            smart_commit::SmartProvider::Cli(p) => {
                format!("Generating with {}…", p.display_name())
            }
        });
        cx.notify();

        let task = cx.background_spawn(async move {
            let repo = match kagi_git::Backend::open(&repo_path) {
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
            // The selected provider decides the backend (ADR-0099): loopback
            // Ollama, or shelling out to a local agentic CLI.
            let backend = match provider {
                smart_commit::SmartProvider::Ollama => {
                    message_gen::MessageBackend::Ollama { host, model }
                }
                smart_commit::SmartProvider::Cli(provider) => {
                    message_gen::MessageBackend::Cli { provider }
                }
            };
            let (msg, used_llm) = match message_gen::generate_message(&backend, &gi, &files) {
                Ok(m) => (m, true),
                Err(e) => {
                    klog!("smart-commit: llm failed ({}) → rule-based", e);
                    (message_gen::rule_based(&gi, &files), false)
                }
            };
            Some((msg, used_llm))
        });

        cx.spawn(async move |this, acx| {
            let out = task.await;
            let _ = this.update(acx, |app, cx| {
                app.smart_commit.generating = false;
                // The panel may have been dropped (tab switch / reload) while the
                // generation ran. Bail (status update is moot without a panel).
                let Some(entity) = cp_entity.clone() else {
                    cx.notify();
                    return;
                };
                match out {
                    Some((msg, used_llm)) if !msg.trim().is_empty() => {
                        // Correction #5: drop the result if a newer generate
                        // superseded this one (bumped `gen`), OR if the message is
                        // no longer empty — i.e. the user typed something AFTER
                        // requesting generation (`overwrite_ok` was captured at
                        // start, so it alone can't catch type-during). Re-checking
                        // emptiness at apply time prevents a stale generated message
                        // from clobbering the user's newer input.
                        let stale = entity.read(cx).gen != gen;
                        let still_empty = app.smart_commit_current_msg(cx).trim().is_empty();
                        if overwrite_ok && still_empty && !stale {
                            // The Input's set_value needs `&mut Window`, which is
                            // unavailable here. Mirror into the panel state and
                            // queue the message on the entity; the next render
                            // (which has a Window) pushes it into the Input.
                            entity.update(cx, |v, _| {
                                v.state.commit_msg = msg.clone();
                                v.pending_smart_msg = Some(msg.clone());
                            });
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
    pub fn do_stage_all(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let paths: Vec<std::path::PathBuf> = match self.commit_panel.as_ref() {
            Some(e) => {
                let p = &e.read(cx).state;
                p.unstaged
                    .iter()
                    .filter(|f| !p.is_conflicted(&f.path))
                    .map(|f| f.path.clone())
                    .collect()
            }
            None => return,
        };
        if paths.is_empty() {
            return;
        }
        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => return,
        };
        match repo.stage_files(&paths) {
            Ok(n) => {
                klog!("staged-all: {} file(s)", n);
                if let Some(entity) = self.commit_panel.clone() {
                    entity.update(cx, |v, _| v.state.reload_status(&repo_path));
                }
                self.refresh_wip_diffstat();
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("stage all failed: {}", e)));
            }
        }
    }

    /// Unstage every staged file (T-UI-002: Unstage all).
    pub fn do_unstage_all(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let paths: Vec<std::path::PathBuf> = match self.commit_panel.as_ref() {
            Some(e) => e
                .read(cx)
                .state
                .staged
                .iter()
                .map(|f| f.path.clone())
                .collect(),
            None => return,
        };
        if paths.is_empty() {
            return;
        }
        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => return,
        };
        match repo.unstage_files(&paths) {
            Ok(n) => {
                klog!("unstaged-all: {} file(s)", n);
                if let Some(entity) = self.commit_panel.clone() {
                    entity.update(cx, |v, _| v.state.reload_status(&repo_path));
                }
                self.refresh_wip_diffstat();
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("unstage all failed: {}", e)));
            }
        }
    }

    pub fn do_stage_file(&mut self, index: usize, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let path = match self
            .commit_panel
            .as_ref()
            .and_then(|e| e.read(cx).state.unstaged.get(index).map(|f| f.path.clone()))
        {
            Some(p) => p,
            None => return,
        };
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                klog!("stage_file: repo open error: {}", "session unavailable");
                return;
            }
        };
        if let Err(e) = repo.stage_file(&path) {
            klog!("stage_file error: {}", e);
        } else {
            klog!("staged: {}", path.display());
        }
        if let Some(entity) = self.commit_panel.clone() {
            entity.update(cx, |v, _| {
                v.state.reload_status(&repo_path);
                eprintln!(
                    "[kagi] commit-panel: unstaged={} staged={}",
                    v.state.unstaged.len(),
                    v.state.staged.len()
                );
            });
        }
        self.refresh_wip_diffstat();
    }

    /// Unstage a single file in the commit panel.
    ///
    /// Calls `unstage_file` from T024 and then refreshes the staging status.
    pub fn do_unstage_file(&mut self, index: usize, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let path = match self
            .commit_panel
            .as_ref()
            .and_then(|e| e.read(cx).state.staged.get(index).map(|f| f.path.clone()))
        {
            Some(p) => p,
            None => return,
        };
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                klog!("unstage_file: repo open error: {}", "session unavailable");
                return;
            }
        };
        if let Err(e) = repo.unstage_file(&path) {
            klog!("unstage_file error: {}", e);
        } else {
            klog!("unstaged: {}", path.display());
        }
        if let Some(entity) = self.commit_panel.clone() {
            entity.update(cx, |v, _| {
                v.state.reload_status(&repo_path);
                eprintln!(
                    "[kagi] commit-panel: unstaged={} staged={}",
                    v.state.unstaged.len(),
                    v.state.staged.len()
                );
            });
        }
        self.refresh_wip_diffstat();
    }

    /// T-UI-003: Select a file in the commit panel and open it in the main diff pane.
    pub fn select_commit_panel_file(
        &mut self,
        file_ref: CommitPanelFileRef,
        cx: &mut Context<Self>,
    ) {
        self.open_main_diff_wip(file_ref, cx);
    }

    /// Handle a key-down event for the commit message input.
    ///
    /// Uses the T014 simple pattern: printable chars appended, backspace removes last.
    #[allow(dead_code)]
    pub fn handle_commit_msg_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(entity) = self.commit_panel.clone() else {
            return;
        };
        let key = &event.keystroke.key;
        let modifiers = &event.keystroke.modifiers;

        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }

        entity.update(cx, |v, _| {
            if key == "backspace" {
                v.state.commit_msg.pop();
            } else if key == "space" {
                v.state.commit_msg.push(' ');
            } else if key.len() == 1 {
                let ch = key.chars().next().unwrap();
                if !ch.is_control() {
                    v.state.commit_msg.push(ch);
                }
            }
        });
    }

    /// Open the commit plan modal for the current staged files and message.
    ///
    /// Uses `plan_commit` from T024.
    /// T026: reads message from InputState if available, else falls back to commit_panel.commit_msg
    /// (used by the headless KAGI_COMMIT_MSG path).
    pub fn open_commit_plan_modal(&mut self, cx: &mut Context<Self>) {
        let _repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // T026 / T-COMMIT-009: prefer the effective message (assembled template
        // in template mode, else the plain Input); fall back to commit_msg
        // (headless path).
        let msg: String = if self.cp_has_commit_input(cx) || self.cp_template_mode(cx) {
            self.effective_commit_message(cx)
        } else {
            match self.commit_panel.as_ref() {
                Some(e) => e.read(cx).state.commit_msg.clone(),
                None => return,
            }
        };
        if msg.trim().is_empty() {
            return;
        }
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                klog!("plan_commit: repo open error: {}", "session unavailable");
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
                if let Some(entity) = self.commit_panel.clone() {
                    entity.update(cx, |v, _| {
                        v.state.plan_modal = Some(CommitPlanModal {
                            plan: std::sync::Arc::new(plan),
                            error: None,
                        });
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
                    if let Some(entity) = self.commit_panel.clone() {
                        entity.update(cx, |v, _| v.state.plan_modal = None);
                    }
                }
            }
            Err(e) => {
                klog!("plan_commit error: {}", e);
            }
        }
    }

    /// Cancel the commit plan modal.
    pub fn cancel_commit_plan_modal(&mut self, cx: &mut Context<Self>) {
        if let Some(entity) = self.commit_panel.clone() {
            entity.update(cx, |v, _| v.state.plan_modal = None);
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
        let commit_message: String = if self.cp_has_commit_input(cx) || self.cp_template_mode(cx) {
            self.effective_commit_message(cx)
        } else {
            self.commit_panel
                .as_ref()
                .map(|e| e.read(cx).state.commit_msg.clone())
                .unwrap_or_default()
        };
        let plan = match self
            .commit_panel
            .as_ref()
            .and_then(|e| e.read(cx).state.plan_modal.as_ref().map(|m| m.plan.clone()))
        {
            Some(plan) => plan,
            None => return,
        };
        if !plan.blockers.is_empty() {
            klog!("refused: commit plan has blockers");
            return;
        }

        // ADR-0068 (T-CONFLICT-FLOW-031): a merge that was continued routes the
        // commit button here with MERGE_HEAD still present.  Create the 2-parent
        // merge commit (HEAD + MERGE_HEAD) + cleanup_state instead of a plain
        // single-parent commit.  This is synchronous (cheap; no tree rebuild on a
        // worker) so the conflict-mode transition stays simple.
        if self.conflict_merge_pending {
            self.finish_merge_commit(&commit_message, &plan, cx);
            return;
        }

        self.busy_op = Some("commit");
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyCommit.t()));
        klog!("async: commit started");

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
        self.finish_op_on_main(cx, task, move |app, result, cx| match result {
            Ok((_new_short, after)) => {
                klog!("async: commit finished");
                // A successful commit clears the branch draft (T-COMMIT-007).
                let branch = app.active_view.status_summary.branch.clone();
                let _ = kagi_git::clear_draft(&repo_path, &branch);
                klog!("draft: cleared {}", branch);
                if let Some(entity) = app.commit_panel.clone() {
                    entity.update(cx, |v, _| v.last_draft_value = String::new());
                }

                app.record_op(
                    "commit",
                    plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                    cx,
                );
                if let (Some((hbranch, before)), Some((_, after_sha))) =
                    (history_before.clone(), app.head_branch_and_sha())
                {
                    let summary =
                        format!("commit {} '{}'", after_sha.short(), history_summary_line);
                    app.record_history(
                        kagi_git::OperationKind::Commit,
                        &hbranch,
                        before,
                        after_sha,
                        summary,
                    );
                }
                app.reload(cx);
            }
            Err(err_msg) => {
                klog!("async: commit failed — {}", err_msg);
                app.record_op(
                    "commit",
                    plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                if let Some(entity) = app.commit_panel.clone() {
                    entity.update(cx, |v, _| {
                        if let Some(ref mut modal) = v.state.plan_modal {
                            modal.error = Some(SharedString::from(err_msg.clone()));
                        }
                    });
                }
                // Surface commit failures in the status footer too, so the
                // error is visible even for the smooth (no-popup) commit path
                // where the plan modal isn't shown.
                app.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("commit failed: {}", err_msg)));
            }
        });
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
        let mut repo = match kagi_git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", e)),
                    cx,
                );
                return;
            }
        };
        // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
        // MergeCommit has no user-facing plan modal (the conflict-resolution
        // save IS the confirm step); we synthesize a plan via plan_merge_commit
        // so run()'s preflight can gate on HEAD movement.
        let merge_op = kagi_git::Operation::MergeCommit {
            message: message.to_string(),
        };
        let merge_plan = match repo.plan(&merge_op) {
            Ok(p) => p,
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Merge-commit plan failed: {}", e)),
                    cx,
                );
                return;
            }
        };
        match repo.run(&merge_op, &merge_plan) {
            Ok(kagi_git::OperationOutcome::Commit(id)) => {
                klog!("executed: merge commit {}", id.short());
                let _ = kagi_git::ResolutionBuffer::clear(&repo_path);
                let branch = self.active_view.status_summary.branch.clone();
                let _ = kagi_git::clear_draft(&repo_path, &branch);
                if let Some(entity) = self.commit_panel.clone() {
                    entity.update(cx, |v, _| v.last_draft_value = String::new());
                }
                let after = StateSummary {
                    head: format!("branch: {} (merge commit {})", branch, id.short()),
                    dirty: "clean".to_string(),
                };
                self.record_op(
                    "merge-commit",
                    plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                    cx,
                );
                // Leave the merge-commit / commit-panel state and re-detect so
                // Conflict Mode clears (MERGE_HEAD is gone after cleanup_state).
                self.conflict_merge_pending = false;
                self.commit_panel_open = false;
                if let Some(entity) = self.commit_panel.clone() {
                    entity.update(cx, |v, _| v.state.plan_modal = None);
                }
                self.reload(cx);
            }
            Ok(_) => {
                // MergeCommit only yields OperationOutcome::Commit; any other
                // variant is a backend bug — surface it loudly.
                klog!("merge commit: unexpected outcome variant");
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from("merge commit: unexpected outcome"),
                    cx,
                );
                return;
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                klog!("merge commit failed: {}", err_msg);
                self.record_op(
                    "merge-commit",
                    plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                if let Some(entity) = self.commit_panel.clone() {
                    entity.update(cx, |v, _| {
                        if let Some(modal) = v.state.plan_modal.as_mut() {
                            modal.error = Some(SharedString::from(err_msg));
                        }
                    });
                }
            }
        }
        cx.notify();
    }

    /// ADR-0118: parent-side handler for the Commit Panel "Amend" control,
    /// deferred from the (leased) `CommitPanelView` listener. Reads the entity's
    /// staged/message state to pick the [`AmendMode`], then opens the amend modal
    /// (which reads `commit_panel` again — safe here on the parent).
    pub fn commit_panel_amend(&mut self, cx: &mut Context<Self>) {
        let staged = self
            .commit_panel
            .as_ref()
            .map(|e| !e.read(cx).state.staged.is_empty())
            .unwrap_or(false);
        let msg = self
            .cp_commit_input(cx)
            .map(|i| !i.read(cx).value().trim().is_empty())
            .unwrap_or(false);
        let mode = match (msg, staged) {
            (true, true) => AmendMode::Both,
            (false, true) => AmendMode::Staged,
            (true, false) => AmendMode::MessageOnly,
            (false, false) => {
                self.status_footer =
                    FooterStatus::Idle(SharedString::from(Msg::AmendNeedMessageOrStaged.t()));
                cx.notify();
                return;
            }
        };
        self.open_amend_modal(mode, cx);
        cx.notify();
    }
}

// `dispatch_commit_action`, moved from `src/ui/mod.rs` (T-HOTSPOT-UIMOD-001).
// Behaviour-preserving relocation.
impl KagiApp {
    pub fn dispatch_commit_action(
        &mut self,
        action: CommitAction,
        target: CommitId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            CommitAction::ShowDetails => {
                if let Some(row_index) = self.row_for_commit_id(&target) {
                    if self.selected != Some(row_index) {
                        self.select(row_index);
                    }
                }
            }
            CommitAction::CopySha => {
                if let Some(row_index) = self.row_for_commit_id(&target) {
                    if let Some(detail) = self.active_view.details.get(row_index) {
                        let full_sha = detail.full_sha.as_ref().to_string();
                        let short: String = full_sha.chars().take(8).collect();
                        context_menu::copy_full_sha(self, full_sha, cx);
                        // W18-COAUTHOR-COPY: surface a toast so the copy is
                        // visible regardless of where it was triggered
                        // (hash chip click or the "Copy SHA" action button).
                        self.push_toast(ToastKind::Info, format!("Copied {}", short), cx);
                    }
                }
            }
            CommitAction::CopyShortSha => {
                if let Some(row_index) = self.row_for_commit_id(&target) {
                    if let Some(detail) = self.active_view.details.get(row_index) {
                        let full_sha = detail.full_sha.as_ref().to_string();
                        context_menu::copy_short_sha(self, &full_sha, cx);
                    }
                }
            }
            CommitAction::CopyMessage => {
                if let Some(row_index) = self.row_for_commit_id(&target) {
                    if let Some(detail) = self.active_view.details.get(row_index) {
                        let full_sha = detail.full_sha.as_ref().to_string();
                        context_menu::copy_message(
                            self,
                            &full_sha,
                            detail.full_message.as_ref().to_string(),
                            cx,
                        );
                    }
                }
            }
            CommitAction::CheckoutCommit => {
                self.open_checkout_commit_modal(target);
            }
            CommitAction::CheckoutRef(ref_name) => {
                if ref_name.is_empty() {
                    self.status_footer =
                        FooterStatus::Idle(SharedString::from("Checkout ref unavailable"));
                    eprintln!(
                        "[kagi] context-menu: checkout-ref unavailable {}",
                        target.short()
                    );
                } else {
                    self.open_plan_modal(ref_name);
                }
            }
            CommitAction::CheckoutTrackingBranch(remote_name) => {
                // Remote-only branch: create a local tracking branch + checkout
                // (same flow as the sidebar remote-branch menu).
                self.open_tracking_checkout_modal(remote_name);
            }
            CommitAction::CreateBranchHere => {
                self.open_create_branch_modal(target, cx);
                eprintln!(
                    "[kagi] context-menu: create-branch {}",
                    self.create_branch_modal()
                        .map(|m| m.at.short())
                        .unwrap_or_default()
                );
            }
            CommitAction::CreateWorktreeHere => {
                self.open_create_worktree_modal(target, cx);
                eprintln!(
                    "[kagi] context-menu: create-worktree {}",
                    self.create_worktree_modal()
                        .map(|m| m.at.short())
                        .unwrap_or_default()
                );
            }
            CommitAction::CherryPick => {
                self.open_cherry_pick_modal(target);
            }
            CommitAction::Revert => {
                self.open_revert_modal(target);
            }
            // ADR-0024: reset stays unimplemented; the menu item is disabled,
            // this arm is defence in depth.
            CommitAction::ResetToCommit => {
                self.status_footer =
                    FooterStatus::Idle(SharedString::from(Msg::ResetUnimplemented.t()));
                klog!("context-menu: stub Reset {}", target.short());
            }
            CommitAction::CompareWithHead => {
                self.open_compare_with_head(target);
            }
            CommitAction::CompareWithWorkingTree => {
                self.open_compare_with_working_tree(target);
            }
            CommitAction::ShowChangedFiles => {
                self.show_changed_files_for_commit(target);
            }
        }
    }
}
