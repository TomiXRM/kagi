//! GitHub-ruleset plan notes (#346, ADR-0150) — local pre-verification of the
//! remote branch ruleset fetched via `gh api /repos/{o}/{r}/rules/branches/{b}`.
//!
//! These notes surface *before* an operation runs (commit / branch-create /
//! push / merge) so a change that GitHub would reject on push is caught while
//! it is still cheap to fix. The (A)-group rules that can be verified with no
//! server round-trip (13 rule types, issue #346) are produced by the pure
//! validators in [`crate::ruleset`]; this enum is only their rendered form.
//!
//! Like [`super::checklist`], ruleset findings carry no title/recovery — they
//! are contributed as extra blockers/warnings on an operation's own plan.

/// Which subject a pattern rule checked. Renders the leading noun of a
/// [`RulesetNote::PatternViolation`] / [`RulesetNote::PatternUncheckable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleField {
    CommitMessage,
    AuthorEmail,
    CommitterEmail,
    BranchName,
}

impl RuleField {
    /// English subject noun.
    pub fn subject_en(self) -> &'static str {
        match self {
            RuleField::CommitMessage => "Commit message",
            RuleField::AuthorEmail => "Commit author email",
            RuleField::CommitterEmail => "Committer email",
            RuleField::BranchName => "Branch name",
        }
    }
}

/// One rendered GitHub-ruleset finding (#346).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesetNote {
    /// A pattern rule is violated. `requirement` is the human description of
    /// what the pattern demands (e.g. `must start with "JIRA-"`).
    PatternViolation {
        field: RuleField,
        requirement: String,
    },
    /// A pattern rule uses the `regex` operator, which Kagi does not evaluate
    /// locally. Surfaced (never silently passed) so a regex rule is not
    /// mistaken for "satisfied".
    PatternUncheckable { field: RuleField, pattern: String },
    /// A staged file exceeds the ruleset's maximum file size.
    FileTooLarge {
        path: String,
        size: String,
        limit: String,
    },
    /// A staged file's extension is on the ruleset's restricted list.
    RestrictedExtension { path: String, ext: String },
    /// A staged file's path matches a restricted-path pattern.
    RestrictedPath { path: String, pattern: String },
    /// A staged file's path exceeds the ruleset's maximum path length.
    PathTooLong {
        path: String,
        len: usize,
        limit: usize,
    },
    /// The ruleset requires signed commits, but commit signing is not
    /// configured locally.
    SignatureRequired,
    /// The operation would create a merge commit, but the ruleset requires
    /// linear history.
    LinearHistoryRequired,
    /// The ruleset forbids non-fast-forward updates to the branch.
    NonFastForward,
    /// The ruleset forbids creating the branch.
    CreationBlocked,
    /// The ruleset forbids updating the branch.
    UpdateBlocked,
    /// The ruleset forbids deleting the branch.
    DeletionBlocked,
    /// The branch ruleset could not be determined (empty or unparseable API
    /// response). Surfaced so an *unknown* ruleset is never presented as
    /// "no constraints" (issue #346 §5, PM-locked).
    ConstraintsUnknown,
}

