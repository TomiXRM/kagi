//! Relative age language and local cleanup calendar regressions (#1454/#1455).
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{detail_panel, list_a11y, theme};
use kagi_ui_core::{
    i18n::{self, Lang},
    time,
};

const AGES: [(i64, &str, &str, &str); 7] = [
    (0, "たった今", "just now", "just now"),
    (59, "たった今", "just now", "just now"),
    (60, "1分前", "1m ago", "1 minute ago"),
    (3600, "1時間前", "1h ago", "1 hour ago"),
    (86400, "1日前", "1d ago", "1 day ago"),
    (86400 * 30, "1か月前", "1mo ago", "1 month ago"),
    (86400 * 365, "1年前", "1y ago", "1 year ago"),
];

pub fn scenario_relative_time_ja(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom"]);
    let original = (i18n::lang(), theme::zoom());
    let fixture = build_fixture();
    for dimensions in [(940., 660.), (1392., 883.)] {
        for zoom in [1., 1.67] {
            let (app, window) = mount(cx, fixture.path());
            theme::set_zoom(zoom);
            crate::pr_detail_adaptive::resize(cx, window, dimensions);
            cx.run_until_parked();
            let identity = cx.read(|cx| app.read(cx).view().rows.as_ptr());
            for language in [Lang::En, Lang::Ja, Lang::En] {
                app.update(cx, |app, cx| app.set_lang(language, cx));
                cx.update_window(window, |_, window, cx| {
                    window.refresh();
                    window.draw(cx).clear();
                })
                .unwrap();
                assert_eq!(
                    cx.read(|cx| app.read(cx).view().rows.as_ptr()),
                    identity,
                    "language switch must not reload the read model"
                );
                for (seconds, japanese, english, spoken) in AGES {
                    let age = time::relative_time(0, seconds);
                    let description = time::relative_time_description(0, seconds);
                    let label = list_a11y::pr_row_label(
                        42,
                        "Fixture",
                        "Open",
                        "author",
                        "topic",
                        "main",
                        "",
                        &description,
                    );
                    assert_eq!(
                        age,
                        if language == Lang::Ja {
                            japanese
                        } else {
                            english
                        }
                    );
                    assert_eq!(
                        description,
                        if language == Lang::Ja {
                            japanese
                        } else {
                            spoken
                        }
                    );
                    assert!(label.contains(&description));
                    if language == Lang::Ja {
                        assert_eq!(time::relative_time_short(0, seconds), japanese);
                        assert!(!label.contains("ago") && !label.contains("just now"));
                    }
                }
                assert_eq!(time::relative_time(60, 0), time::relative_time(0, 0));
            }
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(original.0);
    theme::set_zoom(original.1);
    cx.update(theme::sync_gpui_component_theme);
    eprintln!("[gui-e2e] PASS relative_time_ja");
}

struct TimeZoneGuard(Option<std::ffi::OsString>);
impl Drop for TimeZoneGuard {
    fn drop(&mut self) {
        match &self.0 {
            Some(zone) => std::env::set_var("TZ", zone),
            None => std::env::remove_var("TZ"),
        }
    }
}

pub fn scenario_cleanup_local_date(cx: &mut VisualTestAppContext) {
    let _zone = TimeZoneGuard(std::env::var_os("TZ"));
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "cleanup-time"]);
    let (app, window) = mount(cx, fixture.path());
    cx.run_until_parked();
    let original = cx.read(|cx| app.read(cx).view().cleanup_rows.clone());
    assert!(!original.is_empty(), "real merged branch fixture");
    for zone in ["Asia/Tokyo", "UTC", "America/New_York"] {
        std::env::set_var("TZ", zone);
        // chrono's local-offset cache expires after one second.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        if zone == "America/New_York" {
            for (epoch, clock) in [
                (1710052200, "2024-03-10 01:30"),
                (1710055800, "2024-03-10 03:30"),
                (1730611800, "2024-11-03 01:30"),
                (1730615400, "2024-11-03 01:30"),
            ] {
                assert_eq!(detail_panel::format_local(epoch), clock);
            }
        }
        for epoch in [
            -60, 1800, 84600, 951782400, 1710052200, 1710055800, 1730611800, 1730615400,
        ] {
            let date = kagi::ui::branch_cleanup::format_date_for_e2e(epoch);
            let detail = detail_panel::format_local(epoch);
            assert_eq!(
                date,
                detail.split_once(' ').unwrap().0,
                "cleanup and detail calendar date: {zone} at {epoch}"
            );
            if epoch == -60 {
                assert_eq!(
                    date,
                    if zone == "Asia/Tokyo" {
                        "1970-01-01"
                    } else {
                        "1969-12-31"
                    }
                );
            }
        }
        cx.read(|cx| {
            let rows = &app.read(cx).view().cleanup_rows;
            assert_eq!(rows.len(), original.len());
            for (row, before) in rows.iter().zip(&original) {
                assert_eq!(
                    (
                        &row.name,
                        row.merged_at,
                        row.stale,
                        row.deletable,
                        row.bulk_deletable
                    ),
                    (
                        &before.name,
                        before.merged_at,
                        before.stale,
                        before.deletable,
                        before.bulk_deletable
                    )
                );
            }
        });
    }
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS cleanup_local_date");
}
