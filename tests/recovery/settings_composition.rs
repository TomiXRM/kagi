//! Canonical #1814 relations measured on the production native Settings owner.
//! Oracle values come from 1c6486202, not production kit constants.
use gpui::{px, App, Bounds, FontWeight, Pixels, Point, Role, Window};
use kagi_ui_settings::{ControlProbe, Pane, SettingsView};

fn near(actual: Pixels, expected: Pixels, relation: &str) {
    assert!(
        (actual - expected).abs() <= px(1.),
        "{relation}: measured {actual:?}, canonical {expected:?}"
    );
}

fn contains(outer: Bounds<Pixels>, inner: Bounds<Pixels>, relation: &str) {
    assert!(
        inner.left() >= outer.left() - px(1.)
            && inner.right() <= outer.right() + px(1.)
            && inner.top() >= outer.top() - px(1.)
            && inner.bottom() <= outer.bottom() + px(1.),
        "{relation}: {inner:?} outside measured owner {outer:?}"
    );
}

fn probe<'a>(probes: &'a [ControlProbe], id: &str) -> &'a ControlProbe {
    probes
        .iter()
        .find(|probe| probe.id == id)
        .unwrap_or_else(|| panic!("missing measured native composition part {id}"))
}

fn assert_rgb(actual: [u8; 4], expected: [u8; 3], relation: &str) {
    for (got, want) in actual[..3].iter().zip(expected) {
        assert!(
            got.abs_diff(want) <= 2,
            "{relation}: {actual:?} != {expected:?}"
        );
    }
}

fn assert_tile_gradient(
    id: &str,
    tile: Bounds<Pixels>,
    base: u32,
    backing: [u8; 3],
    scale: f32,
    sample: &impl Fn(Point<Pixels>) -> [u8; 4],
) {
    // Canonical 22%-white top stop, rounded before sRGB interpolation.
    let bottom = [16, 8, 0].map(|shift| ((base >> shift) & 255) as f32);
    let top = bottom.map(|channel| (channel + (255. - channel) * 0.22).round());
    for fraction in [0.25, 0.5, 0.75] {
        let point = Point::new(
            tile.left() + px(2.),
            tile.top() + tile.size.height * fraction,
        );
        let actual = sample(point);
        // render_to_image samples an integer pixel; Metal shades its centre.
        let pixel_y = (f32::from(point.y) * scale).floor() + 0.5;
        let t = (pixel_y / scale - f32::from(tile.top())) / f32::from(tile.size.height);
        for channel in 0..3 {
            let ideal = top[channel] + (bottom[channel] - top[channel]) * t;
            // Pinned GPUI shaders.metal:1232-1242 dithers RGB by ±2/255 AND
            // alpha by ±3/255, then source-alpha blends over the row/sidebar.
            // Bound that renderer noise, not an arbitrary pixel tolerance.
            let mut lower = f32::INFINITY;
            let mut upper = f32::NEG_INFINITY;
            for rgb_noise in [-2., 2.] {
                for alpha in [1. - 3. / 255., 1., 1. + 3. / 255.] {
                    let value =
                        (ideal + rgb_noise) * alpha + f32::from(backing[channel]) * (1. - alpha);
                    lower = lower.min(value);
                    upper = upper.max(value);
                }
            }
            let actual = f32::from(actual[channel]);
            assert!(
                actual >= lower.floor() && actual <= upper.ceil(),
                "{id} gradient at {point:?}, channel {channel}: {actual} outside {lower}..{upper}"
            );
        }
    }
}

