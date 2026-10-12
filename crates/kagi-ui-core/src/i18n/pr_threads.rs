//! EN/JA text for review threads on the PR diff (#351).

use super::{lang, Lang};

/// A thread whose anchor no longer matches the head.
pub fn outdated() -> &'static str {
    match lang() {
        Lang::En => "Outdated",
        Lang::Ja => "古い位置",
    }
}

/// A thread marked resolved on GitHub.
pub fn resolved() -> &'static str {
    match lang() {
        Lang::En => "Resolved",
        Lang::Ja => "解決済み",
    }
}

/// Reveal context omitted by the compact review-hunk preview.
pub fn show_more_lines(count: usize) -> String {
    match lang() {
        Lang::En => format!(
            "Show {count} previous line{}",
            if count == 1 { "" } else { "s" }
        ),
        Lang::Ja => format!("前の {count} 行を表示"),
    }
}

pub fn collapse_hunk() -> &'static str {
    match lang() {
        Lang::En => "Collapse",
        Lang::Ja => "折りたたむ",
    }
}
