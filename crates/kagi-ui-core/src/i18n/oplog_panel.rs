//! EN/JA text for the Operation Log panel's badges and reflog detail (#334).
//! Fixed strings are message keys ([`OplogPanelMsg`], reached as
//! `Msg::OplogPanel(..)`); only text with arguments stays a function.

use super::{lang, Lang};

/// The Operation Log panel's fixed strings: one key each in the `Msg`
/// catalog, under [`super::Msg::OplogPanel`] (#871 review).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OplogPanelMsg {
    /// Actor badge for an operation a person ran in the GUI (MCP / CLI are
    /// shown as those names in both languages).
    ActorHuman,
    /// Heading of the refs the operation recorded moving (#334 slice 2a).
    RecordedHeading,
    /// A recorded operation that moved no ref (refused, failed, or a no-op).
    RecordedNone,
    /// A side of a recorded move where the ref did not exist.
    RefAbsent,
    ReflogLoading,
    ReflogNone,
    /// Marker on a line in a second another operation of the worktree shares.
    ReflogAmbiguous,
    /// The selected row's "undo this one operation" button (#334 slice 2b).
    RevertButton,
    /// The selected row's "put local refs back to here" button.
    RestoreButton,
    /// Why both buttons are disabled for an entry without recorded ref moves.
    RestoreUnavailable,
    /// First confirmation on the restore card.
    RestoreConfirm,
    CopyCommand,
    /// The typed ref list and unchanged-state section.
    RestoreRefs,
    RestoreDelete,
    RestoreUnchanged,
    RestoreAfter,
    RestoreWorkingTree,
    RestoreIndex,
    RestoreUntracked,
    RestoreStash,
    RestoreRemotes,
    RestoreExternalTags,
    RestoreCheckedOut,
    RestoreCheckedOutDirty,
    /// Heading of the card's after-restore graph when it cannot be drawn: no
    /// count of disappearing commits is claimed (#883 review).
    PreviewUnavailableHeading,
    /// A changed tag can point at an annotated tag object, not a commit row.
    PreviewTagChange,
}

impl OplogPanelMsg {
    pub(crate) fn t_for(self, language: Lang) -> &'static str {
        use OplogPanelMsg::*;
        match (language, self) {
            (Lang::En, ActorHuman) => "Human",
            (Lang::Ja, ActorHuman) => "Human",
            (Lang::En, RecordedHeading) => "Recorded — refs this operation moved",
            (Lang::Ja, RecordedHeading) => "記録 — この操作が動かした ref",
            (Lang::En, RecordedNone) => "No ref moved",
            (Lang::Ja, RecordedNone) => "動いた ref はありません",
            (Lang::En, RefAbsent) => "(none)",
            (Lang::Ja, RefAbsent) => "(なし)",
            (Lang::En, ReflogLoading) => "Reading reflog…",
            (Lang::Ja, ReflogLoading) => "reflog を読み込み中…",
            (Lang::En, ReflogNone) => "No reflog lines in this window",
            (Lang::Ja, ReflogNone) => "この時間帯の reflog はありません",
            (Lang::En, ReflogAmbiguous) => "same second as another operation — cannot tell which",
            (Lang::Ja, ReflogAmbiguous) => "別の操作と同じ秒 — どちらの操作のものか判別できません",
            (Lang::En, RevertButton) => "Revert this operation…",
            (Lang::Ja, RevertButton) => "この操作を取り消す…",
            (Lang::En, RestoreButton) => "Restore to this point…",
            (Lang::Ja, RestoreButton) => "この時点まで戻す…",
            (Lang::En, RestoreUnavailable) => {
                "This operation has no recorded ref moves, so it cannot be undone exactly."
            }
            (Lang::Ja, RestoreUnavailable) => {
                "この操作には ref の移動の記録が無いため、正確に戻せません。"
            }
            (Lang::En, RestoreConfirm) => "Restore",
            (Lang::Ja, RestoreConfirm) => "戻す",
            (Lang::En, CopyCommand) => "Copy command",
            (Lang::Ja, CopyCommand) => "コマンドをコピー",
            (Lang::En, RestoreRefs) => "REFS",
            (Lang::Ja, RestoreRefs) => "REF",
            (Lang::En, RestoreDelete) => "delete",
            (Lang::Ja, RestoreDelete) => "削除",
            (Lang::En, RestoreUnchanged) => "UNCHANGED",
            (Lang::Ja, RestoreUnchanged) => "変更なし",
            (Lang::En, RestoreAfter) => "AFTER",
            (Lang::Ja, RestoreAfter) => "戻した後",
            (Lang::En, RestoreWorkingTree) => "working tree",
            (Lang::Ja, RestoreWorkingTree) => "作業ツリー",
            (Lang::En, RestoreIndex) => "index",
            (Lang::Ja, RestoreIndex) => "index",
            (Lang::En, RestoreUntracked) => "untracked",
            (Lang::Ja, RestoreUntracked) => "未追跡",
            (Lang::En, RestoreStash) => "stash",
            (Lang::Ja, RestoreStash) => "stash",
            (Lang::En, RestoreRemotes) => "remotes",
            (Lang::Ja, RestoreRemotes) => "remote",
            (Lang::En, RestoreExternalTags) => "tags moved outside Kagi",
            (Lang::Ja, RestoreExternalTags) => "Kagi 外での tag の変更",
            (Lang::En, RestoreCheckedOut) => "branch moves; files and index stay",
            (Lang::Ja, RestoreCheckedOut) => "branch のみ移動、ファイルと index はそのまま",
            (Lang::En, RestoreCheckedOutDirty) => "branch moves; uncommitted changes stay",
            (Lang::Ja, RestoreCheckedOutDirty) => "branch のみ移動、未 commit の変更はそのまま",
            (Lang::En, PreviewUnavailableHeading) => "Graph after",
            (Lang::Ja, PreviewUnavailableHeading) => "戻した後のグラフ",
            (Lang::En, PreviewTagChange) => "No preview: local tags change",
            (Lang::Ja, PreviewTagChange) => "プレビューなし: local tag が変更されます",
        }
    }
}

