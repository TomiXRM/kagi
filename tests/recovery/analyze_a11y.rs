//! Analyze's renderer, real keyboard paths, metric values and modal ownership.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{px, size, AnyWindowHandle, Entity, Role, VisualTestAppContext};
use kagi::ui::{ecosystem::EcosystemView, KagiApp};
use kagi_domain::{
    activity::Granularity,
    hotspot::{CommitChanges, EcosystemMode, FileChange, FileMetric, RawEcosystem},
};
use kagi_ui_core::i18n::{Lang, Msg};
use kagi_ui_ecosystem::e2e;
use kagi_ui_ecosystem::{CachedMine, ExportFormat};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    e2e::clear();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}
fn metrics(count: usize) -> Vec<FileMetric> {
    let mut files: Vec<_> = (0..count)
        .map(|i| FileMetric {
            path: format!("src/file-{i:04}.rs"),
            commits: (i % 4 + 1) as u32,
            loc: (i % 100) as u32,
            insertions: 1,
            deletions: 0,
            risk: [0.0, 0.5, 1.0, 0.73][i % 4],
        })
        .collect();
    files.sort_by(|a, b| b.risk.total_cmp(&a.risk));
    files
}
fn matrix(
    cx: &mut VisualTestAppContext,
    check: impl Fn(&mut VisualTestAppContext, &Entity<KagiApp>, &Entity<EcosystemView>, AnyWindowHandle),
) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom", "theme"]);
    let original = (
        kagi_ui_core::i18n::lang(),
        kagi_ui_core::theme::zoom(),
        kagi_ui_core::theme::theme().slug.to_string(),
    );
    for lang in [Lang::En, Lang::Ja] {
        for zoom in [1.0, 1.67] {
            for theme in ["apple-light", "apple-dark"] {
                for dimensions in [(940., 660.), (1392., 883.)] {
                    let fixture = build_fixture();
                    let (app, window) = mount(cx, fixture.path());
                    kagi_ui_core::theme::set_zoom(zoom);
                    app.update(cx, |app, cx| {
                        app.set_lang(lang, cx);
                        app.set_theme(theme, cx);
                        let head = app.view().head_oid.clone();
                        app.ui_mut().unwrap().ecosystem_cache =
                            Some(CachedMine { raw: raw(), head });
                        app.open_ecosystem_view(cx);
                    });
                    cx.update_window(window, |_, w, _| {
                        w.resize(size(px(dimensions.0), px(dimensions.1)))
                    })
                    .unwrap();
                    cx.run_until_parked();
                    let view = cx.read(|cx| app.read(cx).ui().ecosystem.clone()).unwrap();
                    view.update(cx, |v, cx| v.install_metrics_for_e2e(metrics(8), cx));
                    draw(cx, window);
                    check(cx, &app, &view, window);
                    drop(view);
                    unmount(cx, app, window);
                }
            }
        }
    }
    kagi_ui_core::i18n::set_lang(original.0);
    kagi_ui_core::theme::set_zoom(original.1);
    kagi_ui_core::theme::set_active(&original.2);
    cx.update(kagi_ui_core::theme::sync_gpui_component_theme);
}

fn raw() -> RawEcosystem {
    let files = metrics(4);
    RawEcosystem {
        loc: files.iter().map(|f| (f.path.clone(), f.loc)).collect(),
        commits: vec![CommitChanges {
            time: 1,
            author: "fixture@example.test".into(),
            files: files
                .iter()
                .map(|f| FileChange {
                    path: f.path.clone(),
                    insertions: 1,
                    deletions: 0,
                })
                .collect(),
        }],
    }
}

fn focus(
    cx: &mut VisualTestAppContext,
    view: &Entity<EcosystemView>,
    window: AnyWindowHandle,
    id: &str,
) {
    cx.update_window(window, |_, w, cx| {
        view.update(cx, |v, cx| v.focus_control_for_e2e(id, w, cx))
    })
    .unwrap();
    draw(cx, window);
}

fn selector(
    cx: &mut VisualTestAppContext,
    view: &Entity<EcosystemView>,
    window: AnyWindowHandle,
    group: &str,
    slot: usize,
) {
    cx.update_window(window, |_, w, cx| {
        view.update(cx, |v, cx| v.focus_selector_for_e2e(group, slot, w, cx))
    })
    .unwrap();
    draw(cx, window);
}

