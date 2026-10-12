//! #1377: informational chips explain symbols without becoming buttons.
use crate::macos::{build_fixture, unmount};
use crate::recovery_layout::draw;
use gpui::{px, size, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::i18n;
use std::{cell::RefCell, rc::Rc};

pub fn scenario_status_chips_semantics(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    for language in ["en", "ja"] {
        std::env::set_var("KAGI_LANG", language);
        i18n::init_lang();
        let captured = Rc::new(RefCell::new(None));
        let output = captured.clone();
        let state = e2e::app_state(fixture.path()).unwrap();
        let win = crate::macos::open_offscreen(cx, size(px(940.), px(660.)), move |window, cx| {
            e2e::mount_root(state, window, cx, &output)
        })
        .into();
        let app = captured.borrow().clone().unwrap();
        cx.run_until_parked();
        for (age, counts, no_upstream) in [
            (720, Some((2, 1)), false),
            (10800, Some((2, 1)), false),
            (870, Some((0, 0)), false),
            (900, Some((2, 3)), false),
            (0, None, true),
            (0, None, false),
        ] {
            app.update(cx, |app, cx| {
                let summary = &mut app.view_mut().status_summary;
                summary.ahead = counts.map(|(ahead, _)| ahead);
                summary.behind = counts.map(|(_, behind)| behind);
                summary.no_upstream = no_upstream;
                summary.upstream_name = if no_upstream {
                    String::new()
                } else {
                    "origin/main".into()
                };
                summary.is_detached = false;
                summary.is_unborn = false;
                summary.has_remote = true;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;
                summary.last_fetch_secs = Some(now - age);
                cx.notify();
            });
            draw(cx, win, (940., 660.));
            let chips = e2e::status_chips();
            assert!(!chips.is_empty());
            for (text, ax, tip) in &chips {
                let ax = ax.as_ref().expect("status chip must have an AX label");
                assert!(!ax.is_empty(), "{text}: empty AX label");
                assert_eq!(
                    tip.as_ref(),
                    Some(ax),
                    "{text}: tooltip matches AX description"
                );
            }
            assert_descriptions(language, age, counts, no_upstream, &chips);
        }
        unmount(cx, app, win);
    }
}

fn assert_descriptions(
    language: &str,
    age: i64,
    counts: Option<(usize, usize)>,
    no_upstream: bool,
    chips: &[(String, Option<String>, Option<String>)],
) {
    if language == "en" {
        let labels: Vec<_> = chips
            .iter()
            .map(|chip| chip.1.as_deref().unwrap())
            .collect();
        if let Some((ahead, behind)) = counts {
            assert!(labels
                .contains(&format!("{ahead} commits ahead, {behind} behind origin/main").as_str()));
        } else if !no_upstream {
            assert!(labels.contains(&"Ahead/behind counts unavailable for origin/main"));
        }
        if no_upstream {
            assert!(labels.contains(&"No upstream"));
        }
        if age == 720 {
            assert!(labels.contains(&"Fetched 12 minutes ago"));
        }
        if age == 870 {
            assert!(labels.contains(&"Fetched 14 minutes ago"));
        }
        if age == 900 {
            assert!(labels.contains(&"Last fetch 15 minutes ago — stale"));
        }
        if age == 10800 {
            assert!(labels.contains(&"Last fetch 3 hours ago — stale"));
            assert!(
                chips.iter().any(|chip| chip.0.contains('!')),
                "stale fetch has a non-colour cue"
            );
        }
    } else {
        let labels: Vec<_> = chips
            .iter()
            .map(|chip| chip.1.as_deref().unwrap())
            .collect();
        if let Some((ahead, behind)) = counts {
            assert!(labels.contains(
                &format!("origin/main より {ahead} commit 先行、{behind} commit 遅れ").as_str()
            ));
        } else if no_upstream {
            assert!(labels.contains(&"upstream がありません"));
        } else {
            assert!(labels.contains(&"origin/main との先行・遅れの commit 数は未取得です"));
        }
        if age == 870 {
            assert!(labels.contains(&"14分前に fetch 済み"));
        }
        if age == 900 {
            assert!(labels.contains(&"最終 fetch は 15分前 — 古い情報です"));
        }
    }
}
