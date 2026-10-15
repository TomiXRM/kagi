//! #1479-S1: production terminal config, native shaping, and PTY cell geometry.
//! PTY text alone is not glyph evidence: check the configured stack's native
//! shapes as well as the parser's cell geometry. Linux tests pin the JP face.
use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use gpui::{px, AnyWindowHandle, Entity, Font, FontWeight, TextRun, VisualTestAppContext};
use gpui_terminal::TerminalView;
use kagi::ui::{terminal, theme, BottomTab, KagiApp};
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

struct FontShell {
    _dir: tempfile::TempDir,
}

impl FontShell {
    fn install() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let shell = dir.path().join("font-shell.sh");
        // A deterministic isolated child, not the developer's login shell.
        // Each spawn prints regular and ANSI-bold Japanese, then leaves the
        // cursor after ASCII + a two-cell character + ASCII on the third row.
        std::fs::write(
            &shell,
            "#!/bin/sh\nstty -echo\nprintf '\\033[2J\\033[HABC 日本語 直骨誤\\033[2;1H\\033[1mABC 日本語 直骨誤\\033[0m\\033[3;1HABC語Z'\nwhile IFS= read -r command; do\n  [ \"$command\" = exit ] && exit 0\ndone\n",
        )
        .unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
        KagiApp::set_terminal_shell_for_e2e(Some(shell.display().to_string()));
        Self { _dir: dir }
    }
}

impl Drop for FontShell {
    fn drop(&mut self) {
        KagiApp::set_terminal_shell_for_e2e(None);
    }
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn start(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> Entity<TerminalView> {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.bottom_panel_open = true;
            app.bottom_tab = BottomTab::Terminal;
            app.ensure_terminal(window, cx);
            cx.notify();
        });
    })
    .unwrap();
    draw(cx, window);
    cx.read(|cx| {
        app.read(cx)
            .ui()
            .terminal_session
            .as_ref()
            .unwrap()
            .view
            .clone()
            .unwrap()
    })
}

