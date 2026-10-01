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
