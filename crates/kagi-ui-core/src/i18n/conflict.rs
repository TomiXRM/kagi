//! Conflict adoption names, separate from operation-dependent visual side labels.
use super::{lang, Lang, Msg};
impl Msg {
    pub fn conflict_hunk(side: bool, hunk: usize) -> String {
        let side = if side { "ours" } else { "theirs" };
        match lang() {
            Lang::En => format!("Use {side} for hunk {hunk}"),
            Lang::Ja => format!("hunk {hunk} に {side} を採用"),
        }
    }
    pub fn conflict_line(side: bool, line: usize) -> String {
        let side = if side { "ours" } else { "theirs" };
        match lang() {
            Lang::En => format!("Use {side} line {line}"),
            Lang::Ja => format!("{side} の {line} 行目を採用"),
        }
    }
}
