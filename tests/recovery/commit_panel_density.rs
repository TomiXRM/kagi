//! #1463 / #1467: Finder-inspired, vertically balanced changed-file slots.
use crate::macos::{build_fixture, git, unmount};
use crate::recovery_layout::{draw, GlobalSettings};
use gpui::{px, size, AnyWindowHandle, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::{i18n, theme};
use std::{cell::RefCell, rc::Rc};

pub fn scenario(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    conflict_control_geometry(cx);
    let fixture = build_fixture();
    // Do not let a machine-global template consume the geometry fixture's lists.
    git(fixture.path(), &["config", "commit.template", "/dev/null"]);
    for n in 0..213 {
        let path = fixture
            .path()
            .join(format!("density/group-{:03}/file-{n:03}.txt", n / 3));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("staged {n}\n")).unwrap();
    }
    git(fixture.path(), &["add", "density"]);
    for n in 0..213 {
        std::fs::write(
            fixture
                .path()
                .join(format!("density/group-{:03}/file-{n:03}.txt", n / 3)),
            format!("unstaged {n}\n"),
        )
        .unwrap();
    }
    for (width, height) in [(1392., 883.), (940., 660.)] {
        for zoom in [1., 1.67] {
            for lang in [i18n::Lang::En, i18n::Lang::Ja] {
                for tree in [false, true] {
                    i18n::set_lang(lang);
                    theme::set_zoom(zoom);
                    let state = e2e::app_state(fixture.path()).unwrap();
                    let captured = Rc::new(RefCell::new(None));
                    let output = captured.clone();
                    let window = crate::macos::open_offscreen(
                        cx,
                        size(px(width), px(height)),
                        move |window, cx| e2e::mount_root(state, window, cx, &output),
                    );
                    let app = captured.borrow().clone().unwrap();
                    cx.update_window(window.into(), |_, window, cx| {
                        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
                        let panel = app.read(cx).ui().commit_panel.clone().unwrap();
                        panel.update(cx, |panel, cx| {
                            panel.state.tree_view = tree;
                            cx.notify();
                        });
                    })
                    .unwrap();
                    for _ in 0..4 {
                        cx.run_until_parked();
                        draw(cx, window.into(), (width, height));
                    }
                    let compact = kagi_domain::compact::is_compact(
                        width,
                        theme::rem_size_px(),
                        theme::compact_threshold_rem(),
                    );
                    if compact {
                        // Changes is a separate surface. At 167% the pinned
                        // composer's shadow reaches these rows, so dismiss it
                        // through its real control before sampling their rings.
                        let close = e2e::control_bounds(window.window_id(), "compact-popup-close")
                            .expect("compact composer close button");
                        cx.simulate_click(window.into(), close.center(), gpui::Modifiers::none());
                        e2e::clear_control_bounds(window.window_id(), "compact-composer");
                        for _ in 0..4 {
                            cx.run_until_parked();
                            draw(cx, window.into(), (width, height));
                        }
                        assert!(
                            e2e::control_bounds(window.window_id(), "compact-composer").is_none(),
                            "density samples the uncovered Changes lists"
                        );
                    }
                    for staged in [false, true] {
                        check(window.into(), staged, tree, zoom, compact);
                        if !tree {
                            check_focus_ring(cx, window.into(), staged, (width, height));
                        }
                    }
                    unmount(cx, app, window.into());
                }
            }
        }
    }
    eprintln!("[gui-e2e] PASS commit_panel_density");
}

