//! Typed post-create / pre-remove worktree steps + trust (issue #341, ADR-0161).
//!
//! Three concerns live here, all in the git layer because kagi-domain must stay
//! dependency-free:
//!
//! 1. **Parse** `.kagi/worktree.toml` (`toml`) into the pure
//!    [`kagi_domain::worktree_steps::WorktreeSteps`] model, plus the SHA-256
//!    (`sha2`/`hex`) of its raw bytes and its canonical path — the trust key.
//! 2. **Trust store** (`trusted_worktree_configs`) — a repo-level allow-list
//!    keyed by `(canonical config path, SHA-256)`. A committed config is
//!    **untrusted by default** (the gwq v0.1.0 ACE lesson); changing its
//!    content changes the SHA and forces a re-confirm.
//! 3. **Executor** — `copy`/`symlink` are self-implemented and need no trust;
//!    `command` shells out through an argv array (never a shell) with a hardened
//!    git-config environment (ADR-0146's lesson) and a timeout that kills the
//!    child (issue #294). A `command` is **never** run untrusted, and **never**
//!    run under the headless harness (assert).

use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::GitError;
use kagi_domain::worktree_steps::{WorktreeStep, WorktreeSteps};

const COMMAND_TIMEOUT_SECS: u64 = 600; // `npm ci` can take minutes; kill past 10m.
const CONFIG_REL_PATH: &str = ".kagi/worktree.toml";

// ────────────────────────────────────────────────────────────
// config parse + hash
// ────────────────────────────────────────────────────────────

/// A parsed, hashed, canonically-located `.kagi/worktree.toml`.
#[derive(Debug, Clone)]
pub struct LoadedWorktreeConfig {
    pub steps: WorktreeSteps,
    /// Canonical absolute path of the config file — half the trust key.
    pub canonical_path: PathBuf,
    /// Lowercase hex SHA-256 of the file's raw bytes — the other half. Content
    /// change ⇒ new hash ⇒ trust must be re-confirmed.
    pub sha256: String,
}

/// Load `<root>/.kagi/worktree.toml` if present. `Ok(None)` when absent (the
/// common case); `Err` only on an unreadable or malformed file.
pub fn load_worktree_config(root: &Path) -> Result<Option<LoadedWorktreeConfig>, GitError> {
    let path = root.join(CONFIG_REL_PATH);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(GitError::Other(format!(
                "cannot read {}: {e}",
                path.display()
            )))
        }
    };
    let sha256 = hex::encode(Sha256::digest(&bytes));
    let text = String::from_utf8(bytes)
        .map_err(|_| GitError::Other(format!("{} is not valid UTF-8", path.display())))?;
    let steps = parse_steps(&text)
        .map_err(|e| GitError::Other(format!("{} is invalid: {e}", path.display())))?;
    // Canonicalize so the trust key is stable regardless of how the repo was
    // opened (symlinks, `..`, relative paths).
    let canonical_path = std::fs::canonicalize(&path).unwrap_or(path);
    Ok(Some(LoadedWorktreeConfig {
        steps,
        canonical_path,
        sha256,
    }))
}

/// Parse the two step arrays out of a `toml::Value` without a serde-derive dep
/// (kagi-git has no serde derive). Unknown `type`s and missing fields are hard
/// errors so a typo never silently drops a step.
fn parse_steps(text: &str) -> Result<WorktreeSteps, String> {
    let value: toml::Value = text.parse().map_err(|e| format!("{e}"))?;
    Ok(WorktreeSteps {
        post_create: parse_phase(&value, "post_create")?,
        pre_remove: parse_phase(&value, "pre_remove")?,
    })
}

fn parse_phase(root: &toml::Value, key: &str) -> Result<Vec<WorktreeStep>, String> {
    let Some(array) = root.get(key) else {
        return Ok(Vec::new());
    };
    let array = array
        .as_array()
        .ok_or_else(|| format!("`{key}` must be an array of tables"))?;
    let mut out = Vec::with_capacity(array.len());
    for (i, item) in array.iter().enumerate() {
        out.push(parse_step(item, key, i)?);
    }
    Ok(out)
}

fn parse_step(item: &toml::Value, key: &str, i: usize) -> Result<WorktreeStep, String> {
    let field = |name: &str| -> Result<String, String> {
        item.get(name)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| format!("`{key}[{i}]` is missing string field `{name}`"))
    };
    match item.get("type").and_then(|v| v.as_str()) {
        Some("copy") => Ok(WorktreeStep::Copy {
            from: field("from")?,
            to: field("to")?,
        }),
        Some("symlink") => Ok(WorktreeStep::Symlink {
            from: field("from")?,
            to: field("to")?,
        }),
        Some("command") => Ok(WorktreeStep::Command { run: field("run")? }),
        Some(other) => Err(format!(
            "`{key}[{i}]` has unknown type `{other}` (expected copy/symlink/command)"
        )),
        None => Err(format!("`{key}[{i}]` is missing string field `type`")),
    }
}

