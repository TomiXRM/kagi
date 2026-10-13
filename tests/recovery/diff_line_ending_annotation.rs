//! #1446: identical visible code still explains LF ↔ CRLF replacements.
use crate::diff_split_unicode_badge::{clipboard, copy_body, fixture, frame, mount_size, reveal};
use crate::macos::unmount;
use crate::recovery_layout::GlobalSettings;
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, DiffRow, KagiApp};
use kagi_domain::diff::{DiffLineKind, HunkApproval};
use kagi_ui_core::{
    i18n::{self, Lang},
    theme,
};
use std::path::Path;

pub fn scenario_diff_line_ending_annotation(cx: &mut VisualTestAppContext) {
    let _globals = GlobalSettings::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&["diff_split", "lang", "theme"]);
    for (old_ending, new_ending) in [("\n", "\r\n"), ("\r\n", "\n")] {
        let temp = fixture(
            &format!("token{old_ending}ordinary old\n"),
            &format!("token{new_ending}ordinary new\n"),
        );
        let repo = temp.path().canonicalize().unwrap();
        assert!(!repo.join(".gitattributes").exists());
        let backend = kagi_git::Backend::open(&repo).unwrap();
        let raw = backend.unstaged_file_diff(Path::new("f.txt")).unwrap();
        let approvals: Vec<_> = raw
            .hunks
            .iter()
            .filter_map(|hunk| hunk.approval())
            .collect();
        assert!(!approvals.is_empty());
        assert!(raw
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .any(|line| line.content == format!("token{old_ending}")
                && line.kind == DiffLineKind::Removed));
        assert!(raw
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .any(|line| line.content == format!("token{new_ending}")
                && line.kind == DiffLineKind::Added));
        let (app, window) = mount_size(cx, &repo, (1392., 883.), "apple-light");
        for slug in ["apple-light", "apple-dark"] {
            app.update(cx, |app, cx| app.set_theme(slug, cx));
            for dimensions in [(940., 660.), (1392., 883.)] {
                crate::pr_detail_adaptive::resize(cx, window, dimensions);
                for zoom in [1., 1.67] {
                    theme::set_zoom(zoom);
                    for lang in [Lang::En, Lang::Ja] {
                        i18n::set_lang(lang);
                        for split in [false, true] {
                            theme::set_diff_split(split);
                            frame(cx, &app, window, dimensions);
                            check_endings(
                                cx, &app, window, dimensions, split, old_ending, new_ending,
                                &approvals,
                            );
                        }
                    }
                }
            }
        }
        unmount(cx, app, window);
        assert_eq!(
            backend
                .unstaged_file_diff(Path::new("f.txt"))
                .unwrap()
                .hunks
                .iter()
                .filter_map(|hunk| hunk.approval())
                .collect::<Vec<_>>(),
            approvals,
            "rendering does not change approved raw patch bytes"
        );
    }
    eprintln!("[gui-e2e] PASS diff_line_ending_annotation LF/CRLF both directions unified/split 940/1392 100%/167% EN/JA light/dark");
}

fn check_endings(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    dimensions: (f32, f32),
    split: bool,
    old_ending: &str,
    new_ending: &str,
    approvals: &[HunkApproval],
) {
    let rows = cx.read(|cx| {
        let pane = app.read(cx).ui().main_diff.as_ref().unwrap().read(cx);
        assert_eq!(
            e2e::diff_content::approvals(&pane.view),
            approvals,
            "display retains backend raw approvals"
        );
        pane.view.rows.clone()
    });
    let endings = e2e::diff_content::endings();
    let mut expected = 0;
    for (index, row) in rows.iter().enumerate() {
        if let DiffRow::Line { kind, text, .. } = row {
            assert!(!text.contains('\r'), "raw CR must not be painted");
            let old = *kind == DiffLineKind::Removed;
            let side = if !split {
                "unified"
            } else if old {
                "old"
            } else {
                "new"
            };
            let marker = endings.get(&(side.to_string(), index));
            if text.get(1..) == Some("token") {
                expected += 1;
                let marker =
                    marker.expect("line-ending-only change must describe its side's LF/CRLF");
                let ending = if old { old_ending } else { new_ending };
                let label = match (i18n::lang(), ending) {
                    (Lang::En, "\n") => "LF line ending",
                    (Lang::En, _) => "CRLF line ending",
                    (Lang::Ja, "\n") => "LF 改行",
                    (Lang::Ja, _) => "CRLF 改行",
                };
                assert_eq!(marker.label, label);
                assert_eq!(marker.help, label);
                reveal(cx, app, window, dimensions, index);
                assert!(
                    e2e::control_bounds(window.window_id(), &format!("diff-ending-{side}-{index}"))
                        .is_some(),
                    "annotation must actually render"
                );
            } else {
                assert!(
                    marker.is_none(),
                    "ordinary body change must not claim an ending-only change"
                );
            }
        }
    }
    assert_eq!(expected, 2);
    assert_eq!(endings.len(), expected, "no false ending annotations");
    copy_body(cx, app, window);
    assert_eq!(
        clipboard(cx),
        "token\nordinary old\ntoken\nordinary new\n",
        "code-only clipboard omits annotations and CR"
    );
}