fn check(window: AnyWindowHandle, staged: bool, tree: bool, zoom: f32, compact: bool) {
    let section = if staged { "st" } else { "us" };
    let bounds = |id: &str| {
        e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("missing {id}"))
    };
    // scaled_px includes Kagi's shared 0.9 density; the button floor is logical.
    let expected = theme::scaled_px(22.).max(px(20.));
    let header = bounds(if staged {
        "cp-staged-header"
    } else {
        "cp-unstaged-header"
    });
    let viewport = bounds(&format!("cp-{section}-viewport"));
    assert!(viewport.top() - header.bottom() <= px(1.));
    if staged || compact {
        let heading = bounds(if staged {
            "cp-staged-heading"
        } else {
            "cp-unstaged-heading"
        });
        assert!(
            (heading.top() - header.top() - px(1.)).abs() <= px(0.5),
            "header retains literal 1px top padding"
        );
        if compact {
            // Changes intentionally wraps the heading above the action row.
            // Pin both rows and their gap instead of applying the wide panel's
            // single-line heading-to-bottom oracle to the whole header.
            let action = bounds(if staged {
                "cp-unstage-all"
            } else {
                "cp-stage-all"
            });
            let selector = bounds(if staged {
                "cp-staged-view-list-label"
            } else {
                "cp-view-list-label"
            });
            let gap = action.top().min(selector.top()) - heading.bottom();
            assert!(
                (gap - px(theme::rem_size_px() * 0.25)).abs() <= px(1.5),
                "Changes header keeps one gap_1 between heading and controls: {gap:?}"
            );
            let row_bottom = action.bottom().max(selector.bottom());
            assert!(
                (header.bottom() - row_bottom - px(2.)).abs() <= px(0.5),
                "Changes header bottom retains 1px padding plus selector border"
            );
            for (name, control) in [("bulk action", action), ("List selector", selector)] {
                crate::recovery_layout::contained(header, control, name);
            }
        } else {
            assert!(
                header.bottom() - heading.bottom() <= px(1.5),
                "single-line header bottom padding"
            );
        }
    }
    if tree {
        // The narrow/high-zoom viewport may show only the root directory.
        // A fresh window avoids satisfying these checks with stale flat-row probes.
        let dir_id = format!("cp-{section}-dir-density");
        let label_id = format!("cp-{section}-dir-label-density");
        let directory = bounds(&dir_id);
        let label = bounds(&label_id);
        assert!((directory.size.height - expected).abs() <= px(1.));
        let above = label.top() - directory.top();
        let below = directory.bottom() - label.bottom();
        assert!(
            (above - below).abs() <= px(1.),
            "directory label must be vertically centered: above={above:?}, below={below:?}"
        );
        let paint = e2e::control_paint(window.window_id(), &label_id).unwrap();
        assert!((paint.font_size - theme::scaled_px(theme::FILE_TREE_FONT_PX)).abs() <= px(0.1));
        eprintln!("commit-panel density {section}/tree@{zoom}: directory={directory:?}, padding={above:?}/{below:?}, font={:?}", paint.font_size);
    } else {
        let first = bounds(&format!("cp-{section}-density-file-0"));
        let second = bounds(&format!("cp-{section}-density-file-1"));
        let slot = second.top() - first.top();
        eprintln!("commit-panel density {section}/flat@{zoom}: slot={slot:?}, row={first:?}");
        assert!((slot - expected).abs() <= px(1.), "file slot must be scaled 22 reference px with 20 logical px floor; measured old slot={slot:?}");
        // Inside probes exclude RowList's 2px focus border on each side.
        assert!((first.size.height + px(4.) - expected).abs() <= px(1.));
        let action = bounds(if staged {
            "cp-row-unstage"
        } else {
            "cp-row-stage"
        });
        // The row-action probe includes the reserved inset focus border.
        assert!(
            action.size.height >= px(20.),
            "Stage/Unstage tap target: {action:?}"
        );
        assert!(
            first.top() - viewport.top() <= px(3.),
            "first flat row starts immediately within border"
        );
    }
}

fn check_focus_ring(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    staged: bool,
    dimensions: (f32, f32),
) {
    let section = if staged { "st" } else { "us" };
    let row = e2e::commit_panel_labels::focus_handle(&format!("cp-{section}-row-0")).unwrap();
    cx.update_window(window, |_, window, cx| row.focus(window, cx))
        .unwrap();
    cx.run_until_parked();
    draw(cx, window, dimensions);
    cx.simulate_keystrokes(window, "tab");
    cx.run_until_parked();
    draw(cx, window, dimensions);
    assert!(
        cx.update_window(window, |_, window, cx| {
            row.contains_focused(window, cx) && !row.is_focused(window)
        })
        .unwrap(),
        "Tab enters the remembered row's Stage/Unstage button"
    );
    let first =
        e2e::control_bounds(window.window_id(), &format!("cp-{section}-density-file-0")).unwrap();
    let action = e2e::control_bounds(
        window.window_id(),
        if staged {
            "cp-row-unstage"
        } else {
            "cp-row-stage"
        },
    )
    .unwrap();
    cx.update_window(window, |_, window, _| {
        let image = window.render_to_image().expect("inline action focus ring");
        let scale = window.scale_factor();
        // The button is centered in the row, but at high zoom its paint frame
        // is shorter than the slot. Sample its own lower border, not the gap.
        let edge_y = first.center().y + action.size.height / 2. - px(0.5);
        let pixel = image.get_pixel(
            (f32::from(action.center().x) * scale) as u32,
            (f32::from(edge_y) * scale) as u32,
        ).0;
        let color = theme::theme().ui_accent();
        let expected = [(color >> 16) as u8, (color >> 8) as u8, color as u8];
        assert!(
            (0..3).all(|channel| pixel[channel].abs_diff(expected[channel]) <= 2),
            "focused Stage/Unstage lower ring edge must survive adjacent row paint: {pixel:?}, expected {expected:?}"
        );
    })
    .unwrap();
}