// ────────────────────────────────────────────────────────────
// trust store — trusted_worktree_configs
// ────────────────────────────────────────────────────────────

/// Path to the `trusted_worktree_configs.json` allow-list. Mirrors the oplog's
/// resolution: `$KAGI_LOG_DIR` (test isolation) then `$HOME/.kagi`.
fn trust_store_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("KAGI_LOG_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("trusted_worktree_configs.json"));
        }
    }
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .filter(|s| !s.is_empty())
        .map(|home| {
            PathBuf::from(home)
                .join(".kagi")
                .join("trusted_worktree_configs.json")
        })
}

fn read_trust_entries() -> Vec<(String, String)> {
    let Some(path) = trust_store_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&text)
    else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter_map(|v| {
            let p = v.get("path")?.as_str()?.to_string();
            let s = v.get("sha")?.as_str()?.to_string();
            Some((p, s))
        })
        .collect()
}

/// True when `(config path, sha)` is in the allow-list. A content change moves
/// the SHA and this returns false — forcing a re-confirm (issue #341 §5).
pub fn is_worktree_config_trusted(cfg: &LoadedWorktreeConfig) -> bool {
    let key = cfg.canonical_path.to_string_lossy();
    read_trust_entries()
        .iter()
        .any(|(p, s)| p.as_str() == key && *s == cfg.sha256)
}

/// Record `(config path, sha)` as trusted. Idempotent; entries for the same
/// path with a stale SHA are replaced so the file does not grow unbounded.
pub fn trust_worktree_config(cfg: &LoadedWorktreeConfig) -> Result<(), GitError> {
    let Some(path) = trust_store_path() else {
        return Err(GitError::Other(
            "cannot locate the trust store (no KAGI_LOG_DIR or HOME)".to_string(),
        ));
    };
    let key = cfg.canonical_path.to_string_lossy().to_string();
    let mut entries = read_trust_entries();
    entries.retain(|(p, _)| p != &key);
    entries.push((key, cfg.sha256.clone()));

    let json = serde_json::Value::Array(
        entries
            .into_iter()
            .map(|(p, s)| serde_json::json!({ "path": p, "sha": s }))
            .collect(),
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| GitError::Other(format!("cannot create trust store dir: {e}")))?;
    }
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&json).unwrap_or_default(),
    )
    .map_err(|e| GitError::Other(format!("cannot write trust store: {e}")))
}

// ────────────────────────────────────────────────────────────
// executor
// ────────────────────────────────────────────────────────────

/// Where a step runs: `main_root` is the source (main worktree) for copy/
/// symlink; `worktree` is the freshly-created / to-be-removed worktree and the
/// working directory for commands.
pub struct StepEnv {
    pub main_root: PathBuf,
    pub worktree: PathBuf,
}

/// True unless the headless `KAGI_*` test harness is active. A `command` is
/// **never** executed under headless (issue #341 §6). `KAGI_LOG_DIR` is
/// deliberately excluded: it is only test-isolation for the store and unit
/// tests, and gating on it would make the trusted-command path untestable.
fn command_execution_allowed() -> bool {
    const HEADLESS_SIGNALS: &[&str] = &[
        "KAGI_OPEN_REPO",
        "KAGI_MENU_DUMP",
        "KAGI_SELECT_FIRST",
        "KAGI_NO_SINGLE_INSTANCE",
    ];
    !HEADLESS_SIGNALS
        .iter()
        .any(|k| std::env::var_os(k).is_some())
}

/// Run the `post_create` steps best-effort: the worktree already exists, so a
/// step failure is never allowed to undo it. `copy`/`symlink` always run;
/// `command` runs only when `trusted` and not headless. Returns a human summary
/// of what happened, for the oplog.
pub fn run_post_create(steps: &[WorktreeStep], env: &StepEnv, trusted: bool) -> Vec<String> {
    let mut log = Vec::new();
    for step in steps {
        let line = match step {
            WorktreeStep::Copy { from, to } => match do_copy(env, from, to) {
                Ok(()) => format!("copy ok: {from} → {to}"),
                Err(e) => format!("copy failed: {from} → {to}: {e}"),
            },
            WorktreeStep::Symlink { from, to } => match do_symlink(env, from, to) {
                Ok(()) => format!("symlink ok: {to} → {from}"),
                Err(e) => format!("symlink failed: {to} → {from}: {e}"),
            },
            WorktreeStep::Command { run } => {
                if !command_execution_allowed() {
                    format!("command skipped (headless): {run}")
                } else if !trusted {
                    format!("command skipped (untrusted): {run}")
                } else {
                    match do_command(env, run) {
                        Ok(()) => format!("command ok: {run}"),
                        Err(e) => format!("command failed: {run}: {e}"),
                    }
                }
            }
        };
        log.push(line);
    }
    log
}

