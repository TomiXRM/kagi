//! Commit-checklist plan notes (ADR-0129 Phase 3) — the staged-content rule
//! findings (ADR-0043 rules 4/5/6) produced by
//! `kagi_domain::checklist::{evaluate_staged_path, evaluate_staged_blob_content}`
//! and surfaced by `plan_commit` / `plan_amend`.
//!
//! Unlike every other `PlanNote` category, `checklist()` only ever
//! contributes blockers/warnings — there is no checklist-specific title or
//! recovery, so no `ChecklistTitle`/`ChecklistRecovery` exist.

/// Findings from the staged-content checklist (ADR-0043 rules 4/5/6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChecklistNote {
    /// warning (rule 5, file-name heuristic) — the staged file's name looks
    /// like a secret (`.env`, `id_rsa`, `*.pem`, …).
    PossibleSecretFileStaged { path: String },
    /// warning (rule 6) — a staged binary exceeds the large-file threshold.
    LargeBinaryStaged { path: String, size: String },
    /// blocker (rule 4) — a staged file still has conflict marker lines.
    ConflictMarkerFound { path: String },
    /// warning (rule 5, content heuristic) — the staged file's content looks
    /// like a secret (PEM private key header, AWS access key id, or a known
    /// token prefix).
    PossibleSecretContentStaged { path: String },
}

impl ChecklistNote {
    /// Byte-identical to the legacy `kagi_domain::checklist` strings
    /// (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            ChecklistNote::PossibleSecretFileStaged { path } => format!(
                "Possible secret file staged: {} — confirm before committing.",
                path
            ),
            ChecklistNote::LargeBinaryStaged { path, size } => format!(
                "Large binary file staged: {} ({}). Confirm before committing.",
                path, size
            ),
            ChecklistNote::ConflictMarkerFound { path } => format!(
                "Conflict marker found in staged file: {}. \
                 Resolve the merge conflict before committing.",
                path
            ),
            ChecklistNote::PossibleSecretContentStaged { path } => format!(
                "Possible secret content in staged file: {} — confirm before committing.",
                path
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── message_en golden tests (ADR-0129 §3): byte-identical to the legacy
    //    format! templates in kagi-domain::checklist. ──

    #[test]
    fn possible_secret_file_staged() {
        assert_eq!(
            ChecklistNote::PossibleSecretFileStaged {
                path: "config/.env".into()
            }
            .message_en(),
            "Possible secret file staged: config/.env — confirm before committing."
        );
    }

    #[test]
    fn large_binary_staged() {
        assert_eq!(
            ChecklistNote::LargeBinaryStaged {
                path: "assets/video.mov".into(),
                size: "6.0 MiB".into()
            }
            .message_en(),
            "Large binary file staged: assets/video.mov (6.0 MiB). Confirm before committing."
        );
    }

    #[test]
    fn conflict_marker_found() {
        assert_eq!(
            ChecklistNote::ConflictMarkerFound {
                path: "src/main.rs".into()
            }
            .message_en(),
            "Conflict marker found in staged file: src/main.rs. \
             Resolve the merge conflict before committing."
        );
    }

    #[test]
    fn possible_secret_content_staged() {
        assert_eq!(
            ChecklistNote::PossibleSecretContentStaged {
                path: "src/config.rs".into()
            }
            .message_en(),
            "Possible secret content in staged file: src/config.rs — confirm before committing."
        );
    }
}
