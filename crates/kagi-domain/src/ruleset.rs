//! GitHub branch-ruleset model and **pure, local** validators (#346, ADR-0150).
//!
//! GitHub exposes the rules that apply to a branch through
//! `GET /repos/{owner}/{repo}/rules/branches/{branch}` — `repo` scope only, no
//! admin. Of the 23 rule types, 13 can be verified with **zero network
//! round-trips** against the change the user is about to make, so a push GitHub
//! would reject is caught while it is still cheap to fix (issue #346 (A) group).
//!
//! This module is the pure core: the [`Ruleset`] struct (the 13 rule
//! parameters), the [`RulesetStatus`] wrapper, and `validate_*` functions that
//! return structured [`Finding`]s. It has **no** git2/gpui/I/O and no JSON
//! dependency — the `gh api` JSON is parsed in `kagi-git::github` and handed
//! here as a plain struct (keeps `kagi-domain` purest; ADR-0150 §3).
//!
//! ## Safety invariant (PM-locked, issue #346 §5)
//!
//! An empty / unparseable API response is **`Unknown`, never "no
//! constraints"**. [`RulesetStatus::from_fetch`] maps a zero-rule response to
//! [`RulesetStatus::Unknown`]; nothing in Kagi ever presents a branch as
//! unconstrained on the strength of an empty response.
//!
//! ## Severity
//!
//! GitHub's branch-rules endpoint does not report `current_user_can_bypass`,
//! so [`Bypass`] is `Unknown` in practice and every finding surfaces as a
//! **warning** ("surface, don't hard-block on incomplete info", §5). The
//! `Allowed → warning` / `Denied → blocker` machinery is kept and tested for
//! when bypass becomes available (#347).

use crate::plan_note::{RuleField, RulesetNote};

// ────────────────────────────────────────────────────────────
// Severity / bypass
// ────────────────────────────────────────────────────────────

/// Whether a finding blocks the operation or only warns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Blocker,
    Warning,
}

/// Whether the current user can bypass the ruleset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Bypass {
    /// `current_user_can_bypass` is true — violations are warnings.
    Allowed,
    /// `current_user_can_bypass` is false — violations are blockers.
    Denied,
    /// Not reported (the branch-rules endpoint does not expose it) — treated
    /// as a warning: surface, don't hard-block on incomplete info (§5).
    #[default]
    Unknown,
}

impl Bypass {
    /// Severity of a *real* rule violation under this bypass state.
    pub fn severity(self) -> Severity {
        match self {
            Bypass::Denied => Severity::Blocker,
            Bypass::Allowed | Bypass::Unknown => Severity::Warning,
        }
    }
}

/// One classified ruleset finding: the rendered note plus its severity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub note: RulesetNote,
    pub severity: Severity,
}

// ────────────────────────────────────────────────────────────
// Pattern rules
// ────────────────────────────────────────────────────────────

/// A pattern-rule operator (GitHub `commit_message_pattern` etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternOp {
    StartsWith,
    EndsWith,
    Contains,
    /// GitHub `regex` — Kagi does not evaluate this locally (dependency-purity:
    /// no regex crate in `kagi-domain`). Surfaced as *uncheckable*, never as
    /// silently satisfied.
    Regex,
}

impl PatternOp {
    /// Parse a GitHub operator slug. Unknown slugs fall back to `Regex` so an
    /// unrecognized operator is surfaced as *uncheckable* rather than skipped.
    pub fn from_slug(s: &str) -> PatternOp {
        match s {
            "starts_with" => PatternOp::StartsWith,
            "ends_with" => PatternOp::EndsWith,
            "contains" => PatternOp::Contains,
            _ => PatternOp::Regex,
        }
    }
}

/// A GitHub pattern rule (`{operator, pattern, negate, name}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    pub operator: PatternOp,
    pub pattern: String,
    /// When true the rule requires the pattern to **not** match.
    pub negate: bool,
}

/// Result of checking a subject against a [`Pattern`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchResult {
    Satisfied,
    Violated,
    /// A `regex` (or unknown) operator — cannot be verified locally.
    Uncheckable,
}

impl Pattern {
    /// Whether the raw (pre-negation) pattern text matches `subject`.
    fn raw_matches(&self, subject: &str) -> bool {
        match self.operator {
            PatternOp::StartsWith => subject.starts_with(&self.pattern),
            PatternOp::EndsWith => subject.ends_with(&self.pattern),
            PatternOp::Contains => subject.contains(&self.pattern),
            PatternOp::Regex => false, // never reached; guarded by `check`
        }
    }

