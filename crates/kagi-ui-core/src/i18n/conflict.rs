//! Accessible conflict names use operation-dependent role/name labels (ADR-0058).
use super::{lang, Lang, Msg};
impl Msg {
    /// A stopped merge/rebase is not a completed operation. A failed status
    /// read must not invent a conflict count.
    pub fn conflict_stopped(count: Option<usize>) -> String {
        stopped_for(lang(), count)
    }
    pub fn conflict_counts(total: usize, resolved: usize) -> String {
        match lang() {
            Lang::En => format!(
                "{total} {} · {resolved} resolved",
                if total == 1 { "conflict" } else { "conflicts" }
            ),
            Lang::Ja => format!("conflict {total} 件 · {resolved} 解決済み"),
        }
    }

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

fn stopped_for(language: Lang, count: Option<usize>) -> String {
    match (language, count) {
        (Lang::En, Some(1)) => "Stopped: 1 conflicted file".into(),
        (Lang::En, Some(count)) => format!("Stopped: {count} conflicted files"),
        (Lang::Ja, Some(count)) => format!("停止: conflict ファイル {count} 件"),
        (Lang::En, None) => "Stopped due to conflicts; file count unavailable".into(),
        (Lang::Ja, None) => "conflict のため停止: ファイル数は取得できません".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopped_conflict_notice_reports_observed_count_in_both_languages() {
        for (language, stopped, conflict) in [
            (Lang::En, "Stopped", "conflict"),
            (Lang::Ja, "停止", "conflict"),
        ] {
            for count in [1, 3, 12] {
                let notice = stopped_for(language, Some(count));
                assert!(notice.contains(stopped));
                assert!(notice.contains(conflict));
                assert!(notice.contains(&count.to_string()));
            }
            let unavailable = stopped_for(language, None);
            assert!(unavailable.contains(stopped));
            assert!(unavailable.contains(conflict));
            assert!(!unavailable.contains('0'));
        }
    }
}
