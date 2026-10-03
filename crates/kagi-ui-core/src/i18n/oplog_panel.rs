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
    /// The second (armed) confirm of an op-revert / restore-to-point.
    RestoreArmed,
    /// Heading of the card's after-restore graph when it cannot be drawn: no
    /// count of disappearing commits is claimed (#883 review).
    PreviewUnavailableHeading,
    /// A local tag move may point at a tag object, so branch graph projection
    /// is not shown. The restore itself is still planned and verified.
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
            (Lang::En, RestoreArmed) => "Really move these local refs back",
            (Lang::Ja, RestoreArmed) => "本当に local ref を戻しますか",
            (Lang::En, PreviewUnavailableHeading) => "Graph after",
            (Lang::Ja, PreviewUnavailableHeading) => "戻した後のグラフ",
            (Lang::En, PreviewTagChange) => {
                "Preview unavailable: local tags change; annotated tags may point to tag objects instead of commits. The restore itself is unaffected."
            }
            (Lang::Ja, PreviewTagChange) => {
                "プレビューできません: local tag が変わります。annotated tag は commit ではなく tag object を指す場合があります。復元そのものには影響しません。"
            }
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

/// #334 slice 2c: heading of the card's after-restore graph.
pub fn preview_heading(removed: usize) -> String {
    match (lang(), removed) {
        (Lang::En, 0) => "Graph after (no commit disappears)".into(),
        (Lang::En, n) => format!("Graph after ({n} commit(s) no longer on any branch)"),
        (Lang::Ja, 0) => "戻した後のグラフ(消える commit はありません)".into(),
        (Lang::Ja, n) => format!("戻した後のグラフ({n} 個の commit がどの branch からも外れます)"),
    }
}

/// Rows of the after-restore graph not drawn above / below the window.
pub fn preview_more(n: usize) -> String {
    match lang() {
        Lang::En => format!("… {n} more row(s)"),
        Lang::Ja => format!("… ほか {n} 行"),
    }
}

/// A branch goes back to a commit the tab has not loaded: no guessed graph.
/// Says how it can come into view (#888): the commit list's "Load more
/// commits" while the history is cut short; never, for a commit no graph
/// root reaches (a deleted branch's tip) — the roots are every ref
/// `commit_log_with_roots` walks from, PR refs and detached HEADs included.
pub fn preview_not_loaded(branch: &str, oid: &str) -> String {
    match lang() {
        Lang::En => format!(
            "Preview unavailable: '{branch}' goes back to {oid}, which is not in the loaded history. If the commit list is cut short, \"Load more commits\" may bring it in; a commit no ref the graph starts from reaches (branches, tags, remote branches, fetched PRs, detached worktree HEADs, stashes — a deleted branch's tip, for one) is never shown. The restore itself is unaffected."
        ),
        Lang::Ja => format!(
            "プレビューできません: `{branch}` の戻し先 {oid} は読み込み済みの履歴にありません。commit 一覧が途中までなら「commit をさらに読み込む」で表示できることがあります。グラフの起点(branch・tag・remote branch・取得済みの PR・detached な worktree の HEAD・stash)のどこからも届かない commit(削除した branch の先端など)は表示されません。復元そのものには影響しません。"
        ),
    }
}
