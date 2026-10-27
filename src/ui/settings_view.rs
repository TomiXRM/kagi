//! Native Settings adapter: retained portable view, existing preference writers and host effects.
use super::{
    i18n::{self, Msg},
    settings, theme, KagiApp,
};
#[cfg(feature = "gui-e2e")]
use gpui::App;
use gpui::{div, prelude::*, AnyElement, Context, Entity, MouseButton, SharedString, Window};
use gpui_component::{button::Button, input::InputState, FocusTrapElement as _, Sizable as _};
use kagi_ui_settings::{
    Pane, ProviderAvailability, SettingsEvent, SettingsIntent, SettingsSnapshot, SettingsView,
    SmartProvider,
};

#[cfg(all(target_os = "macos", feature = "gui-e2e"))]
#[derive(Default)]
pub struct SettingsHostForE2e {
    pub native: bool,
}
#[cfg(all(target_os = "macos", feature = "gui-e2e"))]
impl gpui::Global for SettingsHostForE2e {}

impl KagiApp {
    pub fn settings_snapshot(&self) -> SettingsSnapshot {
        let palette = theme::theme();
        let smart = &self.smart_commit;
        let provider = match smart.provider.slug() {
            "claude-code" => SmartProvider::ClaudeCode,
            "codex" => SmartProvider::Codex,
            _ => SmartProvider::Ollama,
        };
        let folder =
            theme::themes_dir().map(|path| SharedString::from(path.to_string_lossy().into_owned()));
        let preferences = settings::Settings::load();
        SettingsSnapshot {
            language: i18n::lang(),
            themes: theme::all_themes(),
            theme_slug: palette.slug.to_string().into(),
            accent_applicable: cfg!(target_os = "macos")
                && matches!(&*palette.slug, "apple-light" | "apple-dark"),
            follows_system_accent: theme::follows_system_accent(),
            effective_accent: palette.ui_accent(),
            accent_description: theme::detected_system_accent().detected_label().into(),
            can_open_theme_folder: folder.is_some(),
            theme_folder: folder,
            toolbar_display: settings::toolbar_display(&palette.slug),
            zoom: theme::zoom(),
            compact_threshold_rem: theme::compact_threshold_rem(),
            reduce_motion: theme::app_reduce_motion(),
            graph_compact: theme::compact_graph(),
            graph_lane_compact: theme::graph_lane_compact(),
            graph_copy_target: preferences.graph_copy_target(),
            markdown_default_view: preferences.editor_markdown_default_view(),
            terminal_auto_lock: settings::terminal_auto_lock(),
            auto_fetch: theme::auto_fetch(),
            smart_enabled: smart.llm_enabled,
            smart_consent_accepted: smart.consent_accepted,
            smart_provider: provider,
            smart_ollama_available: smart.ollama_available,
            smart_detecting: smart.detecting,
            providers: [
                (SmartProvider::Ollama, true),
                (SmartProvider::ClaudeCode, smart.claude_available),
                (SmartProvider::Codex, smart.codex_available),
            ]
            .into_iter()
            .map(|(provider, available)| ProviderAvailability {
                provider,
                available,
                reason: (!available)
                    .then(|| i18n::provider_not_found_hint(provider.display_name()).into()),
            })
            .collect(),
            smart_model: smart.model.clone().map(Into::into),
            models: smart
                .detected_models
                .iter()
                .cloned()
                .map(Into::into)
                .collect(),
            analyze_saved: self
                .settings_controls
                .saved_ignore
                .clone()
                .unwrap_or_default()
                .into(),
        }
    }

    pub fn seed_settings(&self, cx: &mut Context<Self>) {
        if let Some(view) = &self.settings_controls.view {
            let snapshot = self.settings_snapshot();
            view.update(cx, |view, cx| view.seed_effective(snapshot, cx));
        }
        let origin = cx.entity_id();
        cx.defer(move |cx| {
            for handle in cx.windows() {
                let _ = handle.update(cx, |_, window, cx| {
                    if let Some(owner) = super::repository_window(window, cx)
                        .filter(|owner| owner.entity_id() != origin)
                    {
                        let app = owner.read(cx);
                        if let Some(view) = app.settings_entity() {
                            let snapshot = app.settings_snapshot();
                            view.update(cx, |view, cx| view.seed_effective(snapshot, cx));
                        }
                    }
                });
            }
        });
    }

