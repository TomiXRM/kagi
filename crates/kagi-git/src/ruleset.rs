//! GitHub branch-ruleset fetch + parse + cache (#346, ADR-0150).
//!
//! Fetches `GET /repos/{owner}/{repo}/rules/branches/{branch}` via the `gh`
//! CLI (same auth story as [`crate::github`]), parses the JSON into the pure
//! [`Ruleset`] model (`kagi-domain` stays JSON-free — ADR-0150 §3), and caches
//! the result per `(workdir, branch)`.
//!
//! ## Cache contract
//!
//! * [`ruleset_for`] is cache-first: it fetches over the network only on a
//!   miss, so a cache hit is a **zero round-trip** answer (issue #346 §6).
//! * [`ruleset_cached`] never touches the network — plans use it so planning
//!   stays offline. `None` means "not fetched yet", which callers treat as
//!   "no local findings, keep the conventional flow" (never "unconstrained").
//! * [`refresh_ruleset`] forces a fetch and overwrites the cache — called on
//!   `git fetch` and on explicit refresh only (no TTL timer, §5 PM-locked).
//!
//! ## `gh` absent / unauthenticated
//!
//! [`fetch_ruleset`] returns [`RulesetStatus::Disabled`] when `gh` is missing
//! or the API call fails (not a GitHub repo, logged out): the feature is
//! hidden and the conventional flow runs unchanged — no error is thrown (§5).

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use kagi_domain::plan::OperationPlan;
use kagi_domain::plan_note::{PlanDisposition, PlanNote};
use kagi_domain::ruleset::{Bypass, Finding, Pattern, PatternOp, Ruleset, RulesetStatus, Severity};

use crate::github::gh_available;

// ────────────────────────────────────────────────────────────
// Cache
// ────────────────────────────────────────────────────────────

fn cache() -> &'static Mutex<HashMap<String, RulesetStatus>> {
    static C: OnceLock<Mutex<HashMap<String, RulesetStatus>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_key(workdir: &Path, branch: &str) -> String {
    format!("{}\u{0}{}", workdir.display(), branch)
}

/// Return the cached status for `key`, or compute + store it via `f`. `f` runs
/// **only on a miss** — this is the "cache hit → no network" seam (unit-tested
/// with an injected counting closure).
fn cached_or<F: FnOnce() -> RulesetStatus>(key: &str, f: F) -> RulesetStatus {
    if let Some(hit) = cache().lock().unwrap().get(key).cloned() {
        return hit;
    }
    let v = f();
    cache().lock().unwrap().insert(key.to_string(), v.clone());
    v
}

/// Cache-first ruleset for `branch`; fetches over the network only on a miss.
pub fn ruleset_for(workdir: &Path, branch: &str) -> RulesetStatus {
    let key = cache_key(workdir, branch);
    cached_or(&key, || fetch_ruleset(workdir, branch))
}

/// Cache-only lookup — never touches the network. `None` = not fetched yet.
pub fn ruleset_cached(workdir: &Path, branch: &str) -> Option<RulesetStatus> {
    cache()
        .lock()
        .unwrap()
        .get(&cache_key(workdir, branch))
        .cloned()
}

/// Force a fetch and overwrite the cache. Call on fetch / explicit refresh.
pub fn refresh_ruleset(workdir: &Path, branch: &str) -> RulesetStatus {
    let v = fetch_ruleset(workdir, branch);
    cache()
        .lock()
        .unwrap()
        .insert(cache_key(workdir, branch), v.clone());
    v
}

/// Seed the cache directly. The UI uses this after a background fetch; tests
/// use it to exercise plan integration without a network call.
pub fn seed_ruleset(workdir: &Path, branch: &str, status: RulesetStatus) {
    cache()
        .lock()
        .unwrap()
        .insert(cache_key(workdir, branch), status);
}

// ────────────────────────────────────────────────────────────
// Fetch
// ────────────────────────────────────────────────────────────

fn fetch_ruleset(workdir: &Path, branch: &str) -> RulesetStatus {
    if !gh_available() {
        return RulesetStatus::Disabled;
    }
    let path = format!("repos/{{owner}}/{{repo}}/rules/branches/{}", branch);
    let out = Command::new("gh")
        .args(["api", &path])
        .current_dir(workdir)
        .output();
    match out {
        Ok(o) if o.status.success() => parse_ruleset(&String::from_utf8_lossy(&o.stdout)),
        // Non-zero exit: not a GitHub repo / logged out / no such branch — the
        // feature is simply unavailable here, not an error (§5).
        Ok(_) => RulesetStatus::Disabled,
        Err(_) => RulesetStatus::Disabled,
    }
}

