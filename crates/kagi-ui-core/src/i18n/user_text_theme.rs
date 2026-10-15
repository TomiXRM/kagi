//! Theme-file diagnostics at the Settings presentation boundary.
use super::text;
use crate::i18n::{lang, Lang};
use std::borrow::Cow;

pub(super) fn theme_reason(reason: &str) -> Cow<'_, str> {
    if reason.starts_with("missing ") && reason.ends_with("(required without \"extends\")") {
        return Cow::Borrowed(text(
            "This theme is incomplete. Choose a base theme or fill in the missing settings.",
            "テーマの設定が不足しています。ベースとなるテーマを指定するか、不足している設定を追加してください。",
        ));
    }
    let Some((key, _)) = reason
        .strip_prefix('"')
        .and_then(|rest| rest.split_once("\": "))
    else {
        return Cow::Borrowed(reason);
    };
    let (en, ja) = match key {
        "bg_base" => ("Background colour", "背景色"),
        "bg_row_alt" => ("Alternating row colour", "交互の行の色"),
        "surface" | "panel" | "sidebar" | "modal" | "modal_overlay" => {
            ("Panel colour", "パネルの色")
        }
        "selected" | "inactive_selected" | "selection_tint" => ("Selection colour", "選択色"),
        "text_main" | "text_sub" | "text_muted" | "text_label" => ("Text colour", "文字色"),
        "link" => ("Link colour", "リンクの色"),
        "color_head" | "color_branch" | "color_remote" | "color_tag" | "lane_hsl" => {
            ("Graph colours", "グラフの色")
        }
        "color_success" | "color_warning" | "color_blocker" | "color_blocker_muted" => {
            ("Status colour", "状態を示す色")
        }
        "diff_added_bg" | "diff_removed_bg" | "diff_hunk" => ("Diff colour", "diff の色"),
        "change_added" | "change_modified" | "change_deleted" | "change_renamed"
        | "change_typechange" | "change_dir" => ("Changed file colour", "変更ファイルの色"),
        "accent" => ("Accent colour", "アクセントカラー"),
        "dark" => ("Dark appearance", "ダーク表示"),
        "avatar_sat" | "avatar_light" => ("Avatar colour", "アバターの色"),
        "syntax" => ("Code colours", "コードの色"),
        "term_bg"
        | "term_fg"
        | "term_cursor"
        | "term_selection"
        | "term_black"
        | "term_red"
        | "term_green"
        | "term_yellow"
        | "term_blue"
        | "term_magenta"
        | "term_cyan"
        | "term_white"
        | "term_bright_black"
        | "term_bright_red"
        | "term_bright_green"
        | "term_bright_yellow"
        | "term_bright_blue"
        | "term_bright_magenta"
        | "term_bright_cyan"
        | "term_bright_white" => ("Terminal colours", "ターミナルの色"),
        _ => return Cow::Borrowed(reason),
    };
    Cow::Owned(match lang() {
        Lang::En => format!("Invalid value for “{en}”. Check the theme file."),
        Lang::Ja => format!("「{ja}」の値が正しくありません。テーマファイルを確認してください。"),
    })
}
