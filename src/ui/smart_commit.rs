//! Smart Commit Message — UI-side state, settings & detection (T-COMMIT-016, ADR-0044)
//!
//! This is the *UI* half of the Smart Commit Message feature.  The generation
//! backend lives in `src/git/message_gen.rs`; everything here is the small
//! amount of state, settings persistence, and Ollama-detection glue the commit
//! panel needs.  Rendering itself (buttons, consent dialog, model picker) lives
//! in `mod.rs` so it can reuse the existing modal / theme machinery.
//!
//! ## Policy (ADR-0044, user-decided)
//!
//!   * **Rule-based suggestion is always available** (the "Suggest" button).
//!   * **Local-LLM generation is opt-in**: it is usable only when an Ollama
//!     server is detected *and* the user has explicitly enabled it.
//!   * Pressing **"Generate with Local LLM"** is the *only* moment the staged
//!     diff is sent, and only to loopback Ollama.
//!   * On **first** enable a consent dialog is shown carrying the four mandated
//!     statements (see [`CONSENT_LINES`]).
//!   * Model selection: one model still needs first-time confirmation; multiple
//!     models force a choice.  The chosen model is persisted to `settings.json`.
//!   * `KAGI_OFFLINE=1` disables detection and generation entirely.

use kagi::git::message_gen::{self, CliProvider, Lang};

use super::settings;

// ──────────────────────────────────────────────────────────────────────────
// settings.json keys (string-valued; see settings::write_setting)
// ──────────────────────────────────────────────────────────────────────────

const KEY_ENABLED: &str = "smart_commit_llm_enabled";
const KEY_MODEL: &str = "smart_commit_model";
const KEY_LANG: &str = "smart_commit_lang";
/// Which generation backend is selected: `"ollama"` (default) | `"claude-code"`
/// | `"codex"` (ADR-0099).
const KEY_PROVIDER: &str = "smart_commit_provider";

// ──────────────────────────────────────────────────────────────────────────
// Provider selection (ADR-0099)
// ──────────────────────────────────────────────────────────────────────────

/// The selected Smart Commit message-generation provider.
///
/// `Ollama` is the local, privacy-preserving default. The `Cli(..)` variants
/// shell out to a locally installed coding-agent CLI (Claude Code / Codex):
/// stronger models reusing the user's own auth, at the cost that the staged diff
/// leaves kagi's local sandbox — hence the prominent Settings warning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmartProvider {
    /// Local loopback Ollama (default).
    Ollama,
    /// A locally installed agentic CLI (Claude Code / Codex).
    Cli(CliProvider),
}

