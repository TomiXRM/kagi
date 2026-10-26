//! Accessible conflict names use operation-dependent role/name labels (ADR-0058).
use super::{lang, Lang, Msg};
impl Msg {
    pub fn conflict_draft_rescued() -> String {
        draft_rescued_for(lang()).to_owned()
    }

    pub fn conflict_draft_restore_failed() -> String {
        draft_restore_failed_for(lang()).into()
    }

    pub fn conflict_draft_repository(name: &str) -> String {
        match lang() {
            Lang::En => format!("Repository: {name}"),
            Lang::Ja => format!("リポジトリ: {name}"),
        }
    }

    pub fn conflict_draft_refresh_unavailable() -> &'static str {
        match lang() {
            Lang::En => {
                "The repository for these saved edits is no longer available in this window."
            }
            Lang::Ja => "この保存済み編集のリポジトリは、このウィンドウでは利用できません。",
        }
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

fn draft_rescued_for(language: Lang) -> &'static str {
    match language {
        Lang::En => "Saved conflict edits for this repository were damaged and weren't restored. The damaged copy was kept — see the Operation Log.",
        Lang::Ja => "この repository に保存していた conflict 編集は破損していたため復元していません。破損したコピーは残してあります（詳細は操作ログ）。",
    }
}

fn draft_restore_failed_for(language: Lang) -> &'static str {
    match language {
        Lang::En => "Could not read saved conflict edits.",
        Lang::Ja => "保存した conflict の編集内容を読めませんでした。",
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
            assert!(!preview.contains("Operation Log") && !preview.contains("reload"));
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
    fn damaged_draft_notice_keeps_internal_file_identity_in_the_log_only() {
        assert_eq!(
            draft_rescued_for(Lang::En),
            "Saved conflict edits for this repository were damaged and weren't restored. The damaged copy was kept — see the Operation Log."
        );
        assert_eq!(
            draft_rescued_for(Lang::Ja),
            "この repository に保存していた conflict 編集は破損していたため復元していません。破損したコピーは残してあります（詳細は操作ログ）。"
        );
        for language in [Lang::En, Lang::Ja] {
            let preview = draft_rescued_for(language);
            assert!(!preview.contains("buffer.json") && !preview.contains(".corrupt"));
            assert!(!preview.contains('/') && !preview.contains('\n'));
            assert_ne!(preview, draft_restore_failed_for(language));
        }
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