fn key(cx: &mut VisualTestAppContext, window: AnyWindowHandle, key: &str) {
    crate::keyboard_nav::keys(cx, window, key);
    draw(cx, window);
}
pub fn scenario_analyze_a11y_controls(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, app, view, window| {
        named_controls(window);
        tab_controls(cx, view, window);
        selector(cx, view, window, "mode", 0);
        let bounds = e2e::bounds(window.window_id(), "eco-mode-Hotspots").unwrap();
        key(cx, window, "right");
        assert_eq!(
            cx.read(|cx| view.read(cx).data_for_e2e().mode),
            EcosystemMode::Coupling
        );
        assert_eq!(
            bounds,
            e2e::bounds(window.window_id(), "eco-mode-Hotspots").unwrap(),
            "focus does not move bounds"
        );
        let row = e2e::node("eco-coup-row-1").unwrap();
        assert_eq!((row.role, row.expanded), (Role::Button, Some(false)));
        assert!(!row.name.is_empty());
        focus(cx, view, window, "eco-coup-row-1");
        key(cx, window, "enter");
        assert_eq!(e2e::node("eco-coup-row-1").unwrap().expanded, Some(true));
        key(cx, window, "space");
        assert_eq!(e2e::node("eco-coup-row-1").unwrap().expanded, Some(false));
        selector(cx, view, window, "format", 0);
        key(cx, window, "right");
        assert_eq!(
            cx.read(|cx| view.read(cx).data_for_e2e().export_format),
            ExportFormat::Json
        );
        selector(cx, view, window, "period", 4);
        key(cx, window, "left");
        assert_eq!(
            cx.read(|cx| view.read(cx).data_for_e2e().granularity),
            Granularity::Year
        );
        selector(cx, view, window, "mode", 1);
        key(cx, window, "end");
        assert!(e2e::node("eco-copy").unwrap().disabled);
        cx.update(|cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                "disabled-copy-sentinel".into(),
            ))
        });
        focus(cx, view, window, "eco-copy");
        key(cx, window, "enter");
        key(cx, window, "space");
        assert_eq!(
            cx.read_from_clipboard().and_then(|i| i.text()).as_deref(),
            Some("disabled-copy-sentinel")
        );
        copy_close(cx, app, view, window);
    });
}

fn named_controls(window: AnyWindowHandle) {
    let mode =
        e2e::node("eco-mode-Hotspots").expect("#1265 mode has rendered accessibility semantics");
    assert_eq!(
        (mode.role, mode.name.as_str(), mode.selected),
        (Role::Tab, "Hotspots", Some(true))
    );
    for (id, name) in [
        ("eco-copy", Msg::EcoCopyDiagnostic.t()),
        ("eco-help", Msg::EcoHelpTitle.t()),
        ("eco-close", Msg::RemoteClose.t()),
    ] {
        let node = e2e::node(id).expect("named button rendered");
        assert_eq!((node.role, node.name.as_str()), (Role::Button, name));
        assert!(f32::from(e2e::bounds(window.window_id(), id).unwrap().size.height) >= 20.);
    }
    for (id, name) in [
        ("eco-modes", Msg::EcoMode.t()),
        ("eco-periods", Msg::EcoPeriod.t()),
        ("eco-views", Msg::EcoView.t()),
        ("eco-formats", Msg::EcoExportFormat.t()),
    ] {
        let node = e2e::node(id).expect("named selector group rendered");
        assert_eq!((node.role, node.name.as_str()), (Role::TabList, name));
    }
    let choices = e2e::choices();
    for prefix in ["eco-mode-", "eco-gran-", "eco-view-", "eco-fmt-"] {
        assert_eq!(
            choices
                .iter()
                .filter(|(id, node)| id.starts_with(prefix) && node.selected == Some(true))
                .count(),
            1,
            "one selected choice in {prefix}"
        );
    }
    for (id, node) in choices {
        assert!(!node.name.is_empty());
        assert!(e2e::bounds(window.window_id(), &id).unwrap().size.height >= px(20.));
    }
}

