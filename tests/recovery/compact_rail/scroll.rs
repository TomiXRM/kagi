//! Compact PR navigator's native scrollbar policy and owner lifecycle.
use super::*;
use gpui_component::scroll::{ScrollbarHandle, ScrollbarShow};
fn scrollbar_pixels(cx: &mut VisualTestAppContext, win: AnyWindowHandle) -> Vec<[u8; 4]> {
    let area = bounds(win, "compact-pr-list-scrollbar");
    cx.update_window(win, |_, w, _| {
        let image = w.render_to_image().expect("real native scrollbar paint");
        let scale = w.scale_factor();
        let x = ((f32::from(area.right()) - 5.) * scale) as u32;
        let y = ((f32::from(area.top()) + 4.) * scale) as u32;
        (y..((f32::from(area.bottom()) - 4.) * scale) as u32)
            .map(|y| image.get_pixel(x, y).0)
            .collect()
    })
    .unwrap()
}

fn drag_pr_thumb_to_end(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    handle: &gpui::ScrollHandle,
) {
    let area = bounds(win, "compact-pr-list-scrollbar");
    let height = area.size.height;
    let thumb = (height / handle.content_size().height * height).max(px(48.));
    let from = point(area.right() - px(8.), area.top() + thumb / 2.);
    // Drag to the end of the track, not two pixels before the thumb's final
    // center: that deliberately near-end helper cannot prove tail containment.
    let to = point(from.x, area.bottom() - px(1.));
    cx.simulate_mouse_move(win, from, None, Modifiers::none());
    settle(cx, win);
    cx.simulate_mouse_down(win, from, gpui::MouseButton::Left, Modifiers::none());
    settle(cx, win);
    for step in 1..=5 {
        cx.simulate_mouse_move(
            win,
            from + (to - from) * (step as f32 / 5.),
            gpui::MouseButton::Left,
            Modifiers::none(),
        );
        settle(cx, win);
    }
    cx.simulate_mouse_up(win, to, gpui::MouseButton::Left, Modifiers::none());
    settle(cx, win);
    assert!(
        handle.offset().y < -height,
        "native thumb drag moves the real PR scroll owner"
    );
}
pub fn scenario_compact_pr_popover_scroll(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    let second = build_fixture();
    let _offline = crate::pr_fields_focus::OfflineGh::with_script("#!/bin/sh\nexit 1\n");
    matrix(|case| {
        let (app, win) = mount(cx, fixture.path(), case);
        seed_prs(cx, &app, 33);
        cx.update(|cx| {
            gpui_component::Theme::global_mut(cx).scrollbar_show = ScrollbarShow::Always
        });
        click(cx, win, "compact-rail-prs");
        click(cx, win, "pr-mode-section-1");
        let handle = e2e::compact_rail::pr_scroll(win.window_id())
            .expect("renderer records real session PR scroll owner");
        let viewport = bounds(win, "pr-sidebar-list-viewport");
        contained(
            bounds(win, POPUP),
            bounds(win, "compact-pr-list-scrollbar"),
            "actual thumb lane stays inside popup",
        );
        let extent = handle.content_size().height;
        assert!(
            extent > viewport.size.height * 3.,
            "33 naturally sized cards overflow"
        );
        let always_pixels = scrollbar_pixels(cx, win);
        assert!(
            always_pixels
                .windows(2)
                .any(|pixels| pixels[0] != pixels[1]),
            "Always paints actual thumb against track"
        );
        drag_pr_thumb_to_end(cx, win, &handle);
        settle(cx, win);
        contained(
            viewport,
            bounds(win, "pr-mode-card-39"),
            "thumb drag reaches final real PR card",
        );
        wheel(cx, win, viewport, 100000.);
        tab_to(cx, win, "pr-mode-section-1", true);
        press(cx, win, "space");
        assert!(
            handle.content_size().height < extent,
            "folded section shrinks actual scroll extent"
        );
        press(cx, win, "enter");
        click(cx, win, "list-filter-text");
        cx.simulate_input(win, "rail PR 39");
        settle(cx, win);
        assert!(
            handle.content_size().height < extent / 2.,
            "filter shrinks actual thumb extent"
        );
        bounds(win, "pr-mode-card-39");
        cx.simulate_keystrokes(win, "cmd-a backspace");
        settle(cx, win);
        assert!(
            (handle.content_size().height - extent).abs() < px(1.),
            "filter clear restores natural extent"
        );
        cx.update(|cx| {
            gpui_component::Theme::global_mut(cx).scrollbar_show = ScrollbarShow::Scrolling
        });
        cx.simulate_mouse_move(
            win,
            point(px(case.width - 2.), px(case.height - 2.)),
            None,
            Modifiers::none(),
        );
        for _ in 0..30 {
            cx.advance_clock(Duration::from_millis(100));
            settle(cx, win);
        }
        let hidden_pixels = scrollbar_pixels(cx, win);
        assert_ne!(
            always_pixels, hidden_pixels,
            "Automatic hides real native thumb after idle"
        );
        assert_eq!(
            bounds(win, "pr-sidebar-list-viewport").size.width,
            viewport.size.width,
            "Automatic adds no permanent lane"
        );
        wheel(cx, win, viewport, -160.);
        assert_ne!(
            scrollbar_pixels(cx, win),
            hidden_pixels,
            "wheel reveals actual Automatic thumb"
        );
        press(cx, win, "escape");
        app.update(cx, |app, cx| {
            app.open_repository(second.path().to_path_buf(), cx)
        });
        settle(cx, win);
        seed_prs(cx, &app, 1);
        click(cx, win, "compact-rail-prs");
        let fresh =
            e2e::compact_rail::pr_scroll(win.window_id()).expect("new repository scroll owner");
        assert_eq!(
            fresh.offset().y,
            px(0.),
            "new repository starts at its own top"
        );
        let old_offset = handle.offset();
        fresh.set_offset(point(px(0.), px(-10.)));
        assert_eq!(
            handle.offset(),
            old_offset,
            "repositories never share scroll ownership"
        );
        drop(fresh);
        drop(handle);
        e2e::compact_rail::clear(win.window_id());
        unmount(cx, app, win);
    });
}
