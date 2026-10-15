//! Finder drops exercise GPUI's native external-path dispatch, not a UI helper.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{point, px, AnyWindowHandle, FileDropEvent, VisualTestAppContext};
use kagi::ui::FooterStatus;

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn enter(cx: &mut VisualTestAppContext, window: AnyWindowHandle, paths: Vec<std::path::PathBuf>) {
    cx.simulate_event(
        window,
        FileDropEvent::Entered {
            position: point(px(500.), px(350.)),
            paths: gpui::ExternalPaths(paths.into_iter().collect()),
        },
    );
    draw(cx, window);
}

fn submit(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.simulate_event(
        window,
        FileDropEvent::Submit {
            position: point(px(500.), px(350.)),
        },
    );
    cx.run_until_parked();
    draw(cx, window);
}

fn edge_pixel(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> [u8; 4] {
    cx.update_window(window, |_, window, _| {
        let image = window
            .render_to_image()
            .expect("native drop-target highlight");
        let scale = window.scale_factor();
        image.get_pixel(scale as u32, (350. * scale) as u32).0
    })
    .unwrap()
}

pub fn scenario_folder_drop_open(cx: &mut VisualTestAppContext) {
    for width in [1000., 940.] {
        let initial = build_fixture();
        let target = build_fixture();
        let (app, window) = mount(cx, initial.path());
        crate::pr_detail_adaptive::resize(cx, window, (width, 720.));
        draw(cx, window);
        enter(cx, window, vec![target.path().to_path_buf()]);
        submit(cx, window);
        cx.read(|cx| {
            let state = app.read(cx);
            assert_eq!(
                state.tabs.len(),
                2,
                "Finder repository folder drop must open a new tab"
            );
            assert_eq!(
                state.repo_path.as_deref(),
                Some(target.path().canonicalize().unwrap().as_path())
            );
        });
        let opened_log = format!(
            "[kagi] tab-load: {} rows=",
            target.path().file_name().unwrap().to_string_lossy()
        );
        assert!(
            kagi_ui_core::klog::tail()
                .iter()
                .any(|line| line.starts_with(&opened_log)),
            "repository drop uses existing tab-load logging"
        );
        let neutral = edge_pixel(cx, window);
        enter(cx, window, vec![target.path().to_path_buf()]);
        assert_ne!(
            edge_pixel(cx, window),
            neutral,
            "directory payload highlights the valid target"
        );
        submit(cx, window);
        cx.read(|cx| {
            assert_eq!(
                app.read(cx).tabs.len(),
                2,
                "repeat Finder drop must focus existing tab"
            )
        });
        let file = target.path().join("not-a-folder.txt");
        std::fs::write(&file, "not a repository folder").unwrap();
        enter(cx, window, vec![file]);
        assert_eq!(
            edge_pixel(cx, window),
            neutral,
            "file-only payload has no valid-target highlight"
        );
        submit(cx, window);
        cx.read(|cx| {
            let state = app.read(cx);
            assert_eq!(state.tabs.len(), 2);
            assert!(
                matches!(state.status_footer, FooterStatus::Failed(_)),
                "file drop must present existing open failure"
            );
        });
        let not_repo = tempfile::tempdir().unwrap();
        enter(cx, window, vec![not_repo.path().to_path_buf()]);
        submit(cx, window);
        cx.read(|cx| {
            let state = app.read(cx);
            assert_eq!(state.tabs.len(), 2);
            assert!(
                matches!(state.status_footer, FooterStatus::Failed(_)),
                "non-repository folder presents existing failure"
            );
        });
        unmount(cx, app, window);
    }
}