impl SmartProvider {
    /// Stable slug for `settings.json` (`"ollama"` / `"claude-code"` / `"codex"`).
    pub fn slug(self) -> &'static str {
        match self {
            SmartProvider::Ollama => "ollama",
            SmartProvider::Cli(p) => p.slug(),
        }
    }

    /// Parse a slug, defaulting to `Ollama` for anything unrecognised.
    pub fn from_slug(s: &str) -> SmartProvider {
        match CliProvider::from_slug(s) {
            Some(p) => SmartProvider::Cli(p),
            None => SmartProvider::Ollama,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Consent dialog text (ADR-0044 — these four lines MUST be present)
// ──────────────────────────────────────────────────────────────────────────

/// The four statements the first-time consent dialog must show verbatim.
pub const CONSENT_LINES: [&str; 4] = [
    "Only staged diff will be sent",
    "Unstaged changes will not be included",
    "The request stays on localhost Ollama",
    "Secrets may still exist in staged diff; review before generating",
];

// ──────────────────────────────────────────────────────────────────────────
// State
// ──────────────────────────────────────────────────────────────────────────

/// Which modal (if any) the Smart Commit flow is currently showing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SmartCommitModal {
    /// First-time consent (carries [`CONSENT_LINES`]).  On confirm, enables LLM
    /// generation and proceeds to model selection (or generation).
    Consent,
    /// Model picker: the user must choose one of `models`.
    ModelPicker {
        /// Installed model names from `/api/tags`.
        models: Vec<String>,
    },
}

/// All Smart-Commit UI state held on `KagiApp`.
#[derive(Clone, Debug)]
pub struct SmartCommitState {
    /// Whether an Ollama server was detected at startup (`Local LLM available`).
    pub ollama_available: bool,
    /// Models installed on the detected Ollama server (`/api/tags`).
    pub detected_models: Vec<String>,
    /// Whether the Claude Code CLI (`claude`) was found on `$PATH` (ADR-0099).
    pub claude_available: bool,
    /// Whether the Codex CLI (`codex`) was found on `$PATH` (ADR-0099).
    pub codex_available: bool,
    /// Selected generation backend (persisted; ADR-0099). Default `Ollama`.
    pub provider: SmartProvider,
    /// User has opted in to LLM generation (persisted).
    pub llm_enabled: bool,
    /// Selected model name (persisted); `None` until chosen.
    pub model: Option<String>,
    /// Output language (persisted; remembered like the draft per ADR-0042).
    pub lang: Lang,
    /// Active modal, if any.
    pub modal: Option<SmartCommitModal>,
    /// True while a background generation is in flight (button shows "…").
    pub generating: bool,
    /// Transient status line under the buttons (toast-level, not a blocker).
    pub status: Option<String>,
}

impl Default for SmartCommitState {
    fn default() -> Self {
        SmartCommitState {
            ollama_available: false,
            detected_models: Vec::new(),
            claude_available: false,
            codex_available: false,
            provider: SmartProvider::Ollama,
            llm_enabled: false,
            model: None,
            lang: Lang::En,
            modal: None,
            generating: false,
            status: None,
        }
    }
}

impl SmartCommitState {
    /// Load persisted settings (enabled / model / lang / style).  Detection of
    /// the running Ollama server is done separately in the background so the UI
    /// thread never blocks on a probe.
    pub fn load() -> Self {
        SmartCommitState {
            llm_enabled: settings::read_setting(KEY_ENABLED).as_deref() == Some("1"),
            model: settings::read_setting(KEY_MODEL).filter(|m| !m.is_empty()),
            lang: settings::read_setting(KEY_LANG)
                .map(|l| Lang::from_slug(&l))
                .unwrap_or(Lang::En),
            provider: settings::read_setting(KEY_PROVIDER)
                .map(|p| SmartProvider::from_slug(&p))
                .unwrap_or(SmartProvider::Ollama),
            ..Default::default()
        }
    }

    /// Persist the opt-in flag.
    pub fn set_enabled(&mut self, on: bool) {
        self.llm_enabled = on;
        settings::write_setting(KEY_ENABLED, Some(if on { "1" } else { "0" }));
    }

    /// Persist the selected model.
    pub fn set_model(&mut self, model: impl Into<String>) {
        let model = model.into();
        settings::write_setting(KEY_MODEL, Some(&model));
        self.model = Some(model);
    }

    /// Persist the selected generation provider (ADR-0099).
    pub fn set_provider(&mut self, provider: SmartProvider) {
        self.provider = provider;
        settings::write_setting(KEY_PROVIDER, Some(provider.slug()));
    }

    /// Whether `provider`'s CLI was detected on `$PATH` (from the last probe).
    pub fn cli_available_for(&self, provider: CliProvider) -> bool {
        match provider {
            CliProvider::ClaudeCode => self.claude_available,
            CliProvider::Codex => self.codex_available,
        }
    }

    /// Toggle language and persist.
    pub fn toggle_lang(&mut self) {
        self.lang = match self.lang {
            Lang::En => Lang::Ja,
            Lang::Ja => Lang::En,
        };
        settings::write_setting(KEY_LANG, Some(self.lang.slug()));
    }

    /// Whether the "Generate with LLM" button should be *offered* at all.
    ///
    /// Always requires the opt-in (`llm_enabled`) and not-offline. The backend
    /// requirement depends on the selected provider (ADR-0099):
    /// * `Ollama` → a loopback Ollama server must be detected.
    /// * `Cli(p)` → that provider's CLI must be found on `$PATH`.
    pub fn llm_offered(&self) -> bool {
        if !self.llm_enabled || message_gen::offline() {
            return false;
        }
        match self.provider {
            SmartProvider::Ollama => self.ollama_available,
            SmartProvider::Cli(p) => self.cli_available_for(p),
        }
    }

    /// The Ollama host (`host:port`).  Overridable via `KAGI_OLLAMA_HOST` for
    /// tests / non-default ports; defaults to `localhost:11434` (loopback only).
    pub fn ollama_host() -> String {
        std::env::var("KAGI_OLLAMA_HOST")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| message_gen::DEFAULT_OLLAMA_HOST.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_lines_match_adr() {
        // The four ADR-0044 statements must be present verbatim.
        assert_eq!(CONSENT_LINES.len(), 4);
        assert!(CONSENT_LINES.contains(&"Only staged diff will be sent"));
        assert!(CONSENT_LINES.contains(&"Unstaged changes will not be included"));
        assert!(CONSENT_LINES.contains(&"The request stays on localhost Ollama"));
        assert!(CONSENT_LINES
            .contains(&"Secrets may still exist in staged diff; review before generating"));
    }

    #[test]
    fn default_state_is_disabled_rule_based() {
        let s = SmartCommitState::default();
        assert!(!s.llm_enabled);
        assert!(!s.ollama_available);
        assert!(s.model.is_none());
        assert!(!s.llm_offered());
        assert_eq!(s.lang, Lang::En);
    }

    #[test]
    fn provider_slug_roundtrip_defaults_ollama() {
        assert_eq!(SmartProvider::from_slug("ollama"), SmartProvider::Ollama);
        assert_eq!(
            SmartProvider::from_slug("claude-code"),
            SmartProvider::Cli(CliProvider::ClaudeCode)
        );
        assert_eq!(
            SmartProvider::from_slug("codex"),
            SmartProvider::Cli(CliProvider::Codex)
        );
        // Unknown → Ollama (safe local default).
        assert_eq!(SmartProvider::from_slug("garbage"), SmartProvider::Ollama);
        assert_eq!(SmartProvider::Ollama.slug(), "ollama");
        assert_eq!(SmartProvider::Cli(CliProvider::Codex).slug(), "codex");
    }

    #[test]
    fn llm_offered_requires_selected_backend() {
        let mut s = SmartCommitState {
            llm_enabled: true,
            ..Default::default()
        };
        // Ollama selected but not detected → not offered.
        assert!(!s.llm_offered());
        s.ollama_available = true;
        assert!(s.llm_offered());

        // Switch to a CLI provider: ollama presence no longer matters; the CLI
        // must be detected instead.
        s.provider = SmartProvider::Cli(CliProvider::ClaudeCode);
        assert!(!s.llm_offered());
        s.claude_available = true;
        assert!(s.llm_offered());
    }

    #[test]
    fn toggle_lang_flips() {
        let mut s = SmartCommitState::default();
        // Avoid touching real settings.json in CI: set a throwaway dir.
        std::env::set_var("KAGI_LOG_DIR", std::env::temp_dir().join("kagi-sc-test"));
        let _ = std::fs::create_dir_all(std::env::temp_dir().join("kagi-sc-test"));
        s.toggle_lang();
        assert_eq!(s.lang, Lang::Ja);
    }
}