    /// Check `subject` against this rule. Applies `negate`: a non-negated rule
    /// is satisfied when the pattern matches; a negated rule is satisfied when
    /// it does **not** match.
    pub fn check(&self, subject: &str) -> MatchResult {
        if self.operator == PatternOp::Regex {
            return MatchResult::Uncheckable;
        }
        let raw = self.raw_matches(subject);
        // Satisfied iff (raw == !negate): match required unless negated.
        if raw != self.negate {
            MatchResult::Satisfied
        } else {
            MatchResult::Violated
        }
    }

    /// Human description of what this rule demands (the `requirement` text of a
    /// [`RulesetNote::PatternViolation`]).
    pub fn describe(&self) -> String {
        let verb = match (self.operator, self.negate) {
            (PatternOp::StartsWith, false) => "must start with",
            (PatternOp::StartsWith, true) => "must not start with",
            (PatternOp::EndsWith, false) => "must end with",
            (PatternOp::EndsWith, true) => "must not end with",
            (PatternOp::Contains, false) => "must contain",
            (PatternOp::Contains, true) => "must not contain",
            (PatternOp::Regex, _) => "must match the pattern",
        };
        format!("{} \"{}\"", verb, self.pattern)
    }

    /// Produce a finding for `subject` under this rule for `field`, or `None`
    /// when the rule is satisfied.
    fn finding(&self, field: RuleField, subject: &str, bypass: Bypass) -> Option<Finding> {
        match self.check(subject) {
            MatchResult::Satisfied => None,
            MatchResult::Violated => Some(Finding {
                note: RulesetNote::PatternViolation {
                    field,
                    requirement: self.describe(),
                },
                severity: bypass.severity(),
            }),
            MatchResult::Uncheckable => Some(Finding {
                // Uncheckable is always a warning — we simply can't tell.
                note: RulesetNote::PatternUncheckable {
                    field,
                    pattern: self.pattern.clone(),
                },
                severity: Severity::Warning,
            }),
        }
    }
}

// ────────────────────────────────────────────────────────────
// Ruleset
// ────────────────────────────────────────────────────────────

/// The 13 locally-verifiable branch-rule parameters (#346 (A) group).
///
/// Every field defaults to "rule absent" so `Ruleset::default()` is the
/// no-checkable-rules ruleset. `bypass` governs violation severity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ruleset {
    pub commit_message: Option<Pattern>,
    pub commit_author_email: Option<Pattern>,
    pub committer_email: Option<Pattern>,
    pub branch_name: Option<Pattern>,
    /// `max_file_size` normalized to **bytes** (the API reports MB).
    pub max_file_size_bytes: Option<u64>,
    /// Restricted file extensions, without the leading dot, lowercased.
    pub restricted_extensions: Vec<String>,
    /// Restricted path fnmatch patterns.
    pub restricted_paths: Vec<String>,
    pub max_file_path_length: Option<usize>,
    pub required_signatures: bool,
    pub required_linear_history: bool,
    pub non_fast_forward: bool,
    pub creation: bool,
    pub update: bool,
    pub deletion: bool,
    pub bypass: Bypass,
}

impl Ruleset {
    // ── commit-time validators ────────────────────────────────

    /// Validate a commit about to be made: message / author+committer email
    /// pattern rules, and the required-signatures rule.
    pub fn validate_commit(
        &self,
        message: &str,
        author_email: &str,
        committer_email: &str,
        signing_configured: bool,
    ) -> Vec<Finding> {
        let mut out = Vec::new();
        if let Some(p) = &self.commit_message {
            out.extend(p.finding(RuleField::CommitMessage, message, self.bypass));
        }
        if let Some(p) = &self.commit_author_email {
            out.extend(p.finding(RuleField::AuthorEmail, author_email, self.bypass));
        }
        if let Some(p) = &self.committer_email {
            out.extend(p.finding(RuleField::CommitterEmail, committer_email, self.bypass));
        }
        if self.required_signatures && !signing_configured {
            out.push(Finding {
                note: RulesetNote::SignatureRequired,
                severity: self.bypass.severity(),
            });
        }
        out
    }