fn tab_controls(
    cx: &mut VisualTestAppContext,
    view: &Entity<EcosystemView>,
    window: AnyWindowHandle,
) {
    selector(cx, view, window, "mode", 0);
    for group in ["view", "period", "format"] {
        key(cx, window, "tab");
        cx.update_window(window, |_, w, cx| {
            assert!(
                view.read(cx).selector_focused_for_e2e(group, w).is_some(),
                "Tab reaches {group}"
            )
        })
        .unwrap();
    }
    for id in ["eco-copy", "eco-help", "eco-close"] {
        key(cx, window, "tab");
        cx.update_window(window, |_, w, cx| {
            assert!(
                view.read(cx).control_focused_for_e2e(id, w),
                "Tab reaches {id}"
            )
        })
        .unwrap();
    }
}

fn copy_close(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    view: &Entity<EcosystemView>,
    window: AnyWindowHandle,
) {
    selector(cx, view, window, "mode", 0);
    key(cx, window, "home");
    selector(cx, view, window, "period", 4);
    key(cx, window, "end");
    selector(cx, view, window, "format", 0);
    key(cx, window, "end");
    let expected = cx.read(|cx| {
        kagi_domain::hotspot_report::render(
            view.read(cx).data_for_e2e().ecosystem.as_ref().unwrap(),
            30,
            kagi_domain::hotspot_report::ReportFormat::Json,
        )
    });
    focus(cx, view, window, "eco-copy");
    key(cx, window, "space");
    assert_eq!(
        cx.read_from_clipboard().and_then(|i| i.text()),
        Some(expected)
    );
    focus(cx, view, window, "eco-close");
    key(cx, window, "enter");
    assert!(cx.read(|cx| app.read(cx).ui().ecosystem.is_none()));
    cx.update_window(window, |_, w, cx| {
        assert!(app.read(cx).root_focus.as_ref().unwrap().is_focused(w))
    })
    .unwrap();
}
pub fn scenario_analyze_a11y_risk(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, _, view, window| {
        for (i, f) in metrics(8).iter().enumerate() {
            let node =
                e2e::node(&format!("eco-risk-{}", i + 1)).expect("#1266 risk value is rendered");
            assert_eq!(node.text, format!("{:.2}", f.risk));
            assert!(node.name.contains(&format!("{:.2}", f.risk)));
            assert_eq!(node.role, Role::Label);
            assert_eq!(node.name, kagi_ui_core::i18n::analyze::risk_label(f.risk));
            let row = e2e::node(&format!("eco-hot-row-{}", i + 1)).unwrap();
            assert_eq!(row.role, Role::ListItem);
            assert_eq!(row.name, kagi_ui_core::i18n::analyze::metric_description(f));
        }
        assert_eq!(
            e2e::node("eco-risk-scale").unwrap().text,
            kagi_ui_core::i18n::analyze::scale_legend(false)
        );
        view.update(cx, |v, cx| v.set_map(true, cx));
        draw(cx, window);
        assert_eq!(
            e2e::node("eco-risk-scale").unwrap().text,
            kagi_ui_core::i18n::analyze::scale_legend(true)
        );
        for f in metrics(8) {
            let tile = e2e::node(&format!("eco-tile-{}", f.path)).unwrap();
            assert_eq!(tile.role, Role::Label);
            assert_eq!(
                tile.name,
                kagi_ui_core::i18n::analyze::metric_description(&f)
            );
        }
    });
}
pub fn scenario_analyze_a11y_truncation(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, _, view, window| {
        for count in [0, 149, 150, 151, 299, 300, 301, 1000, 1234] {
            for map in [false, true] {
                view.update(cx, |v, cx| {
                    v.install_metrics_for_e2e(metrics(count), cx);
                    v.set_map(map, cx);
                });
                draw(cx, window);
                let cap = if map { 150 } else { 300 };
                let notice = e2e::node("eco-truncation");
                assert_eq!(
                    e2e::count(if map { "eco-tile-" } else { "eco-hot-row-" }),
                    count.min(cap),
                    "renderer count respects cap"
                );
                if count > cap {
                    let notice =
                        notice.expect("#1267 truncated presentation announces its coverage");
                    assert_eq!(notice.role, Role::Status);
                    assert_eq!(
                        notice.name,
                        kagi_ui_core::i18n::analyze::truncation(cap, count)
                    );
                    if count == 1234 {
                        let expected = match kagi_ui_core::i18n::lang() {
                            Lang::En => {
                                format!("Showing {cap} of 1,234 — narrow the period or filter")
                            }
                            Lang::Ja => format!(
                                "1,234 件中 {cap} 件を表示 — 期間または filter を絞ってください"
                            ),
                        };
                        assert_eq!(notice.text, expected);
                    }
                    let bounds = e2e::bounds(window.window_id(), "eco-truncation").unwrap();
                    cx.update_window(window, |_, w, _| {
                        assert!(bounds.right() <= w.viewport_size().width);
                        assert!(bounds.bottom() <= w.viewport_size().height);
                    })
                    .unwrap();
                } else {
                    assert!(notice.is_none(), "no notice at/below cap");
                }
            }
        }
        row_removal(cx, view, window);
    });
}

