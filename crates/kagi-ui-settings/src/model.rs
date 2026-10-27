//! Host-owned effective values and typed requests. No persistence or discovery runs here.
use gpui::{AnyWindowHandle, SharedString};
use kagi_ui_core::{i18n::Lang, settings::{CopyTarget, MarkdownDefaultView, ToolbarDisplay}, theme::ThemeHandle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane { Appearance, Graph, Editor, Terminal, Network, SmartCommit, Analyze, General }
impl Pane {
    pub const ALL: [Self; 8] = [Self::Appearance, Self::Graph, Self::Editor, Self::Terminal, Self::Network, Self::SmartCommit, Self::Analyze, Self::General];
    pub fn id(self) -> &'static str { match self { Self::Appearance => "appearance", Self::Graph => "graph", Self::Editor => "editor", Self::Terminal => "terminal", Self::Network => "network", Self::SmartCommit => "smart_commit", Self::Analyze => "analyze", Self::General => "general" } }
    pub fn from_id(id: &str) -> Option<Self> { Self::ALL.into_iter().find(|pane| pane.id() == id) }
    pub fn from_legacy(index: usize) -> Self { match index { 1 => Self::General, 2 => Self::SmartCommit, 3 => Self::Analyze, _ => Self::Appearance } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmartProvider { Ollama, ClaudeCode, Codex }
impl SmartProvider {
    pub const ALL: [Self; 3] = [Self::Ollama, Self::ClaudeCode, Self::Codex];
    pub fn slug(self) -> &'static str { match self { Self::Ollama => "ollama", Self::ClaudeCode => "claude-code", Self::Codex => "codex" } }
    pub fn display_name(self) -> &'static str { match self { Self::Ollama => "Ollama", Self::ClaudeCode => "Claude Code", Self::Codex => "Codex" } }
}
#[derive(Clone, Debug)]
pub struct ProviderAvailability { pub provider: SmartProvider, pub available: bool, pub reason: Option<SharedString> }
#[derive(Clone, Debug)]
pub struct SettingsSnapshot {
    pub language: Lang,
    pub themes: Vec<ThemeHandle>,
    pub theme_slug: SharedString,
    pub accent_applicable: bool,
    pub follows_system_accent: bool,
    pub effective_accent: u32,
    pub accent_description: SharedString,
    pub theme_folder: Option<SharedString>,
    pub can_open_theme_folder: bool,
    pub toolbar_display: ToolbarDisplay,
    pub zoom: f32,
    pub compact_threshold_rem: u32,
    pub reduce_motion: bool,
    pub graph_compact: bool,
    pub graph_lane_compact: bool,
    pub graph_copy_target: CopyTarget,
    pub markdown_default_view: MarkdownDefaultView,
    pub terminal_auto_lock: bool,
    pub auto_fetch: bool,
    pub smart_enabled: bool,
    pub smart_consent_accepted: bool,
    pub smart_ollama_available: bool,
    pub smart_detecting: bool,
    pub smart_provider: SmartProvider,
    pub providers: Vec<ProviderAvailability>,
    pub smart_model: Option<SharedString>,
    pub models: Vec<SharedString>,
    pub analyze_saved: SharedString,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalyzeRequest { pub text: SharedString, pub revision: u64 }
#[derive(Clone, Debug)]
pub enum SettingsIntent {
    Pane(Pane), Language(Lang), Theme(SharedString), SystemAccent(bool), Toolbar(ToolbarDisplay),
    Zoom(f32), CompactThreshold(u32), ReduceMotion(bool), GraphCompact(bool), GraphLaneCompact(bool),
    GraphCopyTarget(CopyTarget), MarkdownDefaultView(MarkdownDefaultView), TerminalAutoLock(bool), AutoFetch(bool),
    SmartEnabled(bool), SmartConsentRequested, SmartProvider(SmartProvider), SmartModel(SharedString),
    OpenThemeFolder, ReloadThemes, OpenThemeGuide, SaveAnalyze(AnalyzeRequest), ResetAnalyze(AnalyzeRequest),
}
#[derive(Clone, Debug)]
pub struct SettingsEvent { pub window: AnyWindowHandle, pub intent: SettingsIntent }

/// Acknowledgements carry the captured request, never the current draft.
#[derive(Clone, Debug)]
pub enum AnalyzeResult {
    Saved(AnalyzeRequest),
    Reset { request: AnalyzeRequest, text: SharedString },
    Failed { request: AnalyzeRequest, error: SharedString },
}

impl SettingsSnapshot {
    pub(crate) fn same_effective(&self, other: &Self) -> bool {
        self.language == other.language
            && self.themes.iter().map(ThemeHandle::key).eq(other.themes.iter().map(ThemeHandle::key))
            && self.theme_slug == other.theme_slug
            && self.accent_applicable == other.accent_applicable
            && self.follows_system_accent == other.follows_system_accent
            && self.effective_accent == other.effective_accent
            && self.accent_description == other.accent_description
            && self.theme_folder == other.theme_folder
            && self.can_open_theme_folder == other.can_open_theme_folder
            && self.toolbar_display == other.toolbar_display
            && self.zoom == other.zoom
            && self.compact_threshold_rem == other.compact_threshold_rem
            && self.reduce_motion == other.reduce_motion
            && self.graph_compact == other.graph_compact
            && self.graph_lane_compact == other.graph_lane_compact
            && self.graph_copy_target == other.graph_copy_target
            && self.markdown_default_view == other.markdown_default_view
            && self.terminal_auto_lock == other.terminal_auto_lock
            && self.auto_fetch == other.auto_fetch
            && self.smart_enabled == other.smart_enabled
            && self.smart_consent_accepted == other.smart_consent_accepted
            && self.smart_ollama_available == other.smart_ollama_available
            && self.smart_detecting == other.smart_detecting
            && self.smart_provider == other.smart_provider
            && self.providers.iter().map(|p| (p.provider, p.available, &p.reason))
                .eq(other.providers.iter().map(|p| (p.provider, p.available, &p.reason)))
            && self.smart_model == other.smart_model
            && self.models == other.models
            && self.analyze_saved == other.analyze_saved
    }
}
