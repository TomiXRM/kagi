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

/// Heading of the reflog section in a selected row's detail.
pub fn reflog_heading(open_start: bool, lookback_secs: i64) -> String {
    match (lang(), open_start) {
        (Lang::En, false) => "Reflog since this worktree's previous operation".into(),
        (Lang::En, true) => format!(
            "Reflog of the last {lookback_secs} s before this operation (its previous operation is not loaded)"
        ),
        (Lang::Ja, false) => "この worktree の前の操作以降の reflog".into(),
        (Lang::Ja, true) => {
            format!("この操作までの {lookback_secs} 秒間の reflog(前の操作は読み込まれていません)")
        }
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
