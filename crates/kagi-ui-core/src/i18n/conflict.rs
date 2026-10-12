//! Accessible conflict names use operation-dependent role/name labels (ADR-0058).
use super::{lang, Lang, Msg};
impl Msg {
    pub fn conflict_side(role: &str, name: &str) -> String {
        format!(
            "{role} ({})",
            kagi_domain::text_safety::sanitize_control_bytes(name)
        )
    }
    pub fn conflict_file(side: &str) -> String {
        match lang() {
            Lang::En => format!("Use all: {side}"),
            Lang::Ja => format!("{side} をすべて採用"),
        }
    }
    pub fn conflict_hunk(side: &str, hunk: usize) -> String {
        match lang() {
            Lang::En => format!("Use {side} for hunk {hunk}"),
            Lang::Ja => format!("hunk {hunk} に {side} を採用"),
        }
    }
    pub fn conflict_line(side: &str, line: usize) -> String {
        match lang() {
            Lang::En => format!("Use {side} line {line}"),
            Lang::Ja => format!("{side} の {line} 行目を採用"),
        }
    }
    pub fn conflict_order(side: &str) -> String {
        match lang() {
            Lang::En => format!("Order: {side} first"),
            Lang::Ja => format!("順序: {side} を先に"),
        }
    }
    pub fn conflict_selected_order(side: &str) -> String {
        match lang() {
            Lang::En => format!("Selected output order: {side} first"),
            Lang::Ja => format!("選択中の出力順序: {side} を先に"),
        }
    }
}