fn assert_sidebar_paint(
    panel: Bounds<Pixels>,
    probes: &[ControlProbe],
    scale: f32,
    sample: &impl Fn(Point<Pixels>) -> [u8; 4],
) {
    // Pinned kit ink roles in Apple Light/Dark; never production kit helpers.
    let dark = kagi::ui::theme::theme().dark;
    let (window, sidebar, hairline) = if dark {
        ([16, 16, 18], [22, 22, 24], [50, 50, 53])
    } else {
        ([247, 247, 247], [241, 241, 241], [220, 220, 220])
    };
    assert_rgb(sample(Point::new(px(2.), px(2.))), window, "window ink");
    assert_rgb(
        sample(Point::new(panel.center().x, panel.bottom() - px(20.))),
        sidebar,
        "sidebar ink",
    );
    assert_rgb(
        sample(Point::new(panel.left() + px(16.), panel.top() + px(0.25))),
        hairline,
        "sidebar one-pixel hairline",
    );
    // The top-left raster contour must be concentric with the OS window
    // corner (owner ruling, PR #1818, 2026-10-29): its circle's centre is the
    // window corner's centre, so its radius is that centre minus the measured
    // inset. macOS 26's continuous corner on this titled, toolbar-less window
    // fits circles centred 15.0–15.6pt from each edge in the real window's
    // alpha mask (Tier B); render_to_image omits the OS mask.
    const OS_WINDOW_CORNER_CENTRE: f32 = 15.5;
    near(
        panel.left(),
        panel.top(),
        "sidebar inset equal on both corner axes",
    );
    let radius = OS_WINDOW_CORNER_CENTRE - f32::from(panel.left());
    assert!(
        radius > 2.,
        "sidebar inset leaves a concentric radius: {radius}"
    );
    let boundary = radius - (radius.powi(2) - (radius - 0.25).powi(2)).sqrt();
    let contrast = |pixel: [u8; 4]| {
        let numerator: f32 = (0..3)
            .map(|i| {
                (f32::from(pixel[i]) - f32::from(window[i]))
                    * (f32::from(hairline[i]) - f32::from(window[i]))
            })
            .sum();
        let denominator: f32 = (0..3)
            .map(|i| (f32::from(hairline[i]) - f32::from(window[i])).powi(2))
            .sum();
        numerator / denominator
    };
    for (delta, inside) in [(-1., false), (1., true)] {
        let pixel = sample(Point::new(
            panel.left() + px(boundary + delta),
            panel.top() + px(0.25),
        ));
        assert_eq!(
            contrast(pixel) >= 0.5,
            inside,
            "sidebar contour concentric with the window corner"
        );
    }
    // Hue::rgb values and top-lit gradient come from the pinned canonical kit.
    for (id, light, dark_rgb) in [
        ("appearance", 0x5856d6, 0x5e5ce6),
        ("graph", 0xff9500, 0xff9f0a),
        ("editor", 0x30b0c7, 0x40c8e0),
        ("terminal", 0x8e8e93, 0x6e6e73),
        ("network", 0x007aff, 0x0a84ff),
        ("smart_commit", 0xaf52de, 0xbf5af2),
        ("analyze", 0x30b0c7, 0x40c8e0),
        ("general", 0x8e8e93, 0x6e6e73),
    ] {
        let tile = probe(probes, &format!("settings-pane-tile-{id}")).bounds;
        let base = if dark { dark_rgb } else { light };
        let selected = probe(probes, &format!("settings-pane-{id}")).selected;
        let backing = if selected {
            if dark {
                [58, 58, 60]
            } else {
                [211, 211, 211]
            }
        } else {
            sidebar
        };
        assert_tile_gradient(id, tile, base, backing, scale, sample);
    }
}

struct Section {
    title: &'static str,
    group: &'static str,
    controls: &'static [(&'static str, Role, usize)],
}

