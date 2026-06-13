//! W22-I18N / ADR-0048: UI-language startup-resolution integration tests.
//!
//! The `i18n` module lives in the **binary** crate (`src/ui/i18n.rs`), so its
//! `Msg::t` / `set_lang` / `resolve_lang` unit tests run inside that module
//! (`cargo test --bin kagi`).  This integration file proves the *observable*
//! end-to-end behaviour that a binary-crate unit test cannot: the startup
//! language-resolution priority (`KAGI_LANG` env → `LANG`/`LC_ALL` → English),
//! which the binary logs as `[kagi] lang: <slug>` on stderr before opening the
//! window.
//!
//! The process is launched with an invalid repo path (so it reaches the
//! resolution + log line quickly) and is killed as soon as the line is seen —
//! we never wait on the GUI event loop.  No fixtures touch any real repo.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Spawn the kagi binary with the given `KAGI_LANG` / `LANG` environment and
/// return the resolved language slug parsed from the `[kagi] lang: <slug>`
/// stderr line.  Kills the process once the line is observed.
fn resolved_lang(kagi_lang: Option<&str>, lang_env: Option<&str>) -> String {
    let exe = env!("CARGO_BIN_EXE_kagi");
    let mut cmd = Command::new(exe);
    // An invalid path makes main() short-circuit to with_error(); the language
    // is still resolved + logged first.
    cmd.arg("/kagi-i18n-test-nonexistent-path");
    cmd.env_remove("KAGI_LANG");
    cmd.env_remove("LANG");
    cmd.env_remove("LC_ALL");
    // Keep settings.json out of the picture: point KAGI_LOG_DIR at an empty
    // temp dir so no persisted "lang" key interferes with env resolution.
    let tmp = std::env::temp_dir().join(format!("kagi-i18n-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    cmd.env("KAGI_LOG_DIR", &tmp);
    if let Some(v) = kagi_lang {
        cmd.env("KAGI_LANG", v);
    }
    if let Some(v) = lang_env {
        cmd.env("LANG", v);
    }
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn kagi binary");
    let stderr = child.stderr.take().expect("capture stderr");
    let mut reader = BufReader::new(stderr);

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut slug = None;
    let mut line = String::new();
    while Instant::now() < deadline {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF (process exited)
            Ok(_) => {
                if let Some(rest) = line.trim_end().strip_prefix("[kagi] lang: ") {
                    slug = Some(rest.to_string());
                    break;
                }
            }
            Err(_) => break,
        }
    }
    // We have what we need (or timed out) — stop the GUI process.
    let _ = child.kill();
    let _ = child.wait();

    slug.unwrap_or_else(|| panic!("did not observe '[kagi] lang:' line in stderr"))
}

#[test]
fn kagi_lang_env_forces_japanese() {
    assert_eq!(resolved_lang(Some("ja"), None), "ja");
}

#[test]
fn kagi_lang_env_forces_english() {
    // KAGI_LANG=en wins even when LANG would otherwise select Japanese.
    assert_eq!(resolved_lang(Some("en"), Some("ja_JP.UTF-8")), "en");
}

#[test]
fn lang_locale_selects_japanese_when_no_override() {
    // No KAGI_LANG → LANG starting with "ja" resolves to Japanese.
    assert_eq!(resolved_lang(None, Some("ja_JP.UTF-8")), "ja");
}

#[test]
fn defaults_to_english() {
    // No KAGI_LANG, non-ja LANG → English default.
    assert_eq!(resolved_lang(None, Some("en_US.UTF-8")), "en");
}
