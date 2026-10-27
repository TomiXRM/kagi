//! Verification host for the production entity, not a second Settings renderer.
//! Browser outcomes are explicitly simulated; no native persistence is claimed.
use gpui::{div, prelude::*, AnyWindowHandle, App, AsyncApp, Context, Entity, SharedString, Subscription, WeakEntity, Window};
use gpui_component::input::InputState;
use kagi_ui_core::{i18n::{self, Lang}, settings::{CopyTarget, MarkdownDefaultView, ToolbarDisplay}, theme};
use kagi_ui_settings::{AnalyzeRequest, AnalyzeResult, Pane, ProviderAvailability, SettingsEvent, SettingsIntent, SettingsSnapshot, SettingsView, SmartProvider};
use serde_json::{json, Value};
use std::cell::RefCell;

thread_local! {
    static HOST: RefCell<Option<(WeakEntity<SettingsFixture>, AsyncApp, AnyWindowHandle)>> = const { RefCell::new(None) };
}

pub fn observed() -> String {
    HOST.with(|host| {
        let Some((owner, app, _)) = host.borrow().clone() else { return "null".into() };
        app.update(|cx| owner.upgrade().map(|owner| owner.read(cx).describe(cx)).unwrap_or(Value::Null)).to_string()
    })
}

/// Setup and simulated host-result injection only. Product input always travels
/// through the real rendered controls, never this function.
pub fn command(command: &str, value: Option<&str>) -> Result<(), String> {
    let (owner, mut app, window) = HOST.with(|host| host.borrow().clone()).ok_or("Settings fixture is not open")?;
    app.update_window(window, |_, window, cx| {
        owner.update(cx, |host, cx| host.command(command, value, window, cx)).map_err(|error| error.to_string())?
    }).map_err(|error| error.to_string())?
}

#[derive(Clone, Copy)]
enum SaveMode { Immediate, Deferred, Failed }

pub struct SettingsFixture {
    view: Entity<SettingsView>,
    input: Entity<InputState>,
    snapshot: SettingsSnapshot,
    focus: gpui::FocusHandle,
    requests: Vec<String>,
    unsupported: Vec<String>,
    save_mode: SaveMode,
    pending_saves: Vec<AnalyzeRequest>,
    pending_reset: Option<AnalyzeRequest>,
    pending_consent: bool,
    visible: bool,
    _events: Subscription,
}