fn sections(pane: Pane) -> Vec<Section> {
    match pane {
        Pane::Appearance => vec![
            Section {
                title: "Light Mode",
                group: "settings-themes-light-group",
                controls: &[],
            },
            Section {
                title: "Dark Mode",
                group: "settings-themes-dark-group",
                controls: &[],
            },
            Section {
                title: "Accent",
                group: "settings-theme-controls-group",
                controls: &[("settings-accent-", Role::RadioButton, 2)],
            },
            Section {
                title: "Layout",
                group: "settings-layout-group",
                controls: &[
                    ("settings-toolbar-", Role::RadioButton, 2),
                    ("zoom-", Role::Button, 2),
                    ("compact-threshold-", Role::Button, 2),
                ],
            },
            Section {
                title: "Motion",
                group: "settings-motion-group",
                controls: &[("reduce-motion-toggle", Role::Switch, 1)],
            },
            Section {
                title: "Custom themes",
                group: "settings-custom-themes-group",
                controls: &[
                    ("settings-theme-folder-open", Role::Button, 1),
                    ("settings-theme-reload", Role::Button, 1),
                    ("settings-theme-guide", Role::Button, 1),
                ],
            },
        ],
        Pane::Graph => vec![
            Section {
                title: "Commit graph",
                group: "settings-graph-group",
                controls: &[
                    ("compact-toggle", Role::Switch, 1),
                    ("lane-compact-toggle", Role::Switch, 1),
                ],
            },
            Section {
                title: "Clipboard",
                group: "settings-copy-group",
                controls: &[("settings-copy-", Role::RadioButton, 2)],
            },
        ],
        Pane::Editor => vec![Section {
            title: "Markdown",
            group: "settings-editor-group",
            controls: &[("settings-markdown-", Role::RadioButton, 2)],
        }],
        Pane::Terminal => vec![Section {
            title: "Worktree safety",
            group: "settings-terminal-group",
            controls: &[("terminal-auto-lock-toggle", Role::Switch, 1)],
        }],
        Pane::Network => vec![Section {
            title: "Background sync",
            group: "settings-network-group",
            controls: &[("auto-fetch-toggle", Role::Switch, 1)],
        }],
        Pane::SmartCommit => vec![Section {
            title: "Message generation",
            group: "settings-smart-group",
            controls: &[
                ("sc-enabled-toggle", Role::Switch, 1),
                ("sc-provider-select", Role::ComboBox, 1),
                ("sc-model-select", Role::ComboBox, 1),
            ],
        }],
        Pane::Analyze => vec![Section {
            title: "Analyze ignore",
            group: "settings-analyze-group",
            controls: &[
                ("settings-analyze-input", Role::TextInput, 1),
                ("settings-analyze-", Role::Button, 2),
            ],
        }],
        Pane::General => vec![Section {
            title: "Language",
            group: "settings-language-group",
            controls: &[("settings-language-", Role::RadioButton, 2)],
        }],
    }
}