    /// Validate just the commit-message pattern — for a live, per-keystroke
    /// check where the full commit context (emails, staged files) is not needed.
    pub fn validate_message(&self, message: &str) -> Vec<Finding> {
        self.commit_message
            .as_ref()
            .and_then(|p| p.finding(RuleField::CommitMessage, message, self.bypass))
            .into_iter()
            .collect()
    }

    /// Validate one staged file: max file size, restricted extension,
    /// restricted path, and max path length.
    pub fn validate_staged_file(&self, path: &str, size_bytes: u64) -> Vec<Finding> {
        let mut out = Vec::new();

        if let Some(limit) = self.max_file_size_bytes {
            if size_bytes > limit {
                out.push(Finding {
                    note: RulesetNote::FileTooLarge {
                        path: path.to_string(),
                        size: human_bytes(size_bytes),
                        limit: human_bytes(limit),
                    },
                    severity: self.bypass.severity(),
                });
            }
        }

        if let Some(ext) = file_extension(path) {
            if self.restricted_extensions.iter().any(|r| r == &ext) {
                out.push(Finding {
                    note: RulesetNote::RestrictedExtension {
                        path: path.to_string(),
                        ext,
                    },
                    severity: self.bypass.severity(),
                });
            }
        }

        for pat in &self.restricted_paths {
            if glob_match(pat, path) {
                out.push(Finding {
                    note: RulesetNote::RestrictedPath {
                        path: path.to_string(),
                        pattern: pat.clone(),
                    },
                    severity: self.bypass.severity(),
                });
                break; // one restricted-path finding per file is enough
            }
        }

        if let Some(limit) = self.max_file_path_length {
            let len = path.chars().count();
            if len > limit {
                out.push(Finding {
                    note: RulesetNote::PathTooLong {
                        path: path.to_string(),
                        len,
                        limit,
                    },
                    severity: self.bypass.severity(),
                });
            }
        }

        out
    }

    // ── ref-op validators ─────────────────────────────────────

    /// Validate creating a branch named `name`: the `branch_name` pattern rule
    /// and the `creation` rule.
    pub fn validate_branch_create(&self, name: &str) -> Vec<Finding> {
        let mut out = Vec::new();
        if let Some(p) = &self.branch_name {
            out.extend(p.finding(RuleField::BranchName, name, self.bypass));
        }
        if self.creation {
            out.push(Finding {
                note: RulesetNote::CreationBlocked,
                severity: self.bypass.severity(),
            });
        }
        out
    }

    /// Validate a merge that would (or would not) create a merge commit
    /// against the `required_linear_history` rule.
    pub fn validate_merge(&self, creates_merge_commit: bool) -> Vec<Finding> {
        let mut out = Vec::new();
        if self.required_linear_history && creates_merge_commit {
            out.push(Finding {
                note: RulesetNote::LinearHistoryRequired,
                severity: self.bypass.severity(),
            });
        }
        out
    }

    /// Validate a push against `non_fast_forward`. `is_force` is whether the
    /// push is non-fast-forward; Kagi never force-pushes (invariant #3), so in
    /// practice this is always `false`, but the rule is honored for
    /// completeness.
    pub fn validate_push(&self, is_force: bool) -> Vec<Finding> {
        let mut out = Vec::new();
        if self.non_fast_forward && is_force {
            out.push(Finding {
                note: RulesetNote::NonFastForward,
                severity: self.bypass.severity(),
            });
        }
        out
    }
}

// ────────────────────────────────────────────────────────────
// RulesetStatus
// ────────────────────────────────────────────────────────────

/// The result of trying to determine a branch's ruleset.
///
/// `Active` is the common, load-bearing state (one instance cached per branch);
/// boxing it to shrink the two unit variants would add indirection to every
/// access for no real memory win, so the size skew is accepted.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesetStatus {
    /// `gh` is missing or unauthenticated — the feature is hidden and the
    /// conventional flow runs unchanged (§5). No findings.
    Disabled,
    /// A response arrived but it was empty or unparseable. Treated as
    /// **unknown**, never "no constraints" (§5, PM-locked).
    Unknown,
    /// Active rules parsed from the API.
    Active(Ruleset),
}