/// #1752 / #1772: inert outline labels share Stage/Unstage's frame, not its role.
fn conflict_control_geometry(cx: &mut VisualTestAppContext) {
    for directory in ["", "nested/", "nested/deeper/"] {
        conflict_control_geometry_at_depth(cx, directory);
    }
}

fn conflict_control_geometry_at_depth(cx: &mut VisualTestAppContext, directory: &str) {
    use gpui_component::ActiveTheme as _;
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["config", "commit.template", "/dev/null"]);
    std::fs::create_dir_all(repo.join(directory)).unwrap();
    let conflict_path = format!("{directory}conflict.txt");
    let stage_path = format!("{directory}stage.txt");
    let unstage_path = format!("{directory}unstage.txt");
    let convention_path = format!("{directory}AGENTS.md");
    git(repo, &["checkout", "-qb", "row-main"]);
    std::fs::write(repo.join(&conflict_path), "base\n").unwrap();
    git(repo, &["add", &conflict_path]);
    git(repo, &["commit", "-qm", "row base"]);
    git(repo, &["checkout", "-qb", "row-incoming"]);
    std::fs::write(repo.join(&conflict_path), "incoming\n").unwrap();
    git(repo, &["commit", "-qam", "row incoming"]);
    git(repo, &["checkout", "-q", "row-main"]);
    std::fs::write(repo.join(&conflict_path), "current\n").unwrap();
    git(repo, &["commit", "-qam", "row current"]);
    let merge = std::process::Command::new("git")
        .current_dir(repo)
        .args(["merge", "row-incoming"])
        .output()
        .unwrap();
    assert!(!merge.status.success(), "fixture must conflict");
    std::fs::write(repo.join(&stage_path), "stage\n").unwrap();
    std::fs::write(repo.join(&unstage_path), "unstage\n").unwrap();
    git(repo, &["add", &unstage_path]);
    std::fs::write(repo.join(&convention_path), "Repository conventions\n").unwrap();
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let conflict = std::fs::read(repo.join(&conflict_path)).unwrap();
    let _presentation = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "accent_source", "accent"]);
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        for zoom in [1., 1.67] {
            for tree in [false, true] {
                for slug in ["apple-light", "apple-dark"] {
                    i18n::set_lang(lang);
                    theme::set_zoom(zoom);
                    e2e::commit_panel_labels::clear();
                    let (app, window) = crate::macos::mount(cx, repo);
                    cx.update_window(window, |_, window, cx| {
                        app.update(cx, |app, cx| {
                            app.set_theme(slug, cx);
                            app.show_graph_mode(cx);
                            app.open_commit_panel(window, cx);
                        });
                        app.read(cx)
                            .ui()
                            .commit_panel
                            .clone()
                            .unwrap()
                            .update(cx, |panel, cx| {
                                panel.state.tree_view = tree;
                                cx.notify();
                            });
                    })
                    .unwrap();
                    for _ in 0..4 {
                        cx.run_until_parked();
                        draw(cx, window, (1440., 900.));
                    }
                    let bounds = |id: &str| {
                        e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| {
                            panic!("missing {id}: {lang:?}/{zoom}/{tree}/{slug}")
                        })
                    };
                    let conflict_label = bounds("cp-row-conflict");
                    let radius = cx.read(|cx| cx.theme().radius);
                    for id in [
                        "cp-row-conflict",
                        "cp-row-stage",
                        "cp-row-unstage",
                        "cp-row-convention",
                    ] {
                        let control = bounds(id);
                        assert!(
                            (control.size.height - theme::scaled_px(20.).max(px(20.))).abs()
                                <= px(0.5),
                            "{id} must retain the 20px minimum: {control:?}"
                        );
                        assert!(
                            (control.size.height - conflict_label.size.height).abs() <= px(0.5),
                            "{id} must match Conflict height"
                        );
                        assert_eq!(
                            e2e::commit_panel_labels::rendered_radius(id),
                            Some(radius),
                            "{id} must use the shared Button medium radius token"
                        );
                    }
                    assert_eq!(
                        e2e::commit_panel_labels::rendered("cp-row-conflict").as_deref(),
                        Some(i18n::Msg::CommitPanelConflict.t())
                    );
                    assert_eq!(
                        e2e::commit_panel_labels::rendered("cp-row-convention").as_deref(),
                        Some(i18n::Msg::AgentConventionBadge.t()),
                        "conventions retain the localized Rule label"
                    );
                    let panel = cx.read(|cx| app.read(cx).ui().commit_panel.clone().unwrap());
                    let file_index = |path: &str| {
                        cx.read(|cx| {
                            panel
                                .read(cx)
                                .state
                                .unstaged
                                .iter()
                                .position(|file| file.path == std::path::Path::new(path))
                                .unwrap()
                        })
                    };
                    let conflict_index = file_index(&conflict_path);
                    let convention_index = file_index(&convention_path);
                    let ordinary = bounds(&format!("cp-us-file-label-{}", file_index(&stage_path)));
                    for (id, index) in [
                        ("cp-row-conflict", conflict_index),
                        ("cp-row-convention", convention_index),
                    ] {
                        let filename = bounds(&format!("cp-us-file-label-{index}"));
                        let label = bounds(id);
                        assert!(
                            (filename.left() - ordinary.left()).abs() <= px(0.5),
                            "status labels must not indent same-depth filenames"
                        );
                        assert!(
                            label.left() >= filename.right(),
                            "{id} must follow its filename"
                        );
                        assert!(
                            label.right() < bounds("cp-row-stage").left(),
                            "{id} belongs to file identity, not the trailing action column"
                        );
                    }
                    for (id, semantic) in [
                        ("cp-row-conflict", theme::theme().color_blocker),
                        ("cp-row-convention", theme::theme().color_warning),
                    ] {
                        let (_, ink, border) =
                            e2e::commit_panel_labels::rendered_badge_colors(id).unwrap();
                        let expected: gpui::Hsla = gpui::rgb(semantic).into();
                        let actual: gpui::Hsla = gpui::rgb(ink).into();
                        let difference = (expected.h - actual.h).abs();
                        assert!(
                            difference.min(1. - difference) < 0.02,
                            "{id} must retain its own semantic hue, not share conflict red"
                        );
                        assert_eq!(ink, border, "outline uses the same readable semantic ink");
                    }
                    for id in ["cp-row-conflict", "cp-row-convention"] {
                        assert_eq!(
                            e2e::commit_panel_labels::accessible_role(id),
                            Some(gpui::Role::Label),
                            "{id} is status text, not an unavailable action"
                        );
                        assert_eq!(
                            e2e::commit_panel_labels::accessible_name(id),
                            e2e::commit_panel_labels::rendered(id)
                        );
                        assert_eq!(
                            e2e::commit_panel_labels::accessible_disabled(id),
                            None,
                            "status labels have no disabled-action semantics"
                        );
                        assert_eq!(e2e::commit_panel_labels::accessible_pressed(id), None);
                        assert!(
                            e2e::commit_panel_labels::focus_handle(id).is_none(),
                            "status labels must not add a button focus stop"
                        );
                    }
                    for (id, index) in [
                        ("cp-row-conflict", conflict_index),
                        ("cp-row-convention", convention_index),
                    ] {
                        for hover in [false, true] {
                            let position = if hover {
                                bounds(id).center()
                            } else {
                                gpui::point(px(0.), px(0.))
                            };
                            cx.simulate_mouse_move(window, position, None, gpui::Modifiers::none());
                            cx.run_until_parked();
                            draw(cx, window, (1440., 900.));
                            check_outline_paint(cx, window, id, index, !hover);
                        }
                    }
                    cx.simulate_click(window, conflict_label.center(), gpui::Modifiers::none());
                    for _ in 0..4 {
                        cx.run_until_parked();
                        draw(cx, window, (1440., 900.));
                    }
                    cx.read(|cx| {
                        assert_eq!(
                            panel.read(cx).active_wip,
                            Some((false, std::path::PathBuf::from(&conflict_path))),
                            "clicking the conflict label selects its containing file row"
                        );
                        assert!(panel
                            .read(cx)
                            .state
                            .conflicted_paths
                            .contains(std::path::Path::new(&conflict_path)));
                    });
                    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
                    assert_eq!(std::fs::read(repo.join(&conflict_path)).unwrap(), conflict);
                    cx.simulate_click(
                        window,
                        bounds("cp-row-convention").center(),
                        gpui::Modifiers::none(),
                    );
                    for _ in 0..4 {
                        cx.run_until_parked();
                        draw(cx, window, (1440., 900.));
                    }
                    cx.read(|cx| {
                        // Desktop rows highlight the active WIP diff; selected_file
                        // retains selection only on the compact Changes surface.
                        assert_eq!(
                            panel.read(cx).active_wip,
                            Some((false, std::path::PathBuf::from(&convention_path))),
                            "the convention badge must select its containing file row"
                        );
                        let app = app.read(cx);
                        let pane = app.ui().main_diff.as_ref().expect("convention file diff");
                        let kagi::ui::MainDiffSource::Unstaged { path } =
                            &pane.read(cx).view.source
                        else {
                            panic!("convention badge must open the unstaged file diff");
                        };
                        assert_eq!(path, std::path::Path::new(&convention_path));
                    });
                    for active in [true, false] {
                        app.update(cx, |app, cx| app.set_window_active_for_e2e(active, cx));
                        for hover in [false, true] {
                            let position = if hover {
                                bounds("cp-row-convention").center()
                            } else {
                                gpui::point(px(0.), px(0.))
                            };
                            cx.simulate_mouse_move(window, position, None, gpui::Modifiers::none());
                            // Let the existing selection animation settle before measuring ink.
                            cx.advance_clock(std::time::Duration::from_secs(1));
                            for _ in 0..4 {
                                cx.run_until_parked();
                                draw(cx, window, (1440., 900.));
                            }
                            check_outline_paint(
                                cx,
                                window,
                                "cp-row-convention",
                                convention_index,
                                true,
                            );
                            if active {
                                let (background, ink, border) =
                                    e2e::commit_panel_labels::rendered_badge_colors(
                                        "cp-row-convention",
                                    )
                                    .unwrap();
                                let primary =
                                    u32::from(theme::theme().selection_foreground(true).to_rgb())
                                        >> 8;
                                let selected_ink: gpui::Hsla = gpui::rgb(ink).into();
                                assert!(
                                    selected_ink.s < 0.01,
                                    "on-accent label ink must not retain warning hue"
                                );
                                if theme::contrast_ratio(background, primary) >= 4.6 {
                                    assert_eq!((ink, border), (primary, primary),
                                        "readable on-accent primary must be preserved for both strokes");
                                }
                            }
                        }
                    }
                    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
                    assert_eq!(std::fs::read(repo.join(&conflict_path)).unwrap(), conflict);
                    drop(panel);
                    unmount(cx, app, window);
                }
            }
        }
    }
}