    pub fn ensure_analyze_ignore_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = &self.settings_controls.view {
            view.update(cx, |view, cx| view.prepare_open(cx));
            self.seed_settings(cx);
            return;
        }
        let content = settings::read_analyze_ignore_text();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .auto_grow(8, 22)
        });
        input.update(cx, |input, cx| input.set_value(content.clone(), window, cx));
        self.settings_controls.saved_ignore = Some(content);
        self.analyze_ignore_input = Some(input.clone());
        let stored = settings::read_setting("settings_pane");
        let pane = stored
            .as_deref()
            .and_then(Pane::from_id)
            .unwrap_or_else(|| {
                Pane::from_legacy(stored.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0))
            });
        if stored.as_deref().is_some_and(|stored| stored != pane.id()) {
            settings::write_setting("settings_pane", Some(pane.id()));
        }
        let snapshot = self.settings_snapshot();
        let view = cx.new(|cx| SettingsView::new(snapshot, input, pane, cx));
        cx.subscribe(&view, |_, _, event: &SettingsEvent, cx| {
            let event = event.clone();
            let owner = cx.entity().downgrade();
            cx.defer(move |cx| {
                if let Err(error) = event.window.update(cx, |_, window, cx| {
                    owner.update(cx, |app, cx| {
                        app.apply_settings_intent(event.intent, window, cx)
                    })
                }) {
                    eprintln!("Settings request window closed: {error}");
                }
            });
        })
        .detach();
        let owner = cx.entity().downgrade();
        view.update(cx, |view, cx| {
            view.set_detail_slot(
                std::rc::Rc::new(move |window, cx| {
                    owner
                        .update(cx, |app, cx| {
                            let toasts = app.render_toasts(
                                window,
                                super::toast_stack::ToastAudience::Settings,
                                cx,
                            );
                            Some(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .child(super::e2e::measure_inside("toast-content-anchor"))
                                    .children(toasts)
                                    .into_any_element(),
                            )
                        })
                        .ok()
                        .flatten()
                }),
                cx,
            )
        });
        self.settings_controls.view = Some(view);
    }

    fn apply_settings_intent(
        &mut self,
        intent: SettingsIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match intent {
            SettingsIntent::Pane(pane) => {
                settings::write_setting("settings_pane", Some(pane.id()));
                return;
            }
            SettingsIntent::Language(lang) => return self.set_lang(lang, cx),
            SettingsIntent::Theme(slug) => return self.set_theme(&slug, cx),
            SettingsIntent::SystemAccent(on) => {
                theme::set_follows_system_accent(on);
                theme::sync_gpui_component_theme(cx);
                cx.refresh_windows();
            }
            SettingsIntent::Toolbar(mode) => {
                settings::set_toolbar_display(mode);
                super::commands::publish_menus(cx);
            }
            SettingsIntent::Zoom(zoom) => {
                theme::set_zoom(zoom);
                theme::sync_gpui_component_theme(cx);
                self.apply_terminal_config(cx);
                cx.refresh_windows();
            }
            SettingsIntent::CompactThreshold(value) => {
                theme::set_compact_threshold_rem(value);
                cx.refresh_windows();
            }
            SettingsIntent::ReduceMotion(on) => theme::set_reduce_motion(on),
            SettingsIntent::GraphCompact(on) => {
                self.graph_compact = on;
                theme::set_compact_graph(on);
            }
            SettingsIntent::GraphLaneCompact(on) => {
                theme::set_graph_lane_compact(on);
                self.reload(cx);
            }
            SettingsIntent::GraphCopyTarget(target) => settings::write_setting(
                "graph_copy_target",
                Some(match target {
                    settings::CopyTarget::Hash => "hash",
                    settings::CopyTarget::Branch => "branch",
                }),
            ),
            SettingsIntent::MarkdownDefaultView(mode) => {
                settings::write_setting("editor_markdown_default_view", Some(mode.as_str()));
                cx.refresh_windows();
            }
            SettingsIntent::TerminalAutoLock(on) => settings::set_terminal_auto_lock(on),
            SettingsIntent::AutoFetch(on) => theme::set_auto_fetch(on),
            SettingsIntent::SmartEnabled(on) => self.request_settings_smart_enabled(on, window, cx),
            SettingsIntent::SmartConsentRequested => {
                self.request_settings_smart_enabled(true, window, cx)
            }
            SettingsIntent::SmartProvider(provider) => {
                self.smart_commit
                    .set_provider(super::smart_commit::SmartProvider::from_slug(
                        provider.slug(),
                    ))
            }
            SettingsIntent::SmartModel(model) => self.smart_commit.set_model(model.to_string()),
            SettingsIntent::OpenThemeFolder => return self.open_themes_folder(cx),
            SettingsIntent::ReloadThemes => return self.reload_themes(window, cx),
            SettingsIntent::OpenThemeGuide => {
                return cx.open_url("https://github.com/TomiXRM/kagi/blob/main/docs/themes.md")
            }
            SettingsIntent::SaveAnalyze(request) => {
                return self.save_analyze_request(request, false, window, cx)
            }
            SettingsIntent::ResetAnalyze(request) => {
                self.settings_controls.reset_request = Some(request);
                self.request_analyze_reset(window, cx);
                return;
            }
        }
        self.seed_settings(cx);
        cx.notify();
    }

    pub fn settings_entity(&self) -> Option<Entity<SettingsView>> {
        self.settings_controls.view.clone()
    }
}