fn canonical_section_title(title: &'static str) -> &'static str {
    if kagi::ui::i18n::lang() != kagi::ui::i18n::Lang::Ja {
        return title;
    }
    match title {
        "Light Mode" => "ライトモード",
        "Dark Mode" => "ダークモード",
        "Accent" => "アクセント",
        "Layout" => "レイアウト",
        "Motion" => "モーション",
        "Commit graph" => "commit グラフ",
        "Clipboard" => "クリップボード",
        "Worktree safety" => "worktree の安全性",
        "Background sync" => "バックグラウンド同期",
        "Message generation" => "メッセージ生成",
        "Custom themes" => "自作テーマ",
        "Language" => "言語",
        "Analyze ignore" => "分析の除外設定",
        "Markdown" => title,
        _ => panic!("canonical section needs an explicit Japanese text oracle: {title}"),
    }
}

pub fn assert_composition(
    window: &Window,
    cx: &App,
    view: &SettingsView,
    traffic_lights: &[[f64; 4]],
    sample: impl Fn(Point<Pixels>) -> [u8; 4],
) {
    let probes = view.control_probes();
    let viewport = Bounds::new(Point::default(), window.viewport_size());
    let frame = window.window_bounds().get_bounds();
    let screen = window.display(cx).unwrap().visible_bounds();
    near(
        viewport.size.width,
        px(900.).min(screen.size.width - (frame.size.width - viewport.size.width)),
        "canonical fixed native width, reduced only for screen containment",
    );
    near(
        viewport.size.height,
        px(760.).min(screen.size.height - (frame.size.height - viewport.size.height)),
        "canonical fixed native height on shortest and tallest panes",
    );
    assert!(frame.size.width >= viewport.size.width);
    contains(
        screen,
        frame,
        "entire native frame fits its visible display",
    );

    let title = probe(&probes, "settings-pane-title");
    near(
        title.bounds.top(),
        viewport.top(),
        "detail title starts at native window top; no host band",
    );
    near(
        title.bounds.size.height,
        px(52.),
        "canonical detail title allocation",
    );
    near(
        title.bounds.right(),
        viewport.right(),
        "detail title reaches trailing window edge",
    );
    assert_eq!(title.name, view.pane().title());
    assert_eq!(title.role, Role::Heading);
    let paint = title.paint.expect("native title's resolved paint style");
    near(paint.font_size, px(15.), "canonical title type size");
    assert_eq!(paint.font_weight, FontWeight::BOLD);
    assert_eq!(
        probes
            .iter()
            .filter(|p| p.id == "settings-pane-title")
            .count(),
        1
    );

    // The panel canvas measures its padding box; its observed 1pt border lies
    // outside it. Check against the independently measured scroll owner too.
    let panel = probe(&probes, "settings-sidebar-panel")
        .bounds
        .dilate(px(1.));
    let sidebar = probe(&probes, "settings-sidebar").bounds;
    near(
        panel.left(),
        viewport.left() + px(8.),
        "sidebar leading window inset",
    );
    near(
        panel.top(),
        viewport.top() + px(8.),
        "sidebar top window inset",
    );
    near(
        panel.bottom(),
        viewport.bottom() - px(8.),
        "sidebar follows native window height",
    );
    near(panel.size.width, px(212.), "canonical panel width");
    near(
        sidebar.left(),
        panel.left(),
        "sidebar scroll/panel leading alignment",
    );
    near(
        sidebar.right(),
        panel.right(),
        "sidebar scroll/panel trailing alignment",
    );
    near(
        title.bounds.left(),
        panel.right() + px(8.),
        "detail begins after inset sidebar",
    );

    let order = [
        "appearance",
        "graph",
        "editor",
        "terminal",
        "network",
        "smart_commit",
        "analyze",
        "general",
    ];
    let mut rows = Vec::new();
    for id in order {
        let row = probe(&probes, &format!("settings-pane-{id}"));
        assert_eq!(row.role, Role::Tab);
        contains(
            panel,
            row.bounds,
            "sidebar destination belongs to its panel",
        );
        near(
            row.bounds.size.height,
            px(32.),
            "canonical sidebar row height",
        );
        near(
            row.bounds.left(),
            panel.left() + px(9.),
            "sidebar border plus horizontal padding",
        );
        let tile = probe(&probes, &format!("settings-pane-tile-{id}"));
        near(tile.bounds.size.width, px(20.), "canonical tile width");
        near(tile.bounds.size.height, px(20.), "canonical tile height");
        near(
            tile.bounds.left(),
            row.bounds.left() + px(8.),
            "canonical row leading padding",
        );
        near(
            tile.bounds.center().y,
            row.bounds.center().y,
            "tile is centered in its real row",
        );
        let label = probe(&probes, &format!("settings-pane-{id}-label"));
        near(
            label.bounds.left() - tile.bounds.right(),
            px(9.),
            "canonical tile-to-label gap",
        );
        assert_eq!(
            label.paint.expect("actual sidebar label paint").font_weight,
            if row.selected {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            },
            "selection changes sidebar emphasis, never category identity"
        );
        if let Some(previous) = rows.last() {
            let previous: &&ControlProbe = previous;
            near(
                row.bounds.top() - previous.bounds.top(),
                px(34.),
                "all rows share canonical pitch/order",
            );
        }
        rows.push(row);
    }
    near(
        rows[0].bounds.top() - panel.top(),
        px(41.),
        "traffic-light top padding plus border",
    );
    assert_eq!(rows.iter().filter(|row| row.selected).count(), 1);
    assert!(rows
        .iter()
        .find(|row| row.selected)
        .unwrap()
        .id
        .ends_with(view.pane().id()));
    let selected = rows.iter().find(|row| row.selected).unwrap().bounds;
    let pixel = sample(Point::new(selected.left() + px(3.), selected.center().y));
    let expected = if kagi::ui::theme::theme().dark {
        [58_u8, 58, 60]
    } else {
        [211_u8, 211, 211]
    };
    for (got, want) in pixel[..3].iter().zip(expected) {
        assert!(
            got.abs_diff(want) <= 2,
            "canonical neutral selection: {pixel:?}"
        );
    }
    assert_sidebar_paint(panel, &probes, window.scale_factor(), &sample);
    assert_eq!(traffic_lights.len(), 3);
    for &[x, y, width, height] in traffic_lights {
        let light = Bounds::new(
            Point::new(px(x as f32), px(y as f32)),
            gpui::size(px(width as f32), px(height as f32)),
        );
        contains(
            panel,
            light,
            "actual OS traffic light lies in sidebar panel",
        );
        assert!(
            light.bottom() < rows[0].bounds.top(),
            "OS traffic light stays above first row"
        );
    }
    // The close button sits on the panel corner's bisector: equal gaps to the
    // panel's top and leading edges (owner report, PR #1818: it touched the
    // leading border at an 8 pt top gap).
    let close = traffic_lights
        .iter()
        .min_by(|a, b| a[0].total_cmp(&b[0]))
        .unwrap();
    near(
        px(close[0] as f32) - panel.left(),
        px(close[1] as f32) - panel.top(),
        "close button equidistant from sidebar panel top and leading edges",
    );

    let scroll = probe(&probes, "settings-detail-scroll").bounds;
    near(
        scroll.top(),
        title.bounds.bottom(),
        "detail scrolling begins after its sole title",
    );
    near(
        scroll.bottom(),
        viewport.bottom(),
        "detail viewport follows native height",
    );
    let expected_sections = sections(view.pane());
    let actual_groups: Vec<_> = probes
        .iter()
        .filter(|p| p.id.ends_with("-group") && p.role == Role::Group)
        .collect();
    assert_eq!(
        actual_groups.len(),
        expected_sections.len(),
        "one group per canonical section"
    );
    assert_eq!(
        probes
            .iter()
            .filter(|p| p.id.starts_with("settings-section-"))
            .count(),
        expected_sections.len()
    );
    let mut previous_bottom = title.bounds.bottom() - px(20.);
    for section in &expected_sections {
        let canonical_title = canonical_section_title(section.title);
        let section_bounds = probe(&probes, &format!("settings-section-{canonical_title}")).bounds;
        let heading = probe(&probes, &format!("settings-heading-{canonical_title}"));
        assert_eq!(heading.role, Role::Heading);
        assert_eq!(heading.name, canonical_title);
        near(
            section_bounds.top() - previous_bottom,
            px(20.),
            "canonical section sequence and gap",
        );
        previous_bottom = section_bounds.bottom();
        let group = probe(&probes, section.group).bounds;
        contains(section_bounds, group, "card belongs to its titled section");
        near(
            group.left() - scroll.left(),
            px(6.),
            "detail leading content inset",
        );
        near(
            scroll.right() - group.right(),
            px(18.),
            "detail trailing content inset",
        );
        near(
            heading.bounds.left() - group.left(),
            px(if kagi::ui::theme::theme().dark {
                12.
            } else {
                13.
            }),
            "heading and group row inset alignment",
        );
        let header_bottom = match section.title {
            "Custom themes" => probe(&probes, "settings-custom-themes-help")
                .bounds
                .bottom(),
            "Analyze ignore" => probe(&probes, "settings-analyze-help").bounds.bottom(),
            _ => heading.bounds.bottom(),
        };
        near(
            group.top() - header_bottom,
            px(8.),
            "section heading/help to card gap",
        );
        assert_group_relations(&probes, section.group, scroll, &sample);
        for &(prefix, role, count) in section.controls {
            let controls: Vec<_> = probes
                .iter()
                .filter(|p| p.id.starts_with(prefix) && p.role == role)
                .collect();
            assert_eq!(
                controls.len(),
                count,
                "{} trailing part {prefix}/{role:?}",
                section.title
            );
            for control in controls {
                contains(
                    group,
                    control.bounds,
                    "actual control belongs to the section's card",
                );
            }
        }
    }
    if view.pane() == Pane::Appearance {
        use gpui_component::scroll::ScrollbarHandle as _;
        near(
            scroll.top() + view.scroll_handle().content_size().height - previous_bottom,
            px(20.),
            "final section leaves canonical bottom inset in scrolling content",
        );
        assert_theme_groups(view, &probes);
    }
    if view.pane() == Pane::SmartCommit {
        for id in ["sc-provider-select", "sc-model-select"] {
            let hit = probe(&probes, id).bounds;
            let visual = probe(&probes, &format!("{id}-visual")).bounds;
            let chevron = probe(&probes, &format!("{id}-chevron")).bounds;
            near(visual.size.height, px(24.), "canonical popup visual height");
            near(chevron.size.width, px(18.), "canonical popup chevron width");
            near(
                chevron.size.height,
                px(18.),
                "canonical popup chevron height",
            );
            near(
                chevron.center().y,
                visual.center().y,
                "popup chevron center line",
            );
            contains(hit, visual, "popup visual fits actual hit target");
            contains(
                visual,
                chevron,
                "filled chevron belongs to otherwise unfilled trigger",
            );
            assert_eq!(
                sample(Point::new(visual.left() + px(3.), visual.center().y)),
                sample(Point::new(visual.left() - px(3.), visual.center().y)),
                "rest popup trigger has owning card fill"
            );
        }
    }
    if view.pane() == Pane::Analyze {
        let input = probe(&probes, "settings-analyze-input").bounds;
        let expected = if kagi::ui::theme::theme().dark {
            [48, 48, 51]
        } else {
            [230, 230, 230]
        };
        assert_rgb(
            sample(Point::new(input.right() - px(20.), input.center().y)),
            expected,
            "canonical multiline_value uses the inset control fill, not the generic Input fill",
        );
    }
    assert_canonical_text(view, &probes);
}

fn assert_group_relations(
    probes: &[ControlProbe],
    id: &str,
    viewport: Bounds<Pixels>,
    sample: &impl Fn(Point<Pixels>) -> [u8; 4],
) {
    let group = probe(probes, id).bounds;
    let dark = kagi::ui::theme::theme().dark;
    let border = px(if dark { 0. } else { 1. });
    let row_count = match id {
        "settings-layout-group" | "settings-smart-group" => 3,
        "settings-graph-group" | "settings-custom-themes-group" | "settings-analyze-group" => 2,
        _ => 1,
    };
    let rows: Vec<_> = (0..row_count)
        .map(|index| probe(probes, &format!("{id}-row-{index}")).bounds)
        .collect();
    let row_prefix = format!("{id}-row-");
    let separator_prefix = format!("{id}-separator-");
    assert_eq!(
        probes
            .iter()
            .filter(|p| p.id.starts_with(&row_prefix))
            .count(),
        row_count,
        "{id}: canonical direct-row membership"
    );
    assert_eq!(
        probes
            .iter()
            .filter(|p| p.id.starts_with(&separator_prefix))
            .count(),
        row_count - 1,
        "{id}: exactly one separator per adjacent row pair"
    );
    let sum = rows
        .iter()
        .fold(px(0.), |height, row| height + row.size.height);
    near(
        group.size.height - sum,
        px((row_count - 1) as f32) + border * 2.,
        "card height equals actual rows plus in-flow separators and appearance-specific borders",
    );
    near(
        rows[0].top(),
        group.top() + border,
        "first row follows actual top border",
    );
    near(
        rows.last().unwrap().bottom(),
        group.bottom() - border,
        "last row precedes actual bottom border",
    );
    for (index, row) in rows.iter().enumerate() {
        near(
            row.size.width,
            group.size.width - px(24.) - border * 2.,
            "content width equals card minus padding and actual borders",
        );
        near(
            row.left(),
            group.left() + px(12.) + border,
            "row leading padding and actual border",
        );
        near(
            row.right(),
            group.right() - px(12.) - border,
            "row trailing padding and actual border",
        );
        if index > 0 {
            let separator = probe(probes, &format!("{id}-separator-{}", index - 1)).bounds;
            near(
                separator.size.height,
                px(1.),
                "one-pixel hairline thickness",
            );
            near(
                separator.left(),
                row.left(),
                "separator shares content inset",
            );
            near(
                separator.size.width,
                row.size.width,
                "separator spans the real content width",
            );
            near(
                separator.top(),
                rows[index - 1].bottom(),
                "separator follows previous row in flow",
            );
            near(
                separator.bottom(),
                row.top(),
                "separator occupies space before next row",
            );
        }
    }
    if group.top() >= viewport.top() && group.bottom() <= viewport.bottom() {
        let fill = if dark { [28, 28, 30] } else { [255, 255, 255] };
        let edge = if dark { fill } else { [220, 220, 220] };
        assert_rgb(
            sample(Point::new(group.left() + px(0.25), group.center().y)),
            edge,
            "Light hairline / Dark unbordered card edge",
        );
        assert_rgb(
            sample(Point::new(group.left() + px(2.), group.center().y)),
            fill,
            "canonical group fill",
        );
        let background: [u8; 3] = if dark { [16, 16, 18] } else { [247, 247, 247] };
        let boundary = 10. - (10_f32.powi(2) - 9.75_f32.powi(2)).sqrt();
        for (delta, inside) in [(-1., false), (1., true)] {
            let pixel = sample(Point::new(
                group.left() + px(boundary + delta),
                group.top() + px(0.25),
            ));
            let contrast: f32 = (0..3)
                .map(|i| {
                    (f32::from(pixel[i]) - f32::from(background[i]))
                        * (f32::from(edge[i]) - f32::from(background[i]))
                })
                .sum::<f32>()
                / (0..3)
                    .map(|i| (f32::from(edge[i]) - f32::from(background[i])).powi(2))
                    .sum::<f32>();
            assert_eq!(contrast >= 0.5, inside, "10pt group contour");
        }
    }
}

fn assert_canonical_text(view: &SettingsView, probes: &[ControlProbe]) {
    static TEXT: std::sync::LazyLock<serde_json::Value> = std::sync::LazyLock::new(|| {
        serde_json::from_str(include_str!("settings_text.json"))
            .expect("canonical Settings text oracle")
    });
    let locale = if kagi::ui::i18n::lang() == kagi::ui::i18n::Lang::Ja {
        "ja"
    } else {
        "en"
    };
    for row in TEXT["rows"].as_array().unwrap() {
        if row["pane"].as_str() != Some(view.pane().id()) {
            continue;
        }
        let id = row["id"][locale].as_str().unwrap();
        let actual = probe(probes, id);
        let measured = match row["field"].as_str().unwrap() {
            "name" => actual.name.as_str(),
            "value" => actual.value.as_str(),
            field => panic!("unknown canonical text probe field {field}"),
        };
        assert_eq!(
            measured,
            row["text"][locale].as_str().unwrap(),
            "canonical1c6486202 visible text: {locale}/{}/{id}",
            view.pane().id()
        );
    }
}

fn assert_theme_groups(view: &SettingsView, probes: &[ControlProbe]) {
    for (dark, group_id, strip_id) in [
        (
            false,
            "settings-themes-light-group",
            "settings-themes-light-scroll",
        ),
        (
            true,
            "settings-themes-dark-group",
            "settings-themes-dark-scroll",
        ),
    ] {
        let group = probe(probes, group_id).bounds;
        let strip = probe(probes, strip_id).bounds;
        contains(group, strip, "theme viewport is inside its own group card");
        let mut cards: Vec<_> = probes
            .iter()
            .filter(|p| {
                p.role == Role::RadioButton
                    && view.snapshot().themes.iter().any(|theme| {
                        theme.dark == dark && p.id == format!("settings-theme-{}", theme.slug)
                    })
            })
            .collect();
        cards.sort_by(|a, b| a.bounds.left().partial_cmp(&b.bounds.left()).unwrap());
        assert!(
            cards.len() >= 4,
            "real catalog supplies the four-card canonical comparison"
        );
        for (index, card) in cards.iter().enumerate() {
            near(
                card.bounds.size.width,
                px(132.),
                "canonical theme card width",
            );
            assert!(
                card.bounds.top() >= group.top() && card.bounds.bottom() <= group.bottom(),
                "theme card vertically contained in owning group"
            );
            let slug = card.id.strip_prefix("settings-theme-").unwrap();
            let preview = probe(probes, &format!("settings-theme-preview-{slug}")).bounds;
            near(
                preview.size.width + px(4.),
                card.bounds.size.width,
                "preview border fits real card width",
            );
            near(
                preview.size.height + px(4.),
                px(84.),
                "canonical preview outer height",
            );
            let caption = probe(probes, &format!("settings-theme-caption-{slug}")).bounds;
            near(
                caption.top() - preview.bottom() - px(2.),
                px(6.),
                "canonical preview/caption gap",
            );
            near(
                card.bounds.bottom(),
                caption.bottom(),
                "canonical card height follows its real caption, without fixed-height slack",
            );
            if index > 0 {
                near(
                    card.bounds.left() - cards[index - 1].bounds.right(),
                    px(8.),
                    "canonical visible card gap survives focus clearance",
                );
            }
        }
        let complete = cards
            .iter()
            .filter(|card| {
                card.bounds.left() >= strip.left() && card.bounds.right() <= strip.right()
            })
            .count();
        assert_eq!(
            complete, 4,
            "four complete previews visible at the actual900pt native width"
        );
    }
}
