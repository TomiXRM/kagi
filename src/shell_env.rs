//! Login-shell PATH for GUI launches.
//!
//! A `.app` started from Finder / the Dock inherits launchd's minimal PATH
//! (`/usr/bin:/bin:/usr/sbin:/sbin`), not the user's shell PATH — so every
//! tool kagi shells out to that lives in Homebrew / mise / ~/.local/bin
//! (`gh`, `code`, a configured mergetool) is "not found" even though it works
//! from the terminal. User report: the released app showed no pull requests
//! because `gh` was invisible.
//!
//! Fix once, at startup: ask the login shell for its PATH and merge it into
//! ours. Same approach VS Code / Zed take ("shell environment resolution").
//! Only runs when the current PATH looks like the launchd one; a terminal
//! launch already has the right PATH and skips the ~30ms shell spawn.

use std::process::Command;

const MARK: &str = "__KAGI_PATH__";

/// Merge the login shell's PATH into this process's PATH (idempotent).
pub fn ensure_login_shell_path() {
    let current = std::env::var("PATH").unwrap_or_default();
    if !looks_like_launchd_path(&current) {
        return;
    }
    let Some(shell_path) = login_shell_path() else {
        return;
    };
    let merged = merge_paths(&current, &shell_path);
    if merged != current {
        // Edition 2021: set_var is safe; we are single-threaded this early
        // (called before the runtime / any worker threads start).
        std::env::set_var("PATH", &merged);
        klog!(
            "path: merged login-shell PATH ({} entries)",
            merged.split(':').count()
        );
    }
}

/// True when PATH has none of the usual user tool prefixes — the launchd
/// default, or something equally bare.
fn looks_like_launchd_path(path: &str) -> bool {
    !path.split(':').any(|p| {
        p.contains("/homebrew/")
            || p == "/usr/local/bin"
            || p.contains("/.local/")
            || p.contains("/mise/")
    })
}

/// `$SHELL -lc 'printf MARK%sMARK "$PATH"'` — login (profile) shell, non-
/// interactive: fast (~30ms) and profile is where Homebrew's shellenv and
/// PATH exports conventionally live. Markers strip any rc-file chatter.
fn login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let out = Command::new(&shell)
        .args(["-lc", &format!("printf '{m}%s{m}' \"$PATH\"", m = MARK)])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let start = text.find(MARK)? + MARK.len();
    let end = text[start..].find(MARK)? + start;
    let p = text[start..end].trim();
    (!p.is_empty()).then(|| p.to_string())
}

/// `shell` entries first (they are the ones the user expects to win), then
/// whatever `current` had that the shell lacked; de-duplicated, order kept.
fn merge_paths(current: &str, shell: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for p in shell.split(':').chain(current.split(':')) {
        if !p.is_empty() && !out.contains(&p) {
            out.push(p);
        }
    }
    out.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launchd_default_is_detected_but_a_shell_path_is_not() {
        assert!(looks_like_launchd_path("/usr/bin:/bin:/usr/sbin:/sbin"));
        assert!(!looks_like_launchd_path("/opt/homebrew/bin:/usr/bin:/bin"));
        assert!(!looks_like_launchd_path("/usr/local/bin:/usr/bin"));
    }

    #[test]
    fn merge_prefers_shell_order_and_dedups() {
        assert_eq!(
            merge_paths("/usr/bin:/bin", "/opt/homebrew/bin:/usr/bin"),
            "/opt/homebrew/bin:/usr/bin:/bin"
        );
    }
}