impl RulesetStatus {
    /// Classify a fetched ruleset. `rule_count` is the number of rule objects
    /// the API returned (of any type). **Zero → `Unknown`** — an empty
    /// response is unknown, never unconstrained (issue #346 §5, PM-locked).
    pub fn from_fetch(rule_count: usize, ruleset: Ruleset) -> Self {
        if rule_count == 0 {
            RulesetStatus::Unknown
        } else {
            RulesetStatus::Active(ruleset)
        }
    }

    /// The active ruleset, if any. `Disabled`/`Unknown` yield `None` — callers
    /// then contribute no local findings and fall back to the conventional
    /// flow (they must **not** treat `None` as "unconstrained").
    pub fn active(&self) -> Option<&Ruleset> {
        match self {
            RulesetStatus::Active(rs) => Some(rs),
            _ => None,
        }
    }

    /// The note to surface for an *unknown* ruleset, so the empty case is never
    /// silently shown as unconstrained. `None` for `Disabled`/`Active`.
    pub fn unknown_note(&self) -> Option<RulesetNote> {
        match self {
            RulesetStatus::Unknown => Some(RulesetNote::ConstraintsUnknown),
            _ => None,
        }
    }
}

// ────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────

/// Lowercased file extension without the dot, or `None` if the file name has
/// no extension. `.env` (leading-dot, no other dot) has no extension.
fn file_extension(path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let dot = name.rfind('.')?;
    if dot == 0 {
        return None; // dotfile with no extension, e.g. ".env"
    }
    let ext = &name[dot + 1..];
    if ext.is_empty() {
        None
    } else {
        Some(ext.to_ascii_lowercase())
    }
}

/// Minimal fnmatch (path-aware): `?` matches one non-`/`, `*` matches any run
/// of non-`/`, `**` matches any run including `/`. Adequate for GitHub
/// restricted-path patterns; a false positive only ever adds a *warning*.
fn glob_match(pattern: &str, path: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = path.chars().collect();
    glob_rec(&p, &t)
}

fn glob_rec(p: &[char], t: &[char]) -> bool {
    match p.first() {
        None => t.is_empty(),
        Some('*') => {
            // `**` — match across `/` too.
            if p.get(1) == Some(&'*') {
                let rest = &p[2..];
                // zero-width, or consume one char and retry.
                if glob_rec(rest, t) {
                    return true;
                }
                !t.is_empty() && glob_rec(p, &t[1..])
            } else {
                let rest = &p[1..];
                // zero-width, or consume one non-`/` char.
                if glob_rec(rest, t) {
                    return true;
                }
                matches!(t.first(), Some(&c) if c != '/') && glob_rec(p, &t[1..])
            }
        }
        Some('?') => matches!(t.first(), Some(&c) if c != '/') && glob_rec(&p[1..], &t[1..]),
        Some(&pc) => matches!(t.first(), Some(&tc) if tc == pc) && glob_rec(&p[1..], &t[1..]),
    }
}

/// Format a byte count as a short human string (mirrors the checklist's
/// formatter; kept local to preserve `kagi-domain` module independence).
fn human_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{} B", bytes)
    }
}

