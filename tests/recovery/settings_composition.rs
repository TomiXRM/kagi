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
    assert!(
        viewport.size.width >= px(900.),
        "canonical native width: {viewport:?}"
    );
    assert!(frame.size.width >= viewport.size.width);
    contains(
        screen,
        frame,
        "entire native frame fits its visible display",
    );
    assert!(viewport.size.height >= px(500.).min(screen.size.height));

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
        let section_bounds = probe(&probes, &format!("settings-section-{}", section.title)).bounds;
        let heading = probe(&probes, &format!("settings-heading-{}", section.title));
        assert_eq!(heading.role, Role::Heading);
        assert_eq!(heading.name, section.title);
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
            px(13.),
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
        near(
            scroll.bottom() - previous_bottom,
            px(20.),
            "final section leaves canonical bottom inset",
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