/// Run the `pre_remove` steps as a **precondition of deletion**: any failure —
/// a copy/symlink error, or a `command` that fails, is untrusted, or is blocked
/// by headless — returns `Err`, and the caller must then abort the removal so
/// the worktree survives (issue #341 §5, matching kagi's preflight ethos).
pub fn run_pre_remove(
    steps: &[WorktreeStep],
    env: &StepEnv,
    trusted: bool,
) -> Result<(), GitError> {
    for step in steps {
        match step {
            WorktreeStep::Copy { from, to } => do_copy(env, from, to)?,
            WorktreeStep::Symlink { from, to } => do_symlink(env, from, to)?,
            WorktreeStep::Command { run } => {
                if !command_execution_allowed() {
                    return Err(GitError::Other(format!(
                        "pre-remove command cannot run under the headless harness, so cleanup \
                         cannot be verified — aborting removal: {run}"
                    )));
                }
                if !trusted {
                    return Err(GitError::Other(format!(
                        "pre-remove command is not trusted, so it will not run — aborting \
                         removal (trust .kagi/worktree.toml first): {run}"
                    )));
                }
                do_command(env, run)?;
            }
        }
    }
    Ok(())
}

/// Copy `from` (in the main worktree) to `to` (in the worktree). Never
/// overwrites an existing destination.
fn do_copy(env: &StepEnv, from: &str, to: &str) -> Result<(), GitError> {
    let src = env.main_root.join(from);
    let dst = env.worktree.join(to);
    if dst.exists() {
        return Err(GitError::Other(format!(
            "destination '{}' already exists (copy never overwrites)",
            dst.display()
        )));
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| GitError::Other(format!("cannot create '{}': {e}", parent.display())))?;
    }
    std::fs::copy(&src, &dst).map(|_| ()).map_err(|e| {
        GitError::Other(format!(
            "copy '{}' → '{}': {e}",
            src.display(),
            dst.display()
        ))
    })
}

/// Create a symlink at `to` (in the worktree) pointing at the **absolute** path
/// of `from` (in the main worktree). The four worktree-link safety rules
/// (issue #341 §5): a directory is one link (symlinks never recurse); the
/// target is absolute; `.git` is always excluded; an existing destination is
/// **never overwritten** (asserted).
fn do_symlink(env: &StepEnv, from: &str, to: &str) -> Result<(), GitError> {
    if Path::new(from)
        .components()
        .any(|c| c.as_os_str() == ".git")
        || Path::new(to).components().any(|c| c.as_os_str() == ".git")
    {
        return Err(GitError::Other(
            "symlink refuses any path containing '.git'".to_string(),
        ));
    }
    // Absolute target, canonicalized so the link survives a relative-CWD chdir.
    let target = std::fs::canonicalize(env.main_root.join(from))
        .map_err(|e| GitError::Other(format!("symlink source '{from}' unreadable: {e}")))?;
    let link = env.worktree.join(to);
    // Never overwrite (symlink_metadata: do not follow — an existing symlink
    // counts too).
    if std::fs::symlink_metadata(&link).is_ok() {
        return Err(GitError::Other(format!(
            "symlink destination '{}' already exists (never overwritten)",
            link.display()
        )));
    }
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| GitError::Other(format!("cannot create '{}': {e}", parent.display())))?;
    }
    symlink_impl(&target, &link)
}

#[cfg(unix)]
fn symlink_impl(target: &Path, link: &Path) -> Result<(), GitError> {
    std::os::unix::fs::symlink(target, link).map_err(|e| {
        GitError::Other(format!(
            "symlink '{}' → '{}': {e}",
            link.display(),
            target.display()
        ))
    })
}

#[cfg(not(unix))]
fn symlink_impl(_target: &Path, _link: &Path) -> Result<(), GitError> {
    Err(GitError::Other(
        "symlink steps are only supported on Unix".to_string(),
    ))
}