fn check_outline_paint(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    id: &str,
    index: usize,
    compare_recorded_background: bool,
) {
    let (background, ink, border) = e2e::commit_panel_labels::rendered_badge_colors(id)
        .expect("renderer supplies row background, ink and outline");
    let badge = e2e::control_bounds(window.window_id(), id).unwrap();
    let row =
        e2e::control_bounds(window.window_id(), &format!("cp-us-density-file-{index}")).unwrap();
    cx.update_window(window, |_, window, _| {
        let image = window.render_to_image().expect("outline status paint");
        let scale = window.scale_factor();
        let sample = |x, y| {
            let pixel = image
                .get_pixel((f32::from(x) * scale) as u32, (f32::from(y) * scale) as u32)
                .0;
            [pixel[0], pixel[1], pixel[2]]
        };
        let rgb = |color: u32| [(color >> 16) as u8, (color >> 8) as u8, color as u8];
        let close = |a: [u8; 3], b: [u8; 3]| (0..3).all(|c| a[c].abs_diff(b[c]) <= 2);
        // The spacer just after the label has no glyph, border or action paint.
        let actual_row = sample(badge.right() + px(4.), row.center().y);
        let interior = sample(badge.left() + px(2.), badge.center().y);
        assert!(
            close(interior, actual_row),
            "{id} has no fill: interior {interior:?}, adjacent row {actual_row:?}"
        );
        if compare_recorded_background {
            assert!(
                close(actual_row, rgb(background)),
                "{id} recorded row background matches native paint"
            );
        }
        let actual_background =
            ((actual_row[0] as u32) << 16) | ((actual_row[1] as u32) << 8) | actual_row[2] as u32;
        assert!(
            theme::contrast_ratio(actual_background, ink) >= 4.5,
            "{id} text contrast against actual row paint must reach 4.5:1"
        );
        assert!(
            theme::contrast_ratio(actual_background, border) >= 3.,
            "{id} outline contrast against actual row paint must reach 3:1"
        );
        // A straight vertical edge avoids rounded-corner antialiasing. Search
        // its two physical edge pixels to tolerate fractional logical origins.
        assert!(
            (0..=2).any(|offset| close(
                sample(badge.left() + px(offset as f32 / scale), badge.center().y),
                rgb(border)
            )),
            "{id} must actually paint its outline"
        );
    })
    .unwrap();
}
