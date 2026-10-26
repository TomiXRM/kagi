//! Reference gallery for `kagi::ui::settings_kit` (#1799).
//!
//! ```text
//! cargo run --example settings_gallery -- --list
//! KAGI_GALLERY_PAGE=parts KAGI_GALLERY_STATE=hover KAGI_GALLERY_OUT=/tmp/parts.png \
//!   cargo run --example settings_gallery
//! ```
//!
//! `parts` draws every kit part in one forced state; `server` is a complete
//! page (Rex's Server pane) built only from kit parts — the worked example the
//! settings skill points to.
use gpui::{div, prelude::*, AnyElement, App, Window};
use gpui_component::IconName;
use kagi::ui::gallery_host::{self, GalleryContext, GalleryPage};
use kagi::ui::settings_kit::{
    capsule_button, checkbox, detail_page, group, icon_tile, popup_value, progress_bar,
    search_field, section, section_action, settings_window, sidebar, sidebar_gap, sidebar_item,
    stepper, switch, Forced, Hue, Row, Segmented, Slider, SliderEnds, StepperPart, Tint,
};

static PAGES: [GalleryPage; 2] = [
    GalleryPage {
        name: "parts",
        states: &["rest", "hover", "pressed", "focus", "disabled"],
        size: (900., 900.),
        render: parts,
    },
    GalleryPage {
        name: "server",
        states: &["default", "focus-sidebar", "hover-button"],
        size: (900., 692.),
        render: server,
    },
];

fn main() {
    gallery_host::run(&PAGES);
}

/// The sidebar Rex shows on every pane; `selected` names the current item.
fn rex_sidebar(selected: &str, forced: Forced) -> AnyElement {
    let item = |id: &'static str, icon, tint, label: &'static str| {
        let on = id == selected;
        sidebar_item(
            id,
            icon_tile(icon, tint, 20.),
            label,
            None,
            on,
            if on { Forced::Rest } else { forced },
        )
        .into_any_element()
    };
    sidebar([
        search_field("search", "Search Settings").into_any_element(),
        sidebar_item(
            "server",
            icon_tile(IconName::Bot, Tint::Hue(Hue::Purple), 32.),
            "Server",
            Some("Running".into()),
            selected == "server",
            Forced::Rest,
        )
        .into_any_element(),
        sidebar_gap(),
        item(
            "interface",
            IconName::LayoutDashboard,
            Tint::Hue(Hue::Gray),
            "Interface",
        ),
        item(
            "themes",
            IconName::Palette,
            Tint::Hue(Hue::Indigo),
            "Themes",
        ),
        sidebar_gap(),
        item(
            "terminal",
            IconName::SquareTerminal,
            Tint::Hue(Hue::Gray),
            "Terminal",
        ),
        item(
            "integrations",
            IconName::Github,
            Tint::Hue(Hue::Orange),
            "Integrations",
        ),
        item(
            "notifications",
            IconName::Bell,
            Tint::Hue(Hue::Red),
            "Notifications",
        ),
        item(
            "remote",
            IconName::Globe,
            Tint::Hue(Hue::Blue),
            "Remote Hosts",
        ),
        sidebar_gap(),
        item(
            "developer",
            IconName::Settings2,
            Tint::Hue(Hue::Gray),
            "Developer",
        ),
    ])
    .into_any_element()
}

