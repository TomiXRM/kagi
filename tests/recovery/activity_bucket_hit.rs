//! Painted-point inspection geometry regression (#1510).
use crate::activity_a11y::{draw, focus};
use crate::macos::{build_fixture, mount, unmount};
use gpui::{px, AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, i18n::Lang, theme, BottomTab, KagiApp};
use kagi_domain::activity::Granularity;

fn inspect(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    draw(cx, window);
    let points = e2e::activity::painted_points();
    let data = cx.read(|cx| {
        let app = app.read(cx);
        app.view().activity.get(app.activity_granularity).clone()
    });
    assert_eq!(points.len(), data.buckets.len() * 2);
    for (i, merges, point) in points {
        cx.simulate_mouse_move(window, point, None, Modifiers::none());
        draw(cx, window);
        assert_eq!(
            cx.read(|cx| app.read(cx).activity_hover),
            Some(i),
            "painted bucket {i}, merges={merges}, count={}, point={point:?}",
            data.buckets.len()
        );
        let pointer_readout = e2e::activity::readout();
        let bucket = &data.buckets[i];
        assert!(pointer_readout.ends_with(&format!(
            "{} commits, {} merges",
            bucket.commits, bucket.merges
        )));
        assert_eq!(
            e2e::activity::node(&format!("activity-bucket-{i}"))
                .unwrap()
                .name,
            kagi_ui_core::i18n::activity::bucket(
                &data.bucket_date(i),
                bucket.commits,
                bucket.merges,
                bucket.authors
            )
        );
        if i == 0 {
            focus(cx, window, "activity-bucket-0");
        }
        crate::keyboard_nav::keys(cx, window, if i == 0 { "home" } else { "right" });
        draw(cx, window);
        cx.update_window(window, |_, window, _| {
            assert!(e2e::activity::node(&format!("activity-bucket-{i}"))
                .unwrap()
                .focus
                .unwrap()
                .is_focused(window));
        })
        .unwrap();
        assert_eq!(
            e2e::activity::readout(),
            pointer_readout,
            "keyboard and pointer date/count agree"
        );
        let bounds =
            e2e::control_bounds(window.window_id(), &format!("activity-bucket-{i}")).unwrap();
        assert!(
            bounds.contains(&point),
            "focus region contains its painted point"
        );
    }
    crate::keyboard_nav::keys(cx, window, "end left right");
    draw(cx, window);
    let last = data.buckets.len() - 1;
    cx.update_window(window, |_, window, _| {
        assert!(e2e::activity::node(&format!("activity-bucket-{last}"))
            .unwrap()
            .focus
            .unwrap()
            .is_focused(window));
    })
    .unwrap();
}

fn matrix_cell(cx: &mut VisualTestAppContext, slug: &str, zoom: f32, name: &str) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom", "theme"]);
    let original = (
        kagi_ui_core::i18n::lang(),
        theme::zoom(),
        theme::theme().slug.to_string(),
    );
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    theme::set_zoom(zoom);
    app.update(cx, |app, cx| {
        app.set_lang(Lang::En, cx);
        app.set_theme(slug, cx);
        app.bottom_panel_open = true;
        app.bottom_tab = BottomTab::Activity;
        app.bottom_panel_height = 230.;
        cx.notify();
    });
    crate::pr_detail_adaptive::resize(cx, window, (940., 660.));
    cx.run_until_parked();
    for (period, count) in [
        (Granularity::Day, 48),
        (Granularity::Week, 42),
        (Granularity::Month, 30),
        (Granularity::Year, 52),
    ] {
        app.update(cx, |app, cx| {
            app.activity_granularity = period;
            app.activity_hover = None;
            let data = match period {
                Granularity::Day => &mut app.view_mut().activity.day,
                Granularity::Week => &mut app.view_mut().activity.week,
                Granularity::Month => &mut app.view_mut().activity.month,
                Granularity::Year => &mut app.view_mut().activity.year,
                Granularity::All => unreachable!(),
            };
            assert_eq!(data.buckets.len(), count, "real aggregation bucket count");
            for (i, bucket) in data.buckets.iter_mut().enumerate() {
                bucket.commits = (i % 7 + 1) as u32;
                bucket.merges = (i % 3) as u32;
                bucket.authors = (i % 5 + 1) as u32;
            }
            data.total_commits = data.buckets.iter().map(|b| b.commits).sum();
            data.total_merges = data.buckets.iter().map(|b| b.merges).sum();
            cx.notify();
        });
        // Include the actual narrow/high-zoom application layout.
        inspect(cx, &app, window);
        crate::pr_detail_adaptive::resize(cx, window, (1800., 660.));
        for width in [260., 400., 600.] {
            let _width = e2e::activity::PlotWidth::set(px(width));
            draw(cx, window);
            let plot = e2e::control_bounds(window.window_id(), "activity-chart").unwrap();
            assert!((f32::from(plot.size.width) - width).abs() < 0.1);
            inspect(cx, &app, window);
        }
        crate::pr_detail_adaptive::resize(cx, window, (940., 660.));
    }
    unmount(cx, app, window);
    kagi_ui_core::i18n::set_lang(original.0);
    theme::set_zoom(original.1);
    theme::set_active(&original.2);
    cx.update(theme::sync_gpui_component_theme);
    eprintln!("[gui-e2e] PASS {name}");
}

pub fn scenario_activity_bucket_hit(cx: &mut VisualTestAppContext) {
    matrix_cell(cx, "apple-light", 1.67, "activity_bucket_hit");
}
pub fn scenario_activity_bucket_hit_light_100(cx: &mut VisualTestAppContext) {
    matrix_cell(cx, "apple-light", 1., "activity_bucket_hit_light_100");
}
pub fn scenario_activity_bucket_hit_dark_100(cx: &mut VisualTestAppContext) {
    matrix_cell(cx, "apple-dark", 1., "activity_bucket_hit_dark_100");
}
pub fn scenario_activity_bucket_hit_dark_167(cx: &mut VisualTestAppContext) {
    matrix_cell(cx, "apple-dark", 1.67, "activity_bucket_hit_dark_167");
}
