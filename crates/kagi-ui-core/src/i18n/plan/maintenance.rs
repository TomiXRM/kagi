//! JA strings for `MaintenanceNote`/`MaintenanceTitle`/`MaintenanceRecovery`
//! (repository-health fixes offered by Analyze, #358 / ADR-0205), and the
//! EN/JA text of Analyze's Health axis that offers them.

use kagi_domain::plan_note::{MaintenanceNote, MaintenanceRecovery, MaintenanceTitle};
use kagi_domain::repo_health::HealthFinding;

use crate::i18n::{lang, Lang, Msg};

/// What a health finding means for the user, and what the fix does.
pub fn finding_text(finding: HealthFinding) -> &'static str {
    match (finding, lang()) {
        (HealthFinding::CommitGraphMissing, Lang::En) => {
            "No commit-graph: every history walk (the graph, Analyze, blame) parses each commit \
             object. Writing one makes those walks faster."
        }
        (HealthFinding::CommitGraphMissing, Lang::Ja) => {
            "commit-graph がありません。グラフ表示・Analyze・blame などの履歴走査が、毎回すべてのコミットを解析しています。書き込むと高速になります。"
        }
        (HealthFinding::CommitGraphStale, Lang::En) => {
            "The commit-graph is older than HEAD: commits made since it was written are walked \
             without it. Rewriting it covers them."
        }
        (HealthFinding::CommitGraphStale, Lang::Ja) => {
            "commit-graph が HEAD より古く、その後のコミットは commit-graph なしで走査されます。書き直すと対象になります。"
        }
        (HealthFinding::FsmonitorUnset, Lang::En) => {
            "core.fsmonitor is not set: every status scans the whole working tree. git's \
             built-in filesystem monitor watches it for changes instead."
        }
        (HealthFinding::FsmonitorUnset, Lang::Ja) => {
            "core.fsmonitor が未設定のため、status のたびに作業ツリー全体を走査しています。git 組み込みのファイルシステムモニターが代わりに変更を監視します。"
        }
    }
}

/// The finding's action: opens the fix's plan for confirmation.
pub fn enable_label() -> &'static str {
    match lang() {
        Lang::En => "Enable…",
        Lang::Ja => "有効化…",
    }
}

/// Short, localized AFTER-state chip; the complete prediction is retained in
/// the accessible state description and Copy all text.
pub fn after_state_label(title: &MaintenanceTitle) -> &'static str {
    match title {
        MaintenanceTitle::WriteCommitGraph => Msg::MaintenanceCommitGraphAfter.t(),
        MaintenanceTitle::EnableFsmonitor => Msg::MaintenanceFsmonitorAfter.t(),
    }
}

/// The full localized AFTER-state explanation for accessibility and Copy all.
pub fn after_state_detail(title: &MaintenanceTitle) -> &'static str {
    match title {
        MaintenanceTitle::WriteCommitGraph => Msg::MaintenanceCommitGraphDetail.t(),
        MaintenanceTitle::EnableFsmonitor => Msg::MaintenanceFsmonitorDetail.t(),
    }
}

/// The fix's confirm button on its plan card.
pub fn confirm_label(title: &MaintenanceTitle) -> &'static str {
    match title {
        MaintenanceTitle::WriteCommitGraph => Msg::MaintenanceWriteCommitGraph.t(),
        MaintenanceTitle::EnableFsmonitor => Msg::MaintenanceEnableFsmonitor.t(),
    }
}

/// The Health axis with nothing to suggest.
pub fn healthy_text() -> &'static str {
    match lang() {
        Lang::En => {
            "Nothing to suggest: the commit-graph is current and the filesystem monitor is set \
             (or not available on this platform)."
        }
        Lang::Ja => {
            "提案はありません。commit-graph は最新で、ファイルシステムモニターも設定済み(またはこの環境では利用不可)です。"
        }
    }
}

/// The Health axis while the backend reads the repository.
pub fn checking_text() -> &'static str {
    match lang() {
        Lang::En => "Checking repository health…",
        Lang::Ja => "リポジトリの健全性を確認中…",
    }
}

/// Japanese rendering of one maintenance note.
pub fn note_ja(note: &MaintenanceNote) -> String {
    match note {
        MaintenanceNote::NoCommits => {
            "このリポジトリにはまだコミットがないため、commit-graph に書く履歴がありません。"
                .to_string()
        }
        MaintenanceNote::FsmonitorAlreadySet { value } => format!(
            "core.fsmonitor は既に '{value}' に設定されています。Kagi は既存の設定を変更しません。"
        ),
        MaintenanceNote::FsmonitorUnsupported => {
            "git の組み込みファイルシステムモニターは macOS と Windows でのみ利用できます。"
                .to_string()
        }
        MaintenanceNote::FsmonitorStartsDaemon => {
            "git がこの作業ツリーの変更を監視するバックグラウンドの daemon を起動し、status が全ファイルを走査しなくなります。"
                .to_string()
        }
    }
}

/// Japanese rendering of one maintenance title.
pub fn title_ja(title: &MaintenanceTitle) -> String {
    match title {
        MaintenanceTitle::WriteCommitGraph => "commit-graph を書き込む".to_string(),
        MaintenanceTitle::EnableFsmonitor => "ファイルシステムモニターを有効にする".to_string(),
    }
}

/// Japanese rendering of one maintenance recovery block.
pub fn recovery_ja(recovery: &MaintenanceRecovery) -> String {
    match recovery {
        MaintenanceRecovery::WriteCommitGraph => {
            "commit-graph は git が履歴を速くたどるために読むキャッシュにすぎません。元に戻すには削除してください。なくても git は動作します。"
                .to_string()
        }
        MaintenanceRecovery::EnableFsmonitor => {
            "元に戻すには、このリポジトリの config から設定を削除してください。".to_string()
        }
    }
}
