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