// ────────────────────────────────────────────────────────────
// Parse (JSON → RulesetStatus). Pure; unit-tested below.
// ────────────────────────────────────────────────────────────

/// Parse `GET /rules/branches/{branch}` JSON into a [`RulesetStatus`].
///
/// An empty array — or anything that is not a JSON array — is
/// [`RulesetStatus::Unknown`], **never** an empty/unconstrained ruleset
/// (issue #346 §5, PM-locked).
pub fn parse_ruleset(json: &str) -> RulesetStatus {
    let value: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return RulesetStatus::Unknown,
    };
    let arr = match value.as_array() {
        Some(a) => a,
        None => return RulesetStatus::Unknown,
    };

    let mut rs = Ruleset::default();
    for rule in arr {
        let ty = rule.get("type").and_then(|x| x.as_str()).unwrap_or("");
        let params = rule.get("parameters");
        match ty {
            "commit_message_pattern" => rs.commit_message = parse_pattern(params),
            "commit_author_email_pattern" => rs.commit_author_email = parse_pattern(params),
            "committer_email_pattern" => rs.committer_email = parse_pattern(params),
            "branch_name_pattern" => rs.branch_name = parse_pattern(params),
            "max_file_size" => {
                // API reports the limit in MB; normalize to bytes.
                rs.max_file_size_bytes = num(params, "max_file_size").map(|mb| mb * 1024 * 1024);
            }
            "file_extension_restriction" => {
                rs.restricted_extensions = str_array(params, "restricted_file_extensions")
                    .into_iter()
                    .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
                    .collect();
            }
            "file_path_restriction" => {
                rs.restricted_paths = str_array(params, "restricted_file_paths");
            }
            "max_file_path_length" => {
                rs.max_file_path_length = num(params, "max_file_path_length").map(|n| n as usize);
            }
            "required_signatures" => rs.required_signatures = true,
            "required_linear_history" => rs.required_linear_history = true,
            "non_fast_forward" => rs.non_fast_forward = true,
            "creation" => rs.creation = true,
            "update" => rs.update = true,
            "deletion" => rs.deletion = true,
            // (B)-group / unknown rule types (#347): they still count toward
            // the rule total (so the response is not "empty"), but Kagi has no
            // local check for them.
            _ => {}
        }
    }
    // The branch-rules endpoint does not report `current_user_can_bypass`, so
    // bypass is Unknown → findings surface as warnings (§5).
    rs.bypass = Bypass::Unknown;

    RulesetStatus::from_fetch(arr.len(), rs)
}

fn parse_pattern(params: Option<&serde_json::Value>) -> Option<Pattern> {
    let p = params?;
    let pattern = p.get("pattern")?.as_str()?.to_string();
    let operator = PatternOp::from_slug(
        p.get("operator")
            .and_then(|x| x.as_str())
            .unwrap_or("regex"),
    );
    let negate = p.get("negate").and_then(|x| x.as_bool()).unwrap_or(false);
    Some(Pattern {
        operator,
        pattern,
        negate,
    })
}

fn num(params: Option<&serde_json::Value>, key: &str) -> Option<u64> {
    params?.get(key)?.as_u64()
}

