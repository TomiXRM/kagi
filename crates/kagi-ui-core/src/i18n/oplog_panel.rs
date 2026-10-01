//! EN/JA text for the Operation Log panel's badges and reflog detail (#334).

use super::{lang, Lang};

/// Actor badge for an operation a person ran in the GUI (MCP / CLI are shown
/// as those names in both languages).
pub fn actor_human() -> &'static str {
    match lang() {
        Lang::En => "Human",
        Lang::Ja => "人",
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

/// Heading of the refs the operation recorded moving (#334 slice 2a).
pub fn recorded_heading() -> &'static str {
    match lang() {
        Lang::En => "Recorded — refs this operation moved",
        Lang::Ja => "記録 — この操作が動かした ref",
    }
}

/// A recorded operation that moved no ref (refused, failed, or a no-op).
pub fn recorded_none() -> &'static str {
    match lang() {
        Lang::En => "No ref moved",
        Lang::Ja => "動いた ref はありません",
    }
}

/// A side of a recorded move where the ref did not exist.
pub fn ref_absent() -> &'static str {
    match lang() {
        Lang::En => "(none)",
        Lang::Ja => "(なし)",
    }
}

pub fn reflog_loading() -> &'static str {
    match lang() {
        Lang::En => "Reading reflog…",
        Lang::Ja => "reflog を読み込み中…",
    }
}

pub fn reflog_none() -> &'static str {
    match lang() {
        Lang::En => "No reflog lines in this window",
        Lang::Ja => "この時間帯の reflog はありません",
    }
}

pub fn reflog_unavailable(error: &str) -> String {
    match lang() {
        Lang::En => format!("Reflog unavailable: {error}"),
        Lang::Ja => format!("reflog を読めません: {error}"),
    }
}

/// Marker on a line in a second another operation of the worktree shares.
pub fn reflog_ambiguous() -> &'static str {
    match lang() {
        Lang::En => "same second as another operation — cannot tell which",
        Lang::Ja => "別の操作と同じ秒 — どちらの操作のものか判別できません",
    }
}

/// #334 slice 2b: the selected row's "undo this one operation" button.
pub fn revert_button() -> &'static str {
    match lang() {
        Lang::En => "Revert this operation…",
        Lang::Ja => "この操作を取り消す…",
    }
}

/// #334 slice 2b: the selected row's "put branches back to here" button.
pub fn restore_button() -> &'static str {
    match lang() {
        Lang::En => "Restore to this point…",
        Lang::Ja => "この時点まで戻す…",
    }
}

/// Why both buttons are disabled for an entry without recorded ref moves.
pub fn restore_unavailable() -> &'static str {
    match lang() {
        Lang::En => "This operation has no recorded ref moves, so it cannot be undone exactly.",
        Lang::Ja => "この操作には ref の移動の記録が無いため、正確に戻せません。",
    }
}

/// The second (armed) confirm of an op-revert / restore-to-point.
pub fn restore_armed() -> &'static str {
    match lang() {
        Lang::En => "Really move the branches back",
        Lang::Ja => "本当に branch を戻しますか",
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
pub fn preview_not_loaded(branch: &str, oid: &str) -> String {
    match lang() {
        Lang::En => format!(
            "Preview unavailable: '{branch}' goes back to {oid}, which is not in the loaded history. The restore itself is unaffected."
        ),
        Lang::Ja => format!(
            "プレビューできません: `{branch}` の戻し先 {oid} は読み込み済みの履歴にありません。復元そのものには影響しません。"
        ),
    }
}