fn wait_for_text(cx: &mut VisualTestAppContext, view: &Entity<TerminalView>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| {
            view.read(cx).state().with_term(|term| {
                let cursor = term.grid().cursor.point;
                cursor.line.0 == 2 && cursor.column.0 == 6
            })
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "isolated PTY did not produce font fixture"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn assert_cells(cx: &mut VisualTestAppContext, view: &Entity<TerminalView>) {
    cx.read(|cx| {
        view.read(cx).state().with_term(|term| {
            let grid = term.grid();
            let mut point = grid.cursor.point;
            assert_eq!(
                point.column.0, 6,
                "two-cell CJK keeps the ASCII cursor column"
            );
            // Resizing may move the fixture vertically when scrollback is
            // reflowed. Preserve its relative rows, not an absolute viewport row.
            let first_row = point.line.0 - 2;
            for row in [0, 1] {
                point.line.0 = first_row + row;
                let mut col = 0;
                for ch in "ABC 日本語 直骨誤".chars() {
                    point.column.0 = col;
                    let cell = &grid[point];
                    assert_eq!(cell.c, ch);
                    let flag = |name| cell.flags.iter_names().any(|(key, _)| key == name);
                    assert_eq!(flag("BOLD"), row == 1, "ANSI bold must reach the grid");
                    assert_eq!(flag("WIDE_CHAR"), !ch.is_ascii());
                    col += 1;
                    if !ch.is_ascii() {
                        point.column.0 = col;
                        assert!(grid[point]
                            .flags
                            .iter_names()
                            .any(|(key, _)| key == "WIDE_CHAR_SPACER"));
                        col += 1;
                    }
                }
            }
            point.line.0 = grid.cursor.point.line.0;
            point.column.0 = 5;
            assert_eq!(grid[point].c, 'Z', "ASCII after CJK stays at column five");
        });
    });
}

fn assert_fonts(
    cx: &mut VisualTestAppContext,
    view: &Entity<TerminalView>,
    window: AnyWindowHandle,
    family: &str,
) -> gpui::Size<gpui::Pixels> {
    draw(cx, window);
    cx.update_window(window, |_, window, cx| {
        let terminal = view.read(cx);
        let config = terminal.config();
        let renderer = terminal.renderer();
        assert_eq!(config.font_family, family);
        assert_eq!(config.font_fallbacks, kagi_ui_core::fonts::ui_font().fallbacks);
        assert!(config.font_fallbacks.is_some());
        assert_eq!(renderer.font_size, config.font_size);
        let shape = |text: &str, font: Font| {
            window.text_system().shape_line(
                text.to_owned().into(),
                config.font_size,
                &[TextRun {
                    len: text.len(),
                    font,
                    color: gpui::rgb(0xffffff).into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            )
        };
        let mut jp_faces = Vec::new();
        let mut ascii_faces = Vec::new();
        for bold in [false, true] {
            let font = renderer.font(bold, false);
            assert_eq!(font.family.as_ref(), family);
            assert_eq!(font.fallbacks, config.font_fallbacks);
            assert_eq!(font.weight, if bold { FontWeight::BOLD } else { FontWeight::NORMAL });
            for ch in "ABC日本語直骨誤".chars() {
                let text = ch.to_string();
                let actual = shape(&text, font.clone());
                // This native scenario checks propagation of Kagi's requested
                // stack through the terminal lifecycle. Exact bundled JP face
                // selection is asserted independently by Linux CosmicTextSystem
                // tests; CoreText may resolve an embedded cascade differently.
                let reference = shape(&text, Font {
                    family: family.to_owned().into(),
                    weight: if bold { FontWeight::BOLD } else { FontWeight::NORMAL },
                    ..kagi_ui_core::fonts::ui_font()
                });
                assert_eq!(actual.runs.len(), 1);
                assert_eq!(reference.runs.len(), 1);
                assert!(!actual.runs[0].glyphs.is_empty());
                assert!(actual.runs[0].glyphs.iter().all(|glyph| glyph.id.0 != 0),
                    "{ch} bold={bold}: missing-glyph boxes are not font coverage");
                assert!(
                    actual.runs[0].glyphs.iter().map(|glyph| glyph.id)
                        .eq(reference.runs[0].glyphs.iter().map(|glyph| glyph.id)),
                    "{family} {ch} bold={bold}: rendered glyphs {:?} must match the explicit reference {:?}",
                    actual.runs, reference.runs,
                );
                assert_eq!(actual.runs[0].font_id, reference.runs[0].font_id,
                    "{ch} bold={bold}: renderer must use Kagi's requested font stack");
                if ch == 'A' {
                    ascii_faces.push(actual.runs[0].font_id);
                }
                if ch == '語' {
                    jp_faces.push(actual.runs[0].font_id);
                }
            }
        }
        assert_ne!(jp_faces[0], jp_faces[1], "regular and bold use distinct Japanese faces");
        for (ascii, japanese) in ascii_faces.iter().zip(&jp_faces) {
            assert_ne!(ascii, japanese, "ASCII retains the primary rather than the JP fallback");
        }
        let size = terminal.cell_size().expect("terminal painted cell metrics");
        assert!(size.width > px(0.) && size.height > px(0.));
        let measured = shape("M", renderer.font(false, false));
        assert!((f32::from(size.width) - f32::from(measured.width)).abs() < 0.1);
        size
    })
    .unwrap()
}

pub fn scenario_terminal_font_fallback(cx: &mut VisualTestAppContext) {
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = FontShell::install();
    let first = build_fixture();
    let second = build_fixture();
    let before = [
        repo_fingerprint(first.path()),
        repo_fingerprint(second.path()),
    ];
    theme::set_zoom(1.);
    cx.update(theme::sync_gpui_component_theme);
    let (app, window) = mount(cx, first.path());
    crate::pr_detail_adaptive::resize(cx, window, (1000., 720.));
    crate::native_ax::activate(cx);
    draw(cx, window);
    let primary = terminal::build_terminal_config().font_family;
    let first_view = start(cx, &app, window);
    wait_for_text(cx, &first_view);
    assert_cells(cx, &first_view);
    let initial = assert_fonts(cx, &first_view, window, &primary);

    // Exercise the public live font configuration path, not a fictitious Kagi
    // font preference (production auto-selects an installed Nerd / bundled mono).
    let alternate = if primary == "JetBrains Mono" {
        "Inter"
    } else {
        "JetBrains Mono"
    };
    first_view.update(cx, |view, cx| {
        let mut config = terminal::build_terminal_config();
        config.font_family = alternate.into();
        config.font_size *= 1.5;
        view.update_config(config, cx);
    });
    let changed = assert_fonts(cx, &first_view, window, alternate);
    assert_ne!(
        initial, changed,
        "live font changes must recompute painted metrics"
    );

    app.update(cx, |app, cx| {
        assert!(app.open_repository(second.path().to_path_buf(), cx))
    });
    let second_view = start(cx, &app, window);
    assert_ne!(first_view.entity_id(), second_view.entity_id());
    wait_for_text(cx, &second_view);
    assert_fonts(cx, &second_view, window, &primary);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("view.zoomIn", window, cx)
        });
    })
    .unwrap();
    let zoomed = terminal::build_terminal_config();
    for view in [&first_view, &second_view] {
        cx.read(|cx| {
            let config = view.read(cx).config();
            assert_eq!(config.font_family, zoomed.font_family);
            assert_eq!(config.font_size, zoomed.font_size);
            assert_eq!(config.font_fallbacks, zoomed.font_fallbacks);
        });
    }
    assert_cells(cx, &second_view);
    assert_fonts(cx, &second_view, window, &primary);
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    draw(cx, window);
    let retained = start(cx, &app, window);
    assert_eq!(retained.entity_id(), first_view.entity_id());
    let resized = assert_fonts(cx, &first_view, window, &primary);
    assert!(resized.width > initial.width && resized.height > initial.height);
    assert_cells(cx, &first_view);

    let generation = cx.read(|cx| {
        let session = app.read(cx).ui().terminal_session.as_ref().unwrap();
        session.paste_writer.as_ref().unwrap().write_input("exit\n");
        session.spawns
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| {
            matches!(
                app.read(cx).ui().terminal_session.as_ref().unwrap().state,
                terminal::TerminalState::Exited { code: 0 }
            )
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "font fixture shell did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
    draw(cx, window);
    crate::app_conflict::click_control(cx, window, "terminal-restart");
    let restarted = start(cx, &app, window);
    assert_ne!(restarted.entity_id(), first_view.entity_id());
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().terminal_session.as_ref().unwrap().spawns),
        generation + 1
    );
    wait_for_text(cx, &restarted);
    assert_cells(cx, &restarted);
    assert_eq!(assert_fonts(cx, &restarted, window, &primary), resized);
    // Drop inspection handles before the standard unmount leak/PTY cleanup.
    drop((retained, first_view, second_view, restarted));
    unmount(cx, app, window);
    assert_eq!(
        before,
        [
            repo_fingerprint(first.path()),
            repo_fingerprint(second.path())
        ]
    );
}