impl SettingsFixture {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        i18n::set_lang_no_persist(Lang::En);
        theme::init_loaded_active("apple-light");
        theme::init_loaded_accent(theme::SystemAccent::Blue, Some(true));
        theme::init_loaded_zoom(1.);
        theme::sync_gpui_component_theme(cx);
        let snapshot = SettingsSnapshot {
            language: Lang::En,
            themes: theme::themes_by_name(),
            theme_slug: "apple-light".into(),
            accent_applicable: true,
            follows_system_accent: true,
            effective_accent: 0x007aff,
            accent_description: "Blue (verification fixture)".into(),
            theme_folder: Some("Native theme directory unavailable in browser fixture".into()),
            can_open_theme_folder: false,
            toolbar_display: ToolbarDisplay::IconAndText,
            zoom: 1., compact_threshold_rem: 70, reduce_motion: false,
            graph_compact: false, graph_lane_compact: false, graph_copy_target: CopyTarget::Hash,
            markdown_default_view: MarkdownDefaultView::Preview, terminal_auto_lock: false, auto_fetch: true,
            smart_enabled: false, smart_consent_accepted: false, smart_provider: SmartProvider::Ollama,
            smart_ollama_available: true, smart_detecting: false,
            providers: vec![
                ProviderAvailability { provider: SmartProvider::Ollama, available: true, reason: None },
                ProviderAvailability { provider: SmartProvider::ClaudeCode, available: false, reason: Some("Claude Code is not installed in this fixture".into()) },
                ProviderAvailability { provider: SmartProvider::Codex, available: true, reason: None },
            ],
            smart_model: Some("qwen3:8b".into()), models: vec!["qwen3:8b".into(), "llama3.2:3b".into()],
            analyze_saved: "node_modules/\ntarget/\n".into(),
        };
        let input = cx.new(|cx| InputState::new(window, cx).multi_line(true).auto_grow(8, 22));
        input.update(cx, |input, cx| input.set_value(snapshot.analyze_saved.clone(), window, cx));
        let view = cx.new(|cx| SettingsView::new(snapshot.clone(), input.clone(), Pane::Appearance, cx));
        let events = cx.subscribe(&view, |host, _, event: &SettingsEvent, cx| {
            host.requests.push(format!("{:?}", event.intent));
            let intent = event.intent.clone();
            let owner = cx.entity().downgrade();
            let window = event.window;
            cx.defer(move |cx| {
                let _ = window.update(cx, |_, window, cx| {
                    let _ = owner.update(cx, |host, cx| host.apply(intent, window, cx));
                });
            });
        });
        HOST.with(|host| *host.borrow_mut() = Some((cx.entity().downgrade(), cx.to_async(), window.window_handle())));
        Self { view, input, snapshot, focus, requests: Vec::new(), unsupported: Vec::new(), save_mode: SaveMode::Immediate, pending_saves: Vec::new(), pending_reset: None, pending_consent: false, visible: true, _events: events }
    }

    fn seed(&mut self, cx: &mut Context<Self>) {
        self.snapshot.effective_accent = theme::theme().ui_accent();
        self.view.update(cx, |view, cx| view.seed_effective(self.snapshot.clone(), cx));
        cx.refresh_windows();
        cx.notify();
    }

    fn save_result(&mut self, result: AnalyzeResult, window: &mut Window, cx: &mut Context<Self>) {
        match &result {
            AnalyzeResult::Saved(request) => self.snapshot.analyze_saved = request.text.clone(),
            AnalyzeResult::Reset { text, .. } => self.snapshot.analyze_saved = text.clone(),
            AnalyzeResult::Failed { .. } => {}
        }
        self.view.update(cx, |view, cx| view.seed_analyze_result(result, window, cx));
        self.seed(cx);
    }

    fn apply(&mut self, intent: SettingsIntent, window: &mut Window, cx: &mut Context<Self>) {
        match intent {
            SettingsIntent::Pane(_) => {},
            SettingsIntent::Language(language) => { self.snapshot.language = language; i18n::set_lang_no_persist(language); },
            SettingsIntent::Theme(slug) => {
                if theme::init_loaded_active(&slug) {
                    self.snapshot.theme_slug = slug;
                    self.snapshot.accent_applicable = self.snapshot.theme_slug.starts_with("apple-");
                    theme::sync_gpui_component_theme(cx);
                }
            },
            SettingsIntent::SystemAccent(value) => {
                self.snapshot.follows_system_accent = value;
                theme::init_loaded_accent(theme::SystemAccent::Blue, Some(value));
                theme::sync_gpui_component_theme(cx);
            },
            SettingsIntent::Toolbar(value) => self.snapshot.toolbar_display = value,
            SettingsIntent::Zoom(value) => { self.snapshot.zoom = theme::init_loaded_zoom(value); theme::sync_gpui_component_theme(cx); },
            SettingsIntent::CompactThreshold(value) => self.snapshot.compact_threshold_rem = value,
            SettingsIntent::ReduceMotion(value) => self.snapshot.reduce_motion = value,
            SettingsIntent::GraphCompact(value) => self.snapshot.graph_compact = value,
            SettingsIntent::GraphLaneCompact(value) => self.snapshot.graph_lane_compact = value,
            SettingsIntent::GraphCopyTarget(value) => self.snapshot.graph_copy_target = value,
            SettingsIntent::MarkdownDefaultView(value) => self.snapshot.markdown_default_view = value,
            SettingsIntent::TerminalAutoLock(value) => self.snapshot.terminal_auto_lock = value,
            SettingsIntent::AutoFetch(value) => self.snapshot.auto_fetch = value,
            SettingsIntent::SmartEnabled(value) => self.snapshot.smart_enabled = value && self.snapshot.smart_consent_accepted,
            SettingsIntent::SmartConsentRequested => self.pending_consent = true,
            SettingsIntent::SmartProvider(value) => self.snapshot.smart_provider = value,
            SettingsIntent::SmartModel(value) => self.snapshot.smart_model = Some(value),
            SettingsIntent::OpenThemeFolder => self.unsupported.push("Finder opening is unsupported in browser verification".into()),
            SettingsIntent::ReloadThemes => self.unsupported.push("Native filesystem theme discovery is unsupported in browser verification".into()),
            SettingsIntent::OpenThemeGuide => self.unsupported.push("External navigation is not performed by this fixture".into()),
            SettingsIntent::SaveAnalyze(request) => match self.save_mode {
                SaveMode::Immediate => self.save_result(AnalyzeResult::Saved(request), window, cx),
                SaveMode::Deferred => self.pending_saves.push(request),
                SaveMode::Failed => self.save_result(AnalyzeResult::Failed { request, error: "Simulated host write failure: permission denied".into() }, window, cx),
            },
            SettingsIntent::ResetAnalyze(request) => self.pending_reset = Some(request),
        }
        self.seed(cx);
    }

    fn command(&mut self, command: &str, value: Option<&str>, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        match command {
            "save-mode" => self.save_mode = match value { Some("immediate") => SaveMode::Immediate, Some("deferred") => SaveMode::Deferred, Some("failed") => SaveMode::Failed, _ => return Err("Unknown save mode".into()) },
            "ack" => {
                if self.pending_saves.is_empty() { return Err("No pending save".into()); }
                let request = self.pending_saves.remove(0);
                let result = if value == Some("failure") { AnalyzeResult::Failed { request, error: "Simulated delayed write failure".into() } } else { AnalyzeResult::Saved(request) };
                self.save_result(result, window, cx);
            },
            "consent" => {
                if !self.pending_consent { return Err("No pending consent request".into()); }
                self.pending_consent = false;
                if value == Some("accept") { self.snapshot.smart_consent_accepted = true; self.snapshot.smart_enabled = true; }
            },
            "reset" => {
                let request = self.pending_reset.take().ok_or("No pending reset request")?;
                if value == Some("accept") { self.save_result(AnalyzeResult::Reset { request, text: "node_modules/\ntarget/\n".into() }, window, cx); }
            },
            "close" => { self.visible = false; self.pending_reset = None; self.pending_consent = false; },
            "reopen" => { self.visible = true; window.focus(&self.focus, cx); },
            "appearance" => {
                let slug = value.ok_or("Missing theme slug")?;
                if !theme::init_loaded_active(slug) { return Err("Unknown theme slug".into()); }
                self.snapshot.theme_slug = slug.to_owned().into();
                self.snapshot.accent_applicable = slug.starts_with("apple-");
                theme::sync_gpui_component_theme(cx);
            },
            "language" => { self.snapshot.language = match value { Some("en") => Lang::En, Some("ja") => Lang::Ja, _ => return Err("Unknown language".into()) }; i18n::set_lang_no_persist(self.snapshot.language); },
            "zoom" => { let zoom: f32 = value.ok_or("Missing zoom")?.parse().map_err(|_| "Invalid zoom")?; self.snapshot.zoom = theme::init_loaded_zoom(zoom); theme::sync_gpui_component_theme(cx); },
            "models" => { self.snapshot.models = serde_json::from_str::<Vec<String>>(value.ok_or("Missing model array")?).map_err(|error| error.to_string())?.into_iter().map(SharedString::from).collect(); },
            "reseed" => {},
            _ => return Err(format!("Unsupported fixture command: {command}")),
        }
        self.seed(cx);
        Ok(())
    }

    fn describe(&self, cx: &App) -> Value {
        let view = self.view.read(cx);
        let probes: Vec<_> = view.control_probes().iter().map(|probe| json!({
            "id": probe.id, "x": f32::from(probe.bounds.origin.x), "y": f32::from(probe.bounds.origin.y),
            "width": f32::from(probe.bounds.size.width), "height": f32::from(probe.bounds.size.height),
            "focused": probe.focused, "selected": probe.selected, "disabled": probe.disabled,
        })).collect();
        json!({ "language": self.snapshot.language.slug(), "theme": self.snapshot.theme_slug.as_ref(),
            "pane": view.pane().id(), "requests": self.requests.len(), "events": self.requests,
            "visible": self.visible, "unsupported": self.unsupported, "pendingSaves": self.pending_saves.len(),
            "pendingReset": self.pending_reset.is_some(), "pendingConsent": self.pending_consent,
            "smartEnabled": self.snapshot.smart_enabled, "smartProvider": self.snapshot.smart_provider.slug(),
            "smartModel": self.snapshot.smart_model.as_deref(), "graphCompact": self.snapshot.graph_compact,
            "graphLaneCompact": self.snapshot.graph_lane_compact, "terminalAutoLock": self.snapshot.terminal_auto_lock,
            "autoFetch": self.snapshot.auto_fetch, "reduceMotion": self.snapshot.reduce_motion, "zoom": self.snapshot.zoom,
            "toolbar": self.snapshot.toolbar_display.as_str(), "followsSystemAccent": self.snapshot.follows_system_accent,
            "compactThreshold": self.snapshot.compact_threshold_rem,
            "copyTarget": match self.snapshot.graph_copy_target { CopyTarget::Hash => "hash", CopyTarget::Branch => "branch" },
            "markdown": self.snapshot.markdown_default_view.as_str(),
            "draft": self.input.read(cx).value().as_ref(), "baseline": self.snapshot.analyze_saved.as_ref(),
            "dirty": view.analyze_dirty(cx), "controls": probes })
    }
}

impl gpui::Render for SettingsFixture {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(theme::scaled_px(16.));
        div().id("production-settings-fixture").track_focus(&self.focus).size_full()
            .bg(gpui::rgb(theme::theme().panel)).text_color(gpui::rgb(theme::theme().text_main)).font(theme::ui_font())
            .when(self.visible, |el| el.child(self.view.clone()))
    }
}
