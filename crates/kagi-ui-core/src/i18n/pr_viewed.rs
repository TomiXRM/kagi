//! EN/JA text for a pull request's per-file "viewed" marks (#351).

use super::{lang, Lang};

/// The file-list header's progress, e.g. `2 / 5 viewed`.
pub fn progress(viewed: usize, total: usize) -> String {
    match lang() {
        Lang::En => format!("{viewed} / {total} viewed"),
        Lang::Ja => format!("{viewed} / {total} 確認済み"),
    }
}

/// The checkbox's tooltip.
pub fn checkbox_tooltip() -> &'static str {
    match lang() {
        Lang::En => "Viewed",
        Lang::Ja => "確認済み",
    }
}

/// Marking changed on screen but could not be stored.
pub fn save_failed(error: &std::io::Error) -> String {
    match lang() {
        Lang::En => format!("Could not save viewed files: {error}"),
        Lang::Ja => format!("確認済みの状態を保存できませんでした: {error}"),
    }
}