/// Heading of the estimated (time-window) reflog section, shown for an entry
/// that carries no recorded ref moves (#334 slice 1).
pub fn reflog_heading(open_start: bool, lookback_secs: i64) -> String {
    match (lang(), open_start) {
        (Lang::En, false) => {
            "Estimated — reflog since this worktree's previous operation".into()
        }
        (Lang::En, true) => format!(
            "Estimated — reflog of the last {lookback_secs} s before this operation (its previous operation is not loaded)"
        ),
        (Lang::Ja, false) => "推定 — この worktree の前の操作以降の reflog".into(),
        (Lang::Ja, true) => format!(
            "推定 — この操作までの {lookback_secs} 秒間の reflog(前の操作は読み込まれていません)"
        ),
    }
}

pub fn reflog_unavailable(error: &str) -> String {
    match lang() {
        Lang::En => format!("Reflog unavailable: {error}"),
        Lang::Ja => format!("reflog を読めません: {error}"),
    }
}

/// Compact count chip: only the loaded commits proved to leave every ref.
pub fn preview_heading(removed: usize) -> String {
    match lang() {
        Lang::En => format!(
            "{removed} commit{} off any branch",
            if removed == 1 { "" } else { "s" }
        ),
        Lang::Ja => format!("{removed} 個の commit がどの branch からも外れます"),
    }
}

pub fn restore_confirm(count: usize) -> String {
    match lang() {
        Lang::En => format!("Confirm · {count} refs"),
        Lang::Ja => format!("確認 · {count} refs"),
    }
}

pub fn restore_command_lines(count: usize) -> String {
    match lang() {
        Lang::En => format!(
            "git update-ref --stdin · {count} line{}",
            if count == 1 { "" } else { "s" }
        ),
        Lang::Ja => format!("git update-ref --stdin · {count} 行"),
    }
}

pub fn restore_checked_out(branch: &str, dirty: bool) -> String {
    let suffix = if dirty {
        OplogPanelMsg::RestoreCheckedOutDirty
    } else {
        OplogPanelMsg::RestoreCheckedOut
    };
    match lang() {
        Lang::En => format!("{branch} is checked out · {}", suffix.t_for(Lang::En)),
        Lang::Ja => format!("{branch} はチェックアウト中 · {}", suffix.t_for(Lang::Ja)),
    }
}

/// Rows of the after-restore graph not drawn above / below the window.
pub fn preview_more(n: usize) -> String {
    match lang() {
        Lang::En => format!("… {n} more row(s)"),
        Lang::Ja => format!("… ほか {n} 行"),
    }
}

/// Keep the card to one neutral line; the warnings name the ref and target.
pub fn preview_not_loaded() -> &'static str {
    match lang() {
        Lang::En => "No preview: target commit is outside loaded history",
        Lang::Ja => "プレビューなし: 戻し先の commit は読み込み済みの履歴外",
    }
}