impl RulesetNote {
    /// The sole English renderer (ADR-0129 §3).
    pub fn message_en(&self) -> String {
        match self {
            RulesetNote::PatternViolation { field, requirement } => format!(
                "{} does not satisfy the branch ruleset: {}.",
                field.subject_en(),
                requirement
            ),
            RulesetNote::PatternUncheckable { field, pattern } => format!(
                "{} is constrained by a regex ruleset (/{}/ ) that Kagi cannot verify locally — \
                 GitHub will check it on push.",
                field.subject_en(),
                pattern
            ),
            RulesetNote::FileTooLarge { path, size, limit } => format!(
                "{} ({}) exceeds the ruleset's max file size ({}); GitHub will reject the push.",
                path, size, limit
            ),
            RulesetNote::RestrictedExtension { path, ext } => format!(
                "{} has a restricted extension (.{}); the ruleset forbids it.",
                path, ext
            ),
            RulesetNote::RestrictedPath { path, pattern } => format!(
                "{} matches a restricted path pattern ({}); the ruleset forbids it.",
                path, pattern
            ),
            RulesetNote::PathTooLong { path, len, limit } => format!(
                "{} has a {}-character path, over the ruleset's limit of {}.",
                path, len, limit
            ),
            RulesetNote::SignatureRequired => "The branch ruleset requires signed commits, but \
                 commit signing is not configured (commit.gpgsign / user.signingkey)."
                .to_string(),
            RulesetNote::LinearHistoryRequired => "The branch ruleset requires linear history; a \
                 merge commit would be rejected on push."
                .to_string(),
            RulesetNote::NonFastForward => "The branch ruleset forbids non-fast-forward updates \
                 to this branch."
                .to_string(),
            RulesetNote::CreationBlocked => {
                "The branch ruleset forbids creating this branch.".to_string()
            }
            RulesetNote::UpdateBlocked => {
                "The branch ruleset forbids updating this branch.".to_string()
            }
            RulesetNote::DeletionBlocked => {
                "The branch ruleset forbids deleting this branch.".to_string()
            }
            RulesetNote::ConstraintsUnknown => "The branch ruleset could not be determined; \
                 Kagi is keeping the conservative flow rather than assuming there are no rules."
                .to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_violation_renders_subject_and_requirement() {
        assert_eq!(
            RulesetNote::PatternViolation {
                field: RuleField::CommitMessage,
                requirement: "must start with \"JIRA-\"".into(),
            }
            .message_en(),
            "Commit message does not satisfy the branch ruleset: must start with \"JIRA-\"."
        );
        assert_eq!(
            RulesetNote::PatternViolation {
                field: RuleField::BranchName,
                requirement: "must not contain \"wip\"".into(),
            }
            .message_en(),
            "Branch name does not satisfy the branch ruleset: must not contain \"wip\"."
        );
    }

    #[test]
    fn file_rules_render() {
        assert_eq!(
            RulesetNote::FileTooLarge {
                path: "big.bin".into(),
                size: "12.0 MiB".into(),
                limit: "10.0 MiB".into(),
            }
            .message_en(),
            "big.bin (12.0 MiB) exceeds the ruleset's max file size (10.0 MiB); \
             GitHub will reject the push."
        );
        assert_eq!(
            RulesetNote::RestrictedExtension {
                path: "a.exe".into(),
                ext: "exe".into()
            }
            .message_en(),
            "a.exe has a restricted extension (.exe); the ruleset forbids it."
        );
    }

    #[test]
    fn unknown_is_never_unconstrained_wording() {
        let msg = RulesetNote::ConstraintsUnknown.message_en();
        // Must state "unknown", and must not present the branch as safe.
        assert!(msg.contains("could not be determined"));
        assert!(!msg.to_lowercase().contains("unconstrained"));
        assert!(!msg.to_lowercase().contains("no constraints"));
    }

    #[test]
    fn every_variant_renders_nonempty() {
        let all = [
            RulesetNote::PatternViolation {
                field: RuleField::AuthorEmail,
                requirement: "x".into(),
            },
            RulesetNote::PatternUncheckable {
                field: RuleField::CommitterEmail,
                pattern: "x".into(),
            },
            RulesetNote::FileTooLarge {
                path: "a".into(),
                size: "1 B".into(),
                limit: "1 B".into(),
            },
            RulesetNote::RestrictedExtension {
                path: "a".into(),
                ext: "e".into(),
            },
            RulesetNote::RestrictedPath {
                path: "a".into(),
                pattern: "p".into(),
            },
            RulesetNote::PathTooLong {
                path: "a".into(),
                len: 2,
                limit: 1,
            },
            RulesetNote::SignatureRequired,
            RulesetNote::LinearHistoryRequired,
            RulesetNote::NonFastForward,
            RulesetNote::CreationBlocked,
            RulesetNote::UpdateBlocked,
            RulesetNote::DeletionBlocked,
            RulesetNote::ConstraintsUnknown,
        ];
        for n in &all {
            assert!(!n.message_en().trim().is_empty(), "empty for {n:?}");
        }
    }
}
