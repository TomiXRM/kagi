//! JA strings for `ConflictsNote` (ADR-0129 Phase 2 — the git backend's
//! conflict-session module, the Conflict Editor's continue/abort/skip plans).

use kagi_domain::plan_note::{ConflictsNote, ConflictsRecovery, ConflictsTitle};

use crate::i18n::Msg;

pub(crate) const ADVICE_CONFLICTS_OBSERVATION_CHANGED: &str =
    "表示後に conflict の状態が変わりました。conflict を開き直して、操作をやり直してください。";
pub(crate) const ADVICE_CONFLICTS_PLAN_CHANGED: &str =
    "計画後に conflict の状態が変わりました。ファイルは変更していません。操作を選び直して、最新の計画を確認してください。";
pub(crate) const ADVICE_CONFLICTS_REPOSITORY_IDENTITY_CHANGED: &str =
    "計画後にリポジトリの識別情報が変わりました。リポジトリを開き直して、操作をやり直してください。";
pub(crate) const ADVICE_CONFLICTS_CONFLICT_GONE: &str =
    "conflict は既に終了しています。リポジトリを再読み込みしてください。";
pub(crate) const ADVICE_CONFLICTS_RESOLUTION_MARKERS: &str =
    "解決用バッファーに conflict marker が残っています。すべて削除してから保存してください。";
pub(crate) const ADVICE_CONFLICTS_UNRESOLVED_FILES: &str =
    "{} 件が未解決です。続行前にすべて解決してください。\nfiles {}";
pub(crate) const ADVICE_CONFLICTS_MARKER_RESIDUE: &str =
    "conflict marker が残っています。続行前にすべて削除してください。\nfiles {}";
pub(crate) const ADVICE_CONFLICTS_INDEX_UNMERGED: &str =
    "index にこのセッションが把握していない未 merge エントリがあります。リポジトリを再スキャンしてください。\nfiles {}";
pub(crate) const ADVICE_CONFLICTS_BINARY_UNRESOLVED: &str =
    "バイナリ conflict の採用側が未選択です。\nfiles {}";
pub(crate) const ADVICE_CONFLICTS_DELETION_UNDECIDED: &str = "保持か削除かが未決定です。\nfiles {}";
pub(crate) const ADVICE_CONFLICTS_EMPTY_MERGE_MESSAGE: &str =
    "merge commit のメッセージが空です。入力してから続行してください。";
pub(crate) const ADVICE_CONFLICTS_NO_CONFLICTING_FILES_DETECTED: &str =
    "conflict ファイルはありません。続行すると操作はそのまま完了します。";
pub(crate) const ADVICE_CONFLICTS_PARTIAL_RESOLUTIONS_PRESERVED: &str =
    "部分的な解決内容は autosave と oplog に保存されます。破棄されません。";
pub(crate) const ADVICE_CONFLICTS_SKIP_DISCARDS_STEP: &str =
    "Skip は現在ステップの変更を破棄します。conflict を起こした commit は適用されません。部分的な解決内容は autosave に保存されます。";

/// Japanese rendering of one conflicts note.
pub fn note_ja(note: &ConflictsNote) -> String {
    match note {
        ConflictsNote::ObservationChanged => {
            super::advice_text(Msg::AdviceConflictsObservationChanged, &[])
        }
        ConflictsNote::PlanChanged => super::advice_text(Msg::AdviceConflictsPlanChanged, &[]),
        ConflictsNote::RepositoryIdentityChanged => {
            super::advice_text(Msg::AdviceConflictsRepositoryIdentityChanged, &[])
        }
        ConflictsNote::ConflictGone => super::advice_text(Msg::AdviceConflictsConflictGone, &[]),
        ConflictsNote::ResolutionMarkers => {
            super::advice_text(Msg::AdviceConflictsResolutionMarkers, &[])
        }
        ConflictsNote::UnresolvedFiles { files } => super::advice_text(
            Msg::AdviceConflictsUnresolvedFiles,
            &[&files.len(), &files.join(", ")],
        ),
        ConflictsNote::MarkerResidue { files } => {
            super::advice_text(Msg::AdviceConflictsMarkerResidue, &[&files.join(", ")])
        }
        ConflictsNote::IndexUnmerged { files } => {
            super::advice_text(Msg::AdviceConflictsIndexUnmerged, &[&files.join(", ")])
        }
        ConflictsNote::BinaryUnresolved { files } => {
            super::advice_text(Msg::AdviceConflictsBinaryUnresolved, &[&files.join(", ")])
        }
        ConflictsNote::DeletionUndecided { files } => {
            super::advice_text(Msg::AdviceConflictsDeletionUndecided, &[&files.join(", ")])
        }
        ConflictsNote::EmptyMergeMessage => {
            super::advice_text(Msg::AdviceConflictsEmptyMergeMessage, &[])
        }
        // Checklist prose stays untranslated (error/checklist keying is out of
        // scope for this migration — mirrors CommonNote::GitErrorPassthrough).
        ConflictsNote::ChecklistBlocker { message } => message.clone(),
        ConflictsNote::NoConflictingFilesDetected => {
            super::advice_text(Msg::AdviceConflictsNoConflictingFilesDetected, &[])
        }
        ConflictsNote::PartialResolutionsPreserved => {
            super::advice_text(Msg::AdviceConflictsPartialResolutionsPreserved, &[])
        }
        ConflictsNote::SkipDiscardsStep => {
            super::advice_text(Msg::AdviceConflictsSkipDiscardsStep, &[])
        }
    }
}

/// Japanese rendering of one conflicts title.
pub fn title_ja(title: &ConflictsTitle) -> String {
    match title {
        ConflictsTitle::Continue { op } => format!("{} を続行", op),
        ConflictsTitle::Abort { op } => format!("{} を中止", op),
        ConflictsTitle::Skip { op } => format!("{} のステップをスキップ", op),
    }
}

/// Japanese rendering of one conflicts recovery block.
pub fn recovery_ja(recovery: &ConflictsRecovery) -> String {
    match recovery {
        ConflictsRecovery::Continue { op } => format!(
            "うまくいかない場合は操作前の状態に戻せます:\n  git {} --abort\n\
             操作前の HEAD は ORIG_HEAD と reflog に残ります。",
            op
        ),
        ConflictsRecovery::Abort { op } => format!(
            "Abort は ORIG_HEAD から {} 実行前の状態を復元します。HEAD 移動はすべて reflog に残ります。",
            op
        ),
        ConflictsRecovery::Skip { op } => format!(
            "Skip は現在の {} ステップを破棄します。HEAD 移動は reflog に残り、完全に中止する場合は操作前の HEAD が ORIG_HEAD に残ります。",
            op
        ),
    }
}
