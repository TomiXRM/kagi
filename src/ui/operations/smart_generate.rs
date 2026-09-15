//! Smart Commit generation dispatch, split out of `commit.rs` (already past its
//! LOC ceiling) on the boundary its completion owns.
//!
//! This is the long-lived half of the commit panel: a local LLM or an agentic
//! CLI can keep one generation in flight for minutes. The completion therefore
//! holds the initiating panel **weakly** and upgrades only after the owner
//! entry proves the tab is still open (#722 P1). A strong `Entity` here kept
//! the `CommitPanelView` — and its `InputState` — alive for every tab the user
//! closed mid-generation, which is exactly the resource lifetime ADR-0197
//! 決定 2 says `release_session` must end.

use crate::ui::*;

impl KagiApp {
    /// Collect the staged diff and dispatch generation on a background thread.
    ///
    /// Sends only the staged diff to loopback Ollama (ureq + global timeout in
    /// the backend).  On any `Err` the result falls back to the rule-based draft
    /// so the UI never blocks or shows a blocking error.
    pub(super) fn run_smart_generation(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        // #476: generate from the PANEL's staged diff, not the tab's.
        let Some(repo_path) = self.commit_panel_repo_path(cx) else {
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
        // ADR-0134: the template mode that used to select Conventional Commits
        // is gone, so generated subjects are plain prose. The panel now always
        // has a body input, so a body is always wanted.
        let style = message_gen::Style::Plain;
        let want_body = true;
        let host = smart_commit::SmartCommitState::ollama_host();
        let provider = self.smart_commit.provider;
        // The initiating panel freezes the session that owns both the spinner
        // and the completion status. A tab switch cannot redirect either.
        let Some(cp_entity) = self.ui().commit_panel.clone() else {
            return;
        };
        let owner = cp_entity.read(cx).owner;
        let gen = cp_entity.update(cx, |v, _| {
            v.gen = v.gen.wrapping_add(1);
            v.gen
        });
        // #722 P1: only this weak handle crosses into the completion, so
        // closing the tab drops the panel immediately even mid-generation.
        let cp_weak = cp_entity.downgrade();
        let Some(ui) = self.ui.get_mut(&owner) else {
            return;
        };
        ui.smart_commit_generating = true;
        ui.smart_commit_status = Some(match provider {
            smart_commit::SmartProvider::Ollama => "Generating with local LLM…".to_string(),
            smart_commit::SmartProvider::Cli(p) => {
                format!("Generating with {}…", p.display_name())
            }
        });
        cx.notify();

        let generation = async move {
            let repo = match crate::ui::blocking_ops::open_backend(&repo_path) {
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
        };
        #[cfg(feature = "gui-e2e")]
        let task = crate::ui::e2e::take_smart_generation()
            .unwrap_or_else(|| cx.background_spawn(generation));
        #[cfg(not(feature = "gui-e2e"))]
        let task = cx.background_spawn(generation);

        cx.spawn(async move |this, acx| {
            let out = task.await;
            let _ = this.update(acx, |app, cx| {
                // A closed owner is gone, not a detached fallback destination.
                // Reopening the same path gets a new SessionId and cannot
                // inherit this completion — so the owner entry decides, and it
                // is consulted *before* the panel is touched at all.
                let Some(ui) = app.ui.get_mut(&owner) else {
                    return;
                };
                ui.smart_commit_generating = false;
                // Upgrade only now: `release_session` dropped the entity when
                // the tab closed, so a completion that lost its panel writes
                // nothing instead of resurrecting it (#722 P1).
                let Some(cp_entity) = cp_weak.upgrade() else {
                    return;
                };
                let stale = cp_entity.read(cx).gen != gen;
                match out {
                    Some((msg, used_llm)) if !msg.trim().is_empty() => {
                        // Drop only a result superseded by a newer generation.
                        if !stale {
                            // The Input's set_value needs `&mut Window`, which is
                            // unavailable here. Mirror into the initiating panel
                            // entity; its next render pushes it into the Input.
                            cp_entity.update(cx, |v, _| {
                                v.state.commit_msg = msg.clone();
                                v.pending_smart_msg = Some(msg.clone());
                            });
                            ui.smart_commit_status = Some(if used_llm {
                                "Generated with local LLM".to_string()
                            } else {
                                "LLM unavailable — used rule-based".to_string()
                            });
                        }
                    }
                    _ => {
                        ui.smart_commit_status =
                            Some("Generation failed — edit manually".to_string());
                    }
                }
                if app.active_session() == Some(owner) {
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