fn row_removal(
    cx: &mut VisualTestAppContext,
    view: &Entity<EcosystemView>,
    window: AnyWindowHandle,
) {
    view.update(cx, |v, cx| {
        v.set_map(false, cx);
        v.install_metrics_for_e2e(metrics(1), cx);
    });
    draw(cx, window);
    focus(cx, view, window, "eco-hot-row-1");
    view.update(cx, |v, cx| v.install_metrics_for_e2e(metrics(0), cx));
    draw(cx, window);
    cx.update_window(window, |_, w, cx| {
        assert!(
            !view.read(cx).control_focused_for_e2e("eco-hot-row-1", w),
            "disappearing data releases row focus"
        )
    })
    .unwrap();
}
pub fn scenario_analyze_a11y_help(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, app, view, window| {
        let help = e2e::bounds(window.window_id(), "eco-help").expect("help is drawn");
        cx.simulate_click(window, help.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        draw(cx, window);
        let dialog = e2e::node("eco-help-card").expect("#1268 help is a named dialog");
        assert_eq!(
            (dialog.role, dialog.name.as_str()),
            (Role::Dialog, Msg::EcoHelpTitle.t())
        );
        assert!(cx.read(|cx| app.read(cx).analyze_help_modal().is_some()));
        let actual = kagi::ui::dialog_a11y::recorded_dialog("eco-help-card").unwrap();
        assert_eq!(
            (actual.role, actual.label.as_str()),
            (Role::Dialog, Msg::EcoHelpTitle.t())
        );
        help_geometry(cx, window);
        let in_dialog = |cx: &mut VisualTestAppContext| {
            cx.update_window(window, |_, w, cx| {
                let app = app.read(cx);
                let trap = app.modal_focus.as_ref().unwrap();
                assert!(
                    trap.is_focused(w) || trap.contains_focused(w, cx),
                    "focus trapped within help"
                );
            })
            .unwrap()
        };
        in_dialog(cx);
        key(cx, window, "tab");
        in_dialog(cx);
        key(cx, window, "shift-tab");
        in_dialog(cx);
        cx.simulate_keystrokes(window, "escape");
        cx.run_until_parked();
        draw(cx, window);
        assert!(e2e::node("eco-help-card").is_none());
        assert!(cx.read(|cx| app.read(cx).analyze_help_modal().is_none()));
        assert!(
            cx.read(|cx| app
                .read(cx)
                .ui()
                .ecosystem
                .as_ref()
                .is_some_and(|pane| pane.entity_id() == view.entity_id())),
            "Escape closes only Help"
        );
        cx.update_window(window, |_, w, cx| {
            assert!(
                view.read(cx).control_focused_for_e2e("eco-help", w),
                "focus returns to the exact Help opener"
            );
        })
        .unwrap();
    });
}

fn help_geometry(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    let card = kagi::ui::e2e::control_bounds(window.window_id(), "eco-help-card").unwrap();
    let close = kagi::ui::e2e::control_bounds(window.window_id(), "eco-help-close").unwrap();
    cx.update_window(window, |_, w, _| {
        assert!(card.left() >= px(0.) && card.top() >= px(0.));
        assert!(
            card.right() <= w.viewport_size().width && card.bottom() <= w.viewport_size().height
        );
        assert!(
            close.top() >= card.top() && close.bottom() <= card.bottom(),
            "Close {close:?} must fit Help {card:?}"
        );
        assert!(close.size.width >= px(20.) && close.size.height >= px(20.));
    })
    .unwrap();
    let body = kagi::ui::e2e::control_bounds(window.window_id(), "eco-help-body").unwrap();
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: body.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-10000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    draw(cx, window);
    let last = kagi::ui::e2e::control_bounds(window.window_id(), "eco-help-last").unwrap();
    assert!(
        last.bottom() <= body.bottom() + px(1.),
        "last Help section can be read"
    );
}