fn server(cx: &GalleryContext, _: &mut Window, _: &mut App) -> AnyElement {
    let sidebar_state = if cx.state == "focus-sidebar" {
        Forced::Focus
    } else {
        Forced::Rest
    };
    let button_state = if cx.state == "hover-button" {
        Forced::Hover
    } else {
        Forced::Rest
    };
    settings_window(
        rex_sidebar("server", sidebar_state),
        detail_page(
            "server-scroll",
            "Server",
            [
                group([Row::new("Rex Server")
                    .subtitle("Running for 5h 9m")
                    .leading(icon_tile(IconName::Bot, Tint::Purple, 32.))
                    .trailing(capsule_button("stop", "Stop Server…", button_state))
                    .into_any_element()])
                .into_any_element(),
                section(
                    "Tailscale",
                    None,
                    None,
                    group([Row::new("Sign in to Tailscale")
                        .subtitle("Access Rex securely from your other devices.")
                        .leading(icon_tile(IconName::Network, Tint::Neutral, 32.))
                        .trailing(capsule_button("sign-in", "Sign In", Forced::Rest))
                        .into_any_element()]),
                ),
                section(
                    "Client Info",
                    None,
                    None,
                    group([
                        Row::new("Label")
                            .trailing(popup_value("label", "tomixrm", Forced::Rest))
                            .into_any_element(),
                        Row::new("Remote Control")
                            .subtitle("Let Rex and other clients perform actions in this app.")
                            .trailing(switch(
                                "remote",
                                "Remote Control",
                                false,
                                Forced::Rest,
                                |_, _, _| {},
                            ))
                            .into_any_element(),
                    ]),
                ),
            ],
        ),
    )
    .into_any_element()
}

fn parts(cx: &GalleryContext, _: &mut Window, _: &mut App) -> AnyElement {
    let state = Forced::parse(cx.state).expect("states are Forced names");
    settings_window(
        rex_sidebar("themes", state),
        detail_page(
            "parts-scroll",
            format!("Parts — {}", state.name()),
            [
                section(
                    "Section",
                    Some("Subtitle in secondary ink".into()),
                    Some(section_action("new", IconName::Plus, "New", state).into_any_element()),
                    group([
                        Row::new("Switch on")
                            .trailing(switch("s-on", "Switch on", true, state, |_, _, _| {}))
                            .into_any_element(),
                        Row::new("Switch off")
                            .subtitle("Row with a subtitle")
                            .trailing(switch("s-off", "Switch off", false, state, |_, _, _| {}))
                            .into_any_element(),
                        Row::new("Segmented")
                            .trailing(
                                Segmented::new("seg", ["Above", "Left Side", "Right Side"], 0)
                                    .forced(1, state),
                            )
                            .into_any_element(),
                        Row::new("Pop-up value")
                            .trailing(popup_value("popup", "Always", state))
                            .into_any_element(),
                        Row::new("Stepper")
                            .trailing(stepper(
                                "zoom",
                                "100%",
                                true,
                                true,
                                Some((StepperPart::Increase, state)),
                            ))
                            .into_any_element(),
                        Row::new("Capsule button")
                            .trailing(capsule_button("import", "Import", state))
                            .into_any_element(),
                        Row::new("Slider")
                            .trailing(
                                Slider::new("volume", 0.75)
                                    .ticks(5)
                                    .ends(SliderEnds::Symbols(IconName::Minus, IconName::Plus))
                                    .forced(state),
                            )
                            .into_any_element(),
                        Row::new("Slider with captions")
                            .trailing(
                                Slider::new("balance", 0.5)
                                    .ends(SliderEnds::Captions("Left".into(), "Right".into())),
                            )
                            .into_any_element(),
                        Row::new("Checkbox")
                            .trailing(checkbox("mute", "Mute", true, state))
                            .into_any_element(),
                        Row::new("Progress")
                            .subtitle("46.5 / 50 GB")
                            .trailing(
                                div()
                                    .w(gpui::px(200.))
                                    .child(progress_bar(0.93, Tint::Success)),
                            )
                            .into_any_element(),
                    ]),
                ),
                section(
                    "Navigation rows",
                    None,
                    None,
                    group([
                        Row::new("Working")
                            .subtitle("Tabs")
                            .leading(icon_tile(IconName::Ellipsis, Tint::Accent, 26.))
                            .navigation("nav-working", state)
                            .into_any_element(),
                        Row::new("Error")
                            .subtitle("Tabs, Notifications, Dock Badge, Sound")
                            .leading(icon_tile(IconName::TriangleAlert, Tint::Danger, 26.))
                            .navigation("nav-error", Forced::Rest)
                            .into_any_element(),
                    ]),
                ),
            ],
        ),
    )
    .into_any_element()
}