/// Run one `command` step: argv array (never a shell), with a hardened git
/// environment (ADR-0146's lesson: a hostile committed config must not poison
/// the child's git) and a timeout that kills the child (issue #294). The
/// process PATH already carries the login-shell PATH (see `shell_env.rs`), so
/// tools like `npm` resolve.
fn do_command(env: &StepEnv, run: &str) -> Result<(), GitError> {
    use std::process::{Command, Stdio};

    // Whitespace argv split — no shell, so there is no quoting/expansion.
    // ponytail: no shell-words parsing; add it if quoted args in `run` are
    // needed. A shell is deliberately never used (that is the gwq attack path).
    let mut parts = run.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| GitError::Other("empty command step".to_string()))?;
    let args: Vec<&str> = parts.collect();

    let mut child = Command::new(program)
        .args(&args)
        .current_dir(&env.worktree)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| GitError::Other(format!("failed to start '{program}': {e}")))?;

    let Some(status) =
        crate::cli::wait_or_kill(&mut child, Duration::from_secs(COMMAND_TIMEOUT_SECS))
    else {
        return Err(GitError::Other(format!(
            "command '{run}' timed out after {COMMAND_TIMEOUT_SECS}s and was killed"
        )));
    };
    if status.success() {
        Ok(())
    } else {
        Err(GitError::Other(format!(
            "command '{run}' exited with status {}",
            status.code().unwrap_or(-1)
        )))
    }
}

// ────────────────────────────────────────────────────────────
// plan enumeration helpers (shared by create + remove plans)
// ────────────────────────────────────────────────────────────

use kagi_domain::plan_note::{PlanNote, WorktreeNote};

/// Load `<root>/.kagi/worktree.toml` and record it as trusted. The UI calls
/// this when the user confirms a create / remove plan whose step note is
/// `trust_required` — the visible, escaped command list in that plan is the
/// informed-consent prompt (issue #341 §5). No-op (Ok) when there is no config.
pub fn trust_worktree_config_at(root: &Path) -> Result<(), GitError> {
    match load_worktree_config(root)? {
        Some(cfg) => trust_worktree_config(&cfg),
        None => Ok(()),
    }
}

/// True when `plan` carries a `PostCreateSteps` / `PreRemoveSteps` note that is
/// still `trust_required` — i.e. confirming it should grant trust. Lets the UI
/// decide without reaching into the note variants itself.
pub fn plan_requires_worktree_trust(plan: &kagi_domain::plan::OperationPlan) -> bool {
    plan.warnings.iter().chain(plan.blockers.iter()).any(|n| {
        matches!(
            n,
            PlanNote::Worktree(WorktreeNote::PostCreateSteps {
                trust_required: true,
                ..
            }) | PlanNote::Worktree(WorktreeNote::PreRemoveSteps {
                trust_required: true,
                ..
            })
        )
    })
}

/// Build the `PostCreateSteps` plan note for a loaded config, computing
/// `trust_required` from the live trust store. `None` when there are no
/// post-create steps.
pub fn post_create_note(cfg: &LoadedWorktreeConfig) -> Option<PlanNote> {
    if cfg.steps.post_create.is_empty() {
        return None;
    }
    let trust_required = cfg.steps.post_create_needs_trust() && !is_worktree_config_trusted(cfg);
    Some(PlanNote::Worktree(WorktreeNote::PostCreateSteps {
        steps: cfg.steps.post_create.clone(),
        trust_required,
    }))
}

/// Build the `PreRemoveSteps` plan note for a loaded config. `None` when there
/// are no pre-remove steps.
pub fn pre_remove_note(cfg: &LoadedWorktreeConfig) -> Option<PlanNote> {
    if cfg.steps.pre_remove.is_empty() {
        return None;
    }
    let trust_required = cfg.steps.pre_remove_needs_trust() && !is_worktree_config_trusted(cfg);
    Some(PlanNote::Worktree(WorktreeNote::PreRemoveSteps {
        steps: cfg.steps.pre_remove.clone(),
        trust_required,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_all_three_types() {
        let toml = r#"
[[post_create]]
type = "copy"
from = ".env.example"
to = ".env"

[[post_create]]
type = "symlink"
from = ".claude"
to = ".claude"

[[post_create]]
type = "command"
run = "npm ci"

[[pre_remove]]
type = "command"
run = "docker compose down"
"#;
        let steps = parse_steps(toml).unwrap();
        assert_eq!(steps.post_create.len(), 3);
        assert_eq!(steps.pre_remove.len(), 1);
        assert!(steps.post_create_needs_trust());
        assert!(steps.pre_remove_needs_trust());
    }

    #[test]
    fn parse_rejects_unknown_type() {
        let toml = "[[post_create]]\ntype = \"exec\"\nrun = \"x\"\n";
        assert!(parse_steps(toml).is_err());
    }

    #[test]
    fn parse_rejects_missing_field() {
        let toml = "[[post_create]]\ntype = \"copy\"\nfrom = \"a\"\n";
        assert!(parse_steps(toml).is_err());
    }
}