pub(super) fn render_settings_overlay(
    app: Entity<KagiApp>,
    view: Option<Entity<SettingsView>>,
    trap: Option<gpui::FocusHandle>,
    reset: Option<super::settings_analyze::AnalyzeResetAlert>,
    consent: Option<super::settings_controls::SmartConsentAlert>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let dismiss = cx.listener(|this, _: &gpui::MouseDownEvent, window, cx| {
        this.menu_overlay = None;
        this.clear_settings_smart_consent(window.window_handle().window_id());
        this.clear_analyze_reset_alert(window.window_handle().window_id());
        cx.stop_propagation();
        cx.notify();
    });
    let viewport = window.viewport_size();
    let panel = div()
        .relative()
        .w(gpui::px((f32::from(viewport.width) * 0.85).min(820.)))
        .h(gpui::px((f32::from(viewport.height) * 0.85).min(720.)))
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(gpui::px(12.))
        .bg(gpui::rgb(theme::theme().panel))
        .child(super::e2e::measure_inside("settings-frame"))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex()
                .justify_between()
                .px_4()
                .py_3()
                .child(Msg::SettingsTitle.t())
                .child(super::e2e::measure_control(
                    "settings-close",
                    Button::new("settings-close")
                        .label("×")
                        .small()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.menu_overlay = None;
                            this.clear_settings_smart_consent(window.window_handle().window_id());
                            this.clear_analyze_reset_alert(window.window_handle().window_id());
                            cx.notify();
                        })),
                )),
        )
        .children(view)
        .when_some(consent, |el, alert| {
            el.child(super::settings_controls::smart_consent_alert(
                &app, alert, cx,
            ))
        });
    div()
        .absolute()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .absolute()
                .size_full()
                .bg(gpui::rgb(theme::theme().bg_base))
                .opacity(0.6)
                .occlude()
                .on_mouse_down(MouseButton::Left, dismiss),
        )
        .child(match trap {
            Some(trap) => panel.focus_trap("settings-trap", &trap).into_any_element(),
            None => panel.into_any_element(),
        })
        .when_some(reset, |el, alert| {
            el.child(super::settings_analyze::reset_alert(&app, alert, cx))
        })
        .into_any_element()
}

#[cfg(feature = "gui-e2e")]
pub fn settings_view_for_window_for_e2e(window: &Window, cx: &App) -> Option<Entity<SettingsView>> {
    #[cfg(target_os = "macos")]
    if let Some(view) = super::settings_window::view_for_window(window, cx) {
        return Some(view);
    }
    super::repository_window(window, cx).and_then(|owner| owner.read(cx).settings_entity())
}