fn str_array(params: Option<&serde_json::Value>, key: &str) -> Vec<String> {
    params
        .and_then(|p| p.get(key))
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

// ────────────────────────────────────────────────────────────
// Apply findings to a plan
// ────────────────────────────────────────────────────────────

/// Fold ruleset findings into an operation plan: blockers into `blockers`,
/// warnings into `warnings`, and re-derive the disposition to `Blocked` when
/// any blocker was added. A no-op for an empty findings list, so a
/// `Disabled`/`Unknown`/unfetched ruleset leaves the plan exactly as it was
/// (the conventional flow is intact — §5).
pub fn apply_findings(plan: &mut OperationPlan, findings: Vec<Finding>) {
    for f in findings {
        let note = PlanNote::Ruleset(f.note);
        match f.severity {
            Severity::Blocker => plan.blockers.push(note),
            Severity::Warning => plan.warnings.push(note),
        }
    }
    if !plan.blockers.is_empty() {
        plan.disposition = PlanDisposition::Blocked;
    }
}

// ────────────────────────────────────────────────────────────
// Plan integration (git2 glue → domain validators)
// ────────────────────────────────────────────────────────────

/// Fold the cached branch ruleset's commit findings into `plan` (#346):
/// message / author+committer email patterns, required signatures, and the
/// per-staged-file rules (size / extension / path / path length). Cache-only —
/// a no-op when nothing is cached, `gh` is unavailable, or the response was
/// empty (Unknown), so the conventional commit flow is untouched.
pub fn augment_commit_plan(
    plan: &mut OperationPlan,
    repo: &git2::Repository,
    status: &crate::status::WorkingTreeStatus,
    branch: &str,
    message: &str,
) {
    let rs = match repo
        .workdir()
        .and_then(|w| ruleset_cached(w, branch))
        .and_then(|s| s.active().cloned())
    {
        Some(rs) => rs,
        None => return,
    };

    let email = signature_email(repo);
    let mut findings = rs.validate_commit(message, &email, &email, signing_configured(repo));

    // Per-staged-file rules — skip deletions (no content is pushed) and
    // gitlinks / blobless entries.
    if let Ok(index) = repo.index() {
        for file in &status.staged {
            if matches!(file.change, crate::status::ChangeKind::Deleted) {
                continue;
            }
            let size = index
                .get_path(file.path.as_path(), 0)
                .and_then(|e| repo.find_blob(e.id).ok())
                .map(|b| b.size() as u64)
                .unwrap_or(0);
            findings.extend(rs.validate_staged_file(&file.path.to_string_lossy(), size));
        }
    }
    apply_findings(plan, findings);
}

/// Fold the cached ruleset for the *new* branch `name` into `plan` (#346):
/// branch_name pattern + creation rule. Cache-only; a no-op when uncached.
pub fn augment_branch_create_plan(plan: &mut OperationPlan, repo: &git2::Repository, name: &str) {
    if let Some(rs) = repo
        .workdir()
        .and_then(|w| ruleset_cached(w, name))
        .and_then(|s| s.active().cloned())
    {
        apply_findings(plan, rs.validate_branch_create(name));
    }
}

/// The email git would stamp on the commit (author == committer for Kagi),
/// falling back to `build_signature`'s default.
fn signature_email(repo: &git2::Repository) -> String {
    repo.config()
        .and_then(|c| c.get_string("user.email"))
        .unwrap_or_else(|_| "kagi@local".to_string())
}

/// Whether commit signing is configured (`commit.gpgsign` true, or a
/// `user.signingkey` set) — the `required_signatures` check.
fn signing_configured(repo: &git2::Repository) -> bool {
    let cfg = match repo.config() {
        Ok(c) => c,
        Err(_) => return false,
    };
    cfg.get_bool("commit.gpgsign").unwrap_or(false)
        || cfg
            .get_string("user.signingkey")
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_domain::plan_note::{RuleField, RulesetNote};

    // ── parse ─────────────────────────────────────────────────

    #[test]
    fn empty_array_is_unknown_not_unconstrained() {
        // Acceptance / PM-locked (§5).
        assert_eq!(parse_ruleset("[]"), RulesetStatus::Unknown);
    }

    #[test]
    fn unparseable_is_unknown() {
        assert_eq!(parse_ruleset("not json"), RulesetStatus::Unknown);
        assert_eq!(parse_ruleset("{}"), RulesetStatus::Unknown); // object, not array
    }

    #[test]
    fn parses_pattern_and_size_rules() {
        let json = r#"[
            {"type":"commit_message_pattern","parameters":{"operator":"starts_with","pattern":"JIRA-","negate":false}},
            {"type":"max_file_size","parameters":{"max_file_size":10}},
            {"type":"file_extension_restriction","parameters":{"restricted_file_extensions":[".EXE","dll"]}},
            {"type":"required_signatures"},
            {"type":"creation"}
        ]"#;
        let rs = match parse_ruleset(json) {
            RulesetStatus::Active(rs) => rs,
            other => panic!("expected Active, got {other:?}"),
        };
        let msg = rs.commit_message.expect("commit_message");
        assert_eq!(msg.operator, PatternOp::StartsWith);
        assert_eq!(msg.pattern, "JIRA-");
        assert_eq!(rs.max_file_size_bytes, Some(10 * 1024 * 1024));
        assert_eq!(rs.restricted_extensions, vec!["exe", "dll"]);
        assert!(rs.required_signatures);
        assert!(rs.creation);
        assert_eq!(rs.bypass, Bypass::Unknown);
    }

    #[test]
    fn unknown_rule_type_still_counts_as_nonempty() {
        // A (B)-group rule alone → Active (not Unknown), with no local checks.
        let json = r#"[{"type":"pull_request","parameters":{}}]"#;
        assert!(matches!(parse_ruleset(json), RulesetStatus::Active(_)));
    }

    #[test]
    fn regex_operator_pattern_parses_as_regex() {
        let json = r#"[{"type":"branch_name_pattern","parameters":{"operator":"regex","pattern":"^feat/"}}]"#;
        let rs = parse_ruleset(json).active().cloned().expect("active");
        assert_eq!(rs.branch_name.unwrap().operator, PatternOp::Regex);
    }

    // ── cache ─────────────────────────────────────────────────

    #[test]
    fn cache_hit_skips_the_fetcher() {
        // Acceptance: a cache hit means no second network round-trip.
        use std::sync::atomic::{AtomicUsize, Ordering};
        let key = "cache_hit_skips_the_fetcher\u{0}unique";
        let calls = AtomicUsize::new(0);
        let run = || {
            cached_or(key, || {
                calls.fetch_add(1, Ordering::SeqCst);
                RulesetStatus::Unknown
            })
        };
        assert_eq!(run(), RulesetStatus::Unknown);
        assert_eq!(run(), RulesetStatus::Unknown);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "fetcher ran only on the miss"
        );
    }

    #[test]
    fn seed_then_cached_returns_without_fetch() {
        let dir = Path::new("/tmp/kagi-ruleset-test-seed");
        let rs = Ruleset {
            creation: true,
            ..Ruleset::default()
        };
        seed_ruleset(dir, "main", RulesetStatus::Active(rs.clone()));
        assert_eq!(ruleset_cached(dir, "main"), Some(RulesetStatus::Active(rs)));
        assert_eq!(ruleset_cached(dir, "never-fetched"), None);
    }

    // ── apply_findings ────────────────────────────────────────

    #[test]
    fn apply_warning_keeps_plan_ready() {
        let mut plan = dummy_plan();
        apply_findings(
            &mut plan,
            vec![Finding {
                note: RulesetNote::PatternViolation {
                    field: RuleField::CommitMessage,
                    requirement: "must start with \"x\"".into(),
                },
                severity: Severity::Warning,
            }],
        );
        assert_eq!(plan.warnings.len(), 1);
        assert!(plan.blockers.is_empty());
        assert_eq!(plan.disposition, PlanDisposition::Ready);
    }

    #[test]
    fn apply_blocker_flips_disposition() {
        let mut plan = dummy_plan();
        apply_findings(
            &mut plan,
            vec![Finding {
                note: RulesetNote::CreationBlocked,
                severity: Severity::Blocker,
            }],
        );
        assert_eq!(plan.blockers.len(), 1);
        assert_eq!(plan.disposition, PlanDisposition::Blocked);
    }

    #[test]
    fn apply_empty_is_a_noop() {
        let mut plan = dummy_plan();
        let before = plan.disposition;
        apply_findings(&mut plan, vec![]);
        assert!(plan.warnings.is_empty());
        assert!(plan.blockers.is_empty());
        assert_eq!(plan.disposition, before);
    }

    fn dummy_plan() -> OperationPlan {
        use kagi_domain::head::Head;
        use kagi_domain::plan::StateSummary;
        use kagi_domain::plan_note::{PlanTitle, PushTitle};
        OperationPlan {
            title: PlanTitle::Push(PushTitle::Push {
                branch: "main".into(),
                remote: "origin".into(),
                set_upstream: false,
            }),
            current: StateSummary {
                head: "branch: main".into(),
                dirty: "clean".into(),
            },
            predicted: StateSummary {
                head: "branch: main".into(),
                dirty: "clean".into(),
            },
            warnings: Vec::new(),
            blockers: Vec::new(),
            recovery: None,
            disposition: PlanDisposition::Ready,
            head_at_plan: Head::Detached {
                target: "0000000000000000000000000000000000000000".into(),
            },
            stash_count_at_plan: 0,
            worktree_digest: None,
            preview_files: Vec::new(),
            preview_commits: Vec::new(),
            destructive: false,
        }
    }
}