// ────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn pat(op: PatternOp, s: &str, negate: bool) -> Pattern {
        Pattern {
            operator: op,
            pattern: s.into(),
            negate,
        }
    }

    // ── pattern matching ──────────────────────────────────────

    #[test]
    fn starts_with_satisfied_and_violated() {
        let p = pat(PatternOp::StartsWith, "JIRA-", false);
        assert_eq!(p.check("JIRA-123 fix"), MatchResult::Satisfied);
        assert_eq!(p.check("fix JIRA-123"), MatchResult::Violated);
    }

    #[test]
    fn negate_inverts_requirement() {
        let p = pat(PatternOp::Contains, "wip", true);
        assert_eq!(p.check("ready to go"), MatchResult::Satisfied);
        assert_eq!(p.check("still wip here"), MatchResult::Violated);
    }

    #[test]
    fn ends_with_and_contains() {
        assert_eq!(
            pat(PatternOp::EndsWith, ".", false).check("done."),
            MatchResult::Satisfied
        );
        assert_eq!(
            pat(PatternOp::Contains, "#", false).check("no hash"),
            MatchResult::Violated
        );
    }

    #[test]
    fn regex_is_uncheckable_never_satisfied() {
        let p = pat(PatternOp::Regex, "^feat", false);
        assert_eq!(p.check("feat: x"), MatchResult::Uncheckable);
        assert_eq!(p.check("nope"), MatchResult::Uncheckable);
    }

    #[test]
    fn unknown_operator_slug_falls_back_to_regex() {
        assert_eq!(PatternOp::from_slug("property"), PatternOp::Regex);
        assert_eq!(PatternOp::from_slug("starts_with"), PatternOp::StartsWith);
    }

    #[test]
    fn describe_reads_naturally() {
        assert_eq!(
            pat(PatternOp::StartsWith, "JIRA-", false).describe(),
            "must start with \"JIRA-\""
        );
        assert_eq!(
            pat(PatternOp::Contains, "wip", true).describe(),
            "must not contain \"wip\""
        );
    }

    // ── severity / bypass ─────────────────────────────────────

    #[test]
    fn bypass_governs_violation_severity() {
        assert_eq!(Bypass::Denied.severity(), Severity::Blocker);
        assert_eq!(Bypass::Allowed.severity(), Severity::Warning);
        assert_eq!(Bypass::Unknown.severity(), Severity::Warning);
    }

    #[test]
    fn commit_message_violation_flips_with_bypass() {
        let mk = |bypass| Ruleset {
            commit_message: Some(pat(PatternOp::StartsWith, "JIRA-", false)),
            bypass,
            ..Ruleset::default()
        };
        // Denied → blocker.
        let f = mk(Bypass::Denied).validate_commit("bad msg", "a@b.c", "a@b.c", true);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Blocker);
        // Unknown (the real-world case) → warning.
        let f = mk(Bypass::Unknown).validate_commit("bad msg", "a@b.c", "a@b.c", true);
        assert_eq!(f[0].severity, Severity::Warning);
        // Satisfied → no finding.
        assert!(mk(Bypass::Denied)
            .validate_commit("JIRA-1 fix", "a@b.c", "a@b.c", true)
            .is_empty());
    }

    #[test]
    fn commit_message_pattern_detected() {
        // Acceptance: a message violating commit_message_pattern is detected.
        let rs = Ruleset {
            commit_message: Some(pat(PatternOp::StartsWith, "JIRA-", false)),
            ..Ruleset::default()
        };
        let f = rs.validate_commit("oops no prefix", "a@b.c", "a@b.c", true);
        assert!(matches!(
            f.first().map(|x| &x.note),
            Some(RulesetNote::PatternViolation {
                field: RuleField::CommitMessage,
                ..
            })
        ));
    }

    #[test]
    fn author_and_committer_email_patterns() {
        let rs = Ruleset {
            commit_author_email: Some(pat(PatternOp::EndsWith, "@corp.com", false)),
            committer_email: Some(pat(PatternOp::EndsWith, "@corp.com", false)),
            ..Ruleset::default()
        };
        let f = rs.validate_commit("m", "dev@gmail.com", "bot@ci.corp.com", true);
        // author fails, committer fails (ci.corp.com ends with corp.com? no —
        // "@corp.com" is not a suffix of "bot@ci.corp.com").
        assert_eq!(f.len(), 2);
        let ok = rs.validate_commit("m", "dev@corp.com", "dev@corp.com", true);
        assert!(ok.is_empty());
    }

    #[test]
    fn required_signatures_warns_when_unsigned() {
        let rs = Ruleset {
            required_signatures: true,
            ..Ruleset::default()
        };
        assert_eq!(
            rs.validate_commit("m", "a@b.c", "a@b.c", false)[0].note,
            RulesetNote::SignatureRequired
        );
        assert!(rs.validate_commit("m", "a@b.c", "a@b.c", true).is_empty());
    }

    // ── branch name ───────────────────────────────────────────

    #[test]
    fn branch_name_pattern_detected() {
        // Acceptance: a name violating branch_name_pattern is detected.
        let rs = Ruleset {
            branch_name: Some(pat(PatternOp::StartsWith, "feature/", false)),
            ..Ruleset::default()
        };
        let f = rs.validate_branch_create("hotfix-1");
        assert!(matches!(
            f.first().map(|x| &x.note),
            Some(RulesetNote::PatternViolation {
                field: RuleField::BranchName,
                ..
            })
        ));
        assert!(rs.validate_branch_create("feature/login").is_empty());
    }

    #[test]
    fn creation_rule_blocks_branch_create() {
        let rs = Ruleset {
            creation: true,
            ..Ruleset::default()
        };
        assert_eq!(
            rs.validate_branch_create("anything")[0].note,
            RulesetNote::CreationBlocked
        );
    }

    // ── staged files ──────────────────────────────────────────

    #[test]
    fn max_file_size_detected() {
        // Acceptance: a file exceeding max_file_size is flagged.
        let rs = Ruleset {
            max_file_size_bytes: Some(1024),
            ..Ruleset::default()
        };
        let f = rs.validate_staged_file("big.bin", 2048);
        assert!(matches!(
            f.first().map(|x| &x.note),
            Some(RulesetNote::FileTooLarge { .. })
        ));
        // At/under the limit: no finding.
        assert!(rs.validate_staged_file("ok.bin", 1024).is_empty());
    }

    #[test]
    fn restricted_extension_case_insensitive() {
        let rs = Ruleset {
            restricted_extensions: vec!["exe".into(), "dll".into()],
            ..Ruleset::default()
        };
        assert!(!rs.validate_staged_file("tool.EXE", 1).is_empty());
        assert!(rs.validate_staged_file("main.rs", 1).is_empty());
        // Dotfile with no extension is not flagged.
        assert!(rs.validate_staged_file(".env", 1).is_empty());
    }

    #[test]
    fn restricted_path_glob() {
        let rs = Ruleset {
            restricted_paths: vec!["src/**/*.js".into(), "secrets/*".into()],
            ..Ruleset::default()
        };
        assert!(!rs.validate_staged_file("src/a/b/app.js", 1).is_empty());
        assert!(!rs.validate_staged_file("secrets/key", 1).is_empty());
        assert!(rs.validate_staged_file("src/app.ts", 1).is_empty());
        // `*` does not cross `/`.
        assert!(rs.validate_staged_file("secrets/sub/key", 1).is_empty());
    }

    #[test]
    fn max_path_length() {
        let rs = Ruleset {
            max_file_path_length: Some(10),
            ..Ruleset::default()
        };
        assert!(!rs.validate_staged_file("a/very/long/path.rs", 1).is_empty());
        assert!(rs.validate_staged_file("short.rs", 1).is_empty());
    }

    // ── merge / push ──────────────────────────────────────────

    #[test]
    fn linear_history_only_when_merge_commit() {
        let rs = Ruleset {
            required_linear_history: true,
            ..Ruleset::default()
        };
        assert_eq!(
            rs.validate_merge(true)[0].note,
            RulesetNote::LinearHistoryRequired
        );
        assert!(rs.validate_merge(false).is_empty());
    }

    #[test]
    fn non_fast_forward_only_when_forced() {
        let rs = Ruleset {
            non_fast_forward: true,
            ..Ruleset::default()
        };
        assert_eq!(rs.validate_push(true)[0].note, RulesetNote::NonFastForward);
        assert!(rs.validate_push(false).is_empty());
    }

    // ── status classification (the §5 safety invariant) ───────

    #[test]
    fn empty_response_is_unknown_not_unconstrained() {
        // Acceptance / PM-locked: an empty response is Unknown, never Active.
        let st = RulesetStatus::from_fetch(0, Ruleset::default());
        assert_eq!(st, RulesetStatus::Unknown);
        assert!(st.active().is_none());
        assert_eq!(st.unknown_note(), Some(RulesetNote::ConstraintsUnknown));
    }

    #[test]
    fn nonempty_response_is_active() {
        let rs = Ruleset {
            creation: true,
            ..Ruleset::default()
        };
        let st = RulesetStatus::from_fetch(1, rs.clone());
        assert_eq!(st, RulesetStatus::Active(rs));
        assert!(st.active().is_some());
        assert_eq!(st.unknown_note(), None);
    }

    #[test]
    fn disabled_and_unknown_yield_no_active_ruleset() {
        assert!(RulesetStatus::Disabled.active().is_none());
        assert!(RulesetStatus::Unknown.active().is_none());
        // Disabled must not masquerade as an unknown-note either.
        assert_eq!(RulesetStatus::Disabled.unknown_note(), None);
    }

    #[test]
    fn file_extension_helper() {
        assert_eq!(file_extension("a/b/c.RS"), Some("rs".into()));
        assert_eq!(file_extension(".env"), None);
        assert_eq!(file_extension("Makefile"), None);
        assert_eq!(file_extension("x.tar.gz"), Some("gz".into()));
    }
}
