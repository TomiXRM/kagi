//! Accessible conflict names use operation-dependent role/name labels (ADR-0058).
use super::{lang, Lang, Msg};
impl Msg {
    pub fn conflict_draft_rescued(name: &str) -> String {
        draft_rescued_for(lang(), name)
    }

    pub fn conflict_draft_restore_failed() -> String {
        draft_restore_failed_for(lang()).into()
    }

    pub fn conflict_draft_preserve_failed() -> String {
        draft_preserve_failed_for(lang()).into()
    }

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

fn draft_rescued_for(language: Lang, name: &str) -> String {
    let name = kagi_domain::text_safety::sanitize_control_bytes(name);
    let name = if name.contains('\n') {
        std::borrow::Cow::Owned(name.replace('\n', "\\n"))
    } else {
        std::borrow::Cow::Borrowed(name.as_str())
    };
    match language {
        Lang::En => {
            format!("Draft could not be read; the damaged file was moved aside as {name}")
        }
        Lang::Ja => {
            format!("下書きを読めませんでした。破損したファイルを {name} に移しました")
        }
    }
}

fn draft_restore_failed_for(language: Lang) -> &'static str {
    match language {
        Lang::En => "Could not read saved conflict edits. See Operation Log, then reload to try again.",
        Lang::Ja => "保存した conflict の編集内容を読めませんでした。Operation Log を確認してから再読み込みしてください。",
    }
}

fn draft_preserve_failed_for(language: Lang) -> &'static str {
    match language {
        Lang::En => "Could not save conflict edits. Keep this window open; see Operation Log, then reload.",
        Lang::Ja => "conflict の編集内容を保存できませんでした。このウィンドウを閉じず、Operation Log を確認して再読み込みしてください。",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_draft_restore_preview_is_bounded_in_both_languages() {
        for language in [Lang::En, Lang::Ja] {
            let preview = draft_restore_failed_for(language);
            assert!(preview.contains("Operation Log"));
            assert!(preview.chars().count() < 100);
            assert!(!preview.contains('/') && !preview.contains('\n'));
        }
    }

    #[test]
    fn failed_draft_preservation_preview_is_bounded_in_both_languages() {
        for language in [Lang::En, Lang::Ja] {
            let preview = draft_preserve_failed_for(language);
            assert!(preview.contains("Operation Log"));
            assert!(preview.chars().count() < 100);
            assert!(!preview.contains('/') && !preview.contains('\n'));
        }
        assert!(draft_preserve_failed_for(Lang::En).contains("Keep this window open"));
        assert!(draft_preserve_failed_for(Lang::Ja).contains("閉じず"));
    }

    #[test]
    fn damaged_draft_notice_names_the_preserved_file_in_both_languages() {
        assert_eq!(
            draft_rescued_for(Lang::En, "buffer.json.corrupt.1"),
            "Draft could not be read; the damaged file was moved aside as buffer.json.corrupt.1"
        );
        assert_eq!(
            draft_rescued_for(Lang::Ja, "buffer.json.corrupt.1"),
            "下書きを読めませんでした。破損したファイルを buffer.json.corrupt.1 に移しました"
        );
        assert!(!draft_rescued_for(Lang::En, "buffer.json\n.corrupt").contains('\n'));
    }

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
