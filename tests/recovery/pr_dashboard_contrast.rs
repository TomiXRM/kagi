//! #1352: native composited PR metadata, not just unattenuated theme tokens.
use crate::macos::{build_fixture, unmount};
use gpui::{
    point, px, size, AnyWindowHandle, Bounds, Modifiers, Pixels, Role, VisualTestAppContext,
};
use kagi::ui::{e2e, i18n, list_a11y, theme, KagiApp};
use std::{cell::RefCell, rc::Rc};

fn draw(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn bounds(win: AnyWindowHandle, name: &str) -> Bounds<Pixels> {
    e2e::control_bounds(win.window_id(), name).expect("production PR cell painted")
}

fn contrast(a: u32, b: u32) -> f64 {
    let a = crate::settings_quiet::luminance(a);
    let b = crate::settings_quiet::luminance(b);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn assert_pixels(image: &image::RgbaImage, scale: f32, rect: Bounds<Pixels>, name: &str, ink: u32) {
    let mut colors = std::collections::HashMap::<u32, usize>::new();
    for y in (f32::from(rect.top()) * scale).ceil() as u32
        ..(f32::from(rect.bottom()) * scale).floor() as u32
    {
        for x in (f32::from(rect.left()) * scale).ceil() as u32
            ..(f32::from(rect.right()) * scale).floor() as u32
        {
            let p = image.get_pixel(x, y).0;
            let rgb = (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
            *colors.entry(rgb).or_default() += 1;
        }
    }
    // The blank portion of each cell is its actual painted background.
    // The strongest glyph pixel avoids anti-aliased edge coverage; no theme
    // foreground or background is substituted into the contrast oracle.
    let background = *colors.iter().max_by_key(|(_, count)| *count).unwrap().0;
    let foreground = *colors
        .keys()
        .max_by(|a, b| contrast(**a, background).total_cmp(&contrast(**b, background)))
        .unwrap();
    let ratio = contrast(foreground, background);
    eprintln!("[gui-e2e] {name}: painted #{foreground:06x} on #{background:06x}, {ratio:.3}:1");
    assert!(ratio >= 4.5, "{name}: native metadata contrast {ratio:.3}:1 < 4.5:1 (#{foreground:06x} on #{background:06x})");
    for shift in [0, 8, 16] {
        assert!(
            (((foreground >> shift) & 255) as i32 - ((ink >> shift) & 255) as i32).abs() <= 2,
            "{name}: title/metadata must retain full token contrast"
        );
    }
}

fn cell_matrix(cx: &mut VisualTestAppContext, app: &gpui::Entity<KagiApp>, win: AnyWindowHandle) {
    let horizontal = cx.read(|cx| {
        app.read(cx)
            .pr_mode()
            .unwrap()
            .dashboard_horizontal_scroll
            .clone()
    });
    for active in [true, false] {
        app.update(cx, |app, cx| app.set_window_active_for_e2e(active, cx));
        for hover in [false, true] {
            for number in [1, 2] {
                for cells in [
                    &["number", "title"][..],
                    &["state", "author"][..],
                    &["age"][..],
                ] {
                    horizontal.set_offset(point(px(0.), px(0.)));
                    app.update(cx, |_, cx| cx.notify());
                    draw(cx, win);
                    let id = format!("pr-home-{}-{number}", cells.last().unwrap());
                    let viewport = bounds(win, "pr-home-table-viewport");
                    let rect = bounds(win, &id);
                    if rect.right() > viewport.right() {
                        horizontal
                            .set_offset(point(viewport.right() - rect.right() - px(2.), px(0.)));
                        app.update(cx, |_, cx| cx.notify());
                        draw(cx, win);
                    }
                    let rect = bounds(win, &id);
                    assert!(
                        rect.left() >= viewport.left() && rect.right() <= viewport.right(),
                        "{id} reachable: {rect:?} in {viewport:?}"
                    );
                    cx.simulate_mouse_move(
                        win,
                        if hover {
                            rect.center()
                        } else {
                            point(px(1.), px(1.))
                        },
                        None,
                        Modifiers::none(),
                    );
                    draw(cx, win);
                    cx.update_window(win, |_, window, _| {
                        let image = window
                            .render_to_image()
                            .expect("native PR metadata Metal pixels");
                        for cell in cells {
                            if *cell == "state" && number == 2 {
                                continue;
                            }
                            let id = format!("pr-home-{cell}-{number}");
                            let rect = bounds(win, &id);
                            assert!(
                                rect.left() >= viewport.left() && rect.right() <= viewport.right(),
                                "{id} reachable"
                            );
                            assert_pixels(
                                &image,
                                window.scale_factor(),
                                rect,
                                &id,
                                if *cell == "title" {
                                    theme::theme().text_main
                                } else {
                                    theme::theme().text_muted
                                },
                            );
                        }
                    })
                    .unwrap();
                }
            }
        }
    }
    let list = list_a11y::recorded_list("pr-list").unwrap();
    assert_eq!(list.role, Some(Role::List));
    assert_eq!(list.size, 2);
    assert!(list
        .rows
        .values()
        .any(|(label, _)| label.contains(i18n::Msg::PrDraft.t())));
    assert!(
        list.rows.values().all(|(_, selected)| !selected),
        "dashboard is an action list, not a selected-row list"
    );
}

pub fn scenario_pr_dashboard_draft_contrast(
    cx: &mut VisualTestAppContext,
    dimensions: (f32, f32),
    slug: &str,
) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    let state = e2e::app_state(fixture.path()).expect("fixture app state");
    let captured: Rc<RefCell<Option<gpui::Entity<KagiApp>>>> = Rc::default();
    let output = captured.clone();
    let win: AnyWindowHandle = crate::macos::open_offscreen(
        cx,
        size(px(dimensions.0), px(dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &output),
    )
    .into();
    let app = captured.borrow_mut().take().unwrap();
    let mut draft = crate::evidence_support::pull_request(
        1,
        "Readable PR / 読めるタイトル",
        "feature/readable",
    );
    draft.is_draft = true;
    draft.updated_at = "2026-10-01T00:00:00Z".into();
    let mut ready = draft.clone();
    ready.number = 2;
    ready.is_draft = false;
    e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(crate::evidence_support::pr_page(
        vec![draft, ready],
        "",
        None,
    ))));
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.set_theme(slug, cx));
    for lang in ["en", "ja"] {
        std::env::set_var("KAGI_LANG", lang);
        i18n::init_lang();
        for zoom in [1., 1.667] {
            theme::set_zoom(zoom);
            eprintln!("[gui-e2e] PR contrast {dimensions:?} {slug} {lang} zoom={zoom}");
            cell_matrix(cx, &app, win);
        }
    }
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS pr_dashboard_draft_contrast {dimensions:?} {slug}: native title/number/author/age/Draft pixels, ready parity, rest/hover, active/inactive, EN/JA at 100%/167%");
}
