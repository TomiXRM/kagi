use super::*;
pub(super) struct SectionPicker {
    section: usize,
    parent: WeakEntity<Prototype>,
    pub(super) popover: Option<WeakEntity<PopoverState>>,
    pub(super) filter: Entity<InputState>,
    _filter_subscription: Subscription,
}

impl SectionPicker {
    pub(super) fn new(
        section: usize,
        parent: WeakEntity<Prototype>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter…"));
        let subscription = cx.observe(&filter, |this, _, cx| {
            cx.notify();
            let _ = this.parent.update(cx, |_, cx| cx.notify());
        });
        Self {
            section,
            parent,
            popover: None,
            filter,
            _filter_subscription: subscription,
        }
    }

    fn items(&self) -> Vec<SharedString> {
        match self.section {
            0 => branches(),
            1 => ["origin/dev", "origin/main", "origin/feature/compact-layout"]
                .map(Into::into)
                .to_vec(),
            2 => ["kagi · dev", "compact-layout · hig/compact-layout"]
                .map(Into::into)
                .to_vec(),
            3 => ["v0.44.0", "v0.43.0", "v0.42.0"].map(Into::into).to_vec(),
            4 => ["stash@{0}: WIP on dev", "stash@{1}: sidebar polish"]
                .map(Into::into)
                .to_vec(),
            5 => [
                "#1224 Improve Settings grouping",
                "#1222 Sidebar section filters",
            ]
            .map(Into::into)
            .to_vec(),
            _ => ["#1225 Compact layout", "#1200 Settings groups"]
                .map(Into::into)
                .to_vec(),
        }
    }
}

impl Render for SectionPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.filter.read(cx).value().to_lowercase();
        let items = self.items();
        div()
            .w(scaled_px(310.))
            .flex()
            .flex_col()
            .gap_2()
            .font_family(UI_FONT)
            .text_color(rgb(theme().text_main))
            .child(section_header(
                "proto-popover-section",
                format!("{} · {}", SECTIONS[self.section].0, items.len()).into(),
            ))
            .child(Input::new(&self.filter).small())
            .child(
                div()
                    .id("section-scroll")
                    .max_h(scaled_px(360.))
                    .overflow_y_scroll()
                    .children(
                        items
                            .into_iter()
                            .filter(|name| name.to_lowercase().contains(&query))
                            .enumerate()
                            .map(|(i, name)| {
                                div()
                                    .id(("section-item", i))
                                    .px_2()
                                    .h(scaled_px(25.))
                                    .flex()
                                    .items_center()
                                    .text_xs()
                                    .cursor_pointer()
                                    .hover(|el| el.bg(rgb(theme().surface)))
                                    .child(name.clone())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let _ = this.parent.update(cx, |parent, cx| {
                                            parent.selected_ref = name.clone();
                                            cx.notify();
                                        });
                                        if let Some(popover) = &this.popover {
                                            let _ =
                                                popover.update(cx, |p, cx| p.dismiss(window, cx));
                                        }
                                    }))
                            }),
                    ),
            )
    }
}

pub(super) fn branches() -> Vec<SharedString> {
    std::iter::once("dev".into())
        .chain((1..121).map(|i| format!("feature/layout-study-{i:03}").into()))
        .collect()
}
pub(super) fn run() {
    // Environment override chooses tokens without writing the user's Settings.
    if std::env::var_os("KAGI_THEME").is_none() {
        std::env::set_var(
            "KAGI_THEME",
            if std::env::args().any(|a| a == "--dark") {
                "apple-dark"
            } else {
                "apple-light"
            },
        );
    }
    theme::init_active();
    gpui_platform::application()
        .with_assets(KagiAssets)
        .run(|cx| {
            gpui_component::init(cx);
            theme::sync_gpui_component_theme(cx);
            keyboard_nav::bind_keys(cx);
            if let Err(error) = cx.text_system().add_fonts(vec![
                Cow::Borrowed(include_bytes!("../../assets/fonts/Inter-Regular.ttf").as_slice()),
                Cow::Borrowed(include_bytes!("../../assets/fonts/Inter-Bold.ttf").as_slice()),
                Cow::Borrowed(
                    include_bytes!("../../assets/fonts/NotoSansJP-Regular.ttf").as_slice(),
                ),
            ]) {
                eprintln!("prototype fonts: {error}");
            }
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(940.), px(1000.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Kagi — Compact layout prototype".into()),
                    ..Default::default()
                }),
                window_min_size: Some(size(px(620.), px(500.))),
                ..Default::default()
            };
            if let Err(error) = cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| Prototype::new(window, cx));
                let focus = view.read(cx).focus.clone();
                focus.focus(window, cx);
                eprintln!("compact-layout prototype ready");
                cx.new(|cx| Root::new(view, window, cx))
            }) {
                eprintln!("prototype window: {error}");
                cx.quit();
            }
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}
