use super::{
    controls::{pt, section},
    *,
};
use gpui_component::input::Input;
use kagi_ui_core::{
    i18n::{self, Lang},
    settings::{CopyTarget, MarkdownDefaultView, ToolbarDisplay},
    theme::{self, theme},
};

impl SettingsView {
    pub(crate) fn render_pane(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let content = match self.pane {
            Pane::Appearance => self.appearance(window, cx),
            Pane::Graph => self.graph(window, cx),
            Pane::Editor => {
                let selected = self.snapshot.markdown_default_view;
                let control = self.choices(
                    "settings-markdown-default-view",
                    [
                        (MarkdownDefaultView::Preview, Msg::MarkdownDefaultPreview),
                        (MarkdownDefaultView::Source, Msg::MarkdownDefaultSource),
                    ]
                    .into_iter()
                    .map(|(value, label)| {
                        (
                            format!("settings-markdown-{}", value.as_str()),
                            label.t().into(),
                            selected == value,
                            true,
                            SettingsIntent::MarkdownDefaultView(value),
                        )
                    })
                    .collect(),
                    window,
                    cx,
                );
                section(
                    Msg::SettingsMarkdownSection,
                    self.group("settings-editor-group").child(self.row(
                        Msg::SettingsMarkdownDefaultView,
                        Msg::SettingsMarkdownDefaultViewDesc.t(),
                        control,
                    )),
                )
            }
            Pane::Terminal => {
                let control = self.toggle(
                    "terminal-auto-lock-toggle",
                    Msg::SettingsTerminalAutoLock,
                    self.snapshot.terminal_auto_lock,
                    SettingsIntent::TerminalAutoLock(!self.snapshot.terminal_auto_lock),
                    window,
                    cx,
                );
                section(
                    Msg::SettingsWorktreesSection,
                    self.group("settings-terminal-group").child(self.row(
                        Msg::SettingsTerminalAutoLock,
                        Msg::SettingsTerminalAutoLockDesc.t(),
                        control,
                    )),
                )
            }
            Pane::Network => {
                let control = self.toggle(
                    "auto-fetch-toggle",
                    Msg::SettingsAutoFetch,
                    self.snapshot.auto_fetch,
                    SettingsIntent::AutoFetch(!self.snapshot.auto_fetch),
                    window,
                    cx,
                );
                section(
                    Msg::SettingsFetchSection,
                    self.group("settings-network-group").child(self.row(
                        Msg::SettingsAutoFetch,
                        Msg::SettingsAutoFetchDesc.t(),
                        control,
                    )),
                )
            }
            Pane::SmartCommit => self.smart(window, cx),
            Pane::Analyze => self.analyze(window, cx),
            Pane::General => {
                let selected = self.snapshot.language;
                let control = self.choices(
                    "settings-language",
                    [(Lang::En, "en"), (Lang::Ja, "ja")]
                        .into_iter()
                        .map(|(value, id)| {
                            (
                                format!("settings-language-{id}"),
                                value.display_name().into(),
                                selected == value,
                                true,
                                SettingsIntent::Language(value),
                            )
                        })
                        .collect(),
                    window,
                    cx,
                );
                section(
                    Msg::SettingsLanguage,
                    self.group("settings-language-group").child(self.row(
                        Msg::SettingsInterfaceLang,
                        Msg::SettingsInterfaceLangDesc.t(),
                        control,
                    )),
                )
            }
        };
        div()
            .id("settings-detail-content")
            .relative()
            .min_w_0()
            .w_full()
            .child(content)
            .into_any_element()
    }
    fn appearance(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let themes = self.theme_cards(window, cx);
        let mut interface = self.group("settings-interface-group");
        if self.snapshot.accent_applicable {
            let selected = self.snapshot.follows_system_accent;
            let choices = self.choices(
                "settings-accent",
                [
                    (true, Msg::SettingsAccentSystem, "system"),
                    (false, Msg::SettingsAccentTheme, "theme"),
                ]
                .into_iter()
                .map(|(value, label, id)| {
                    (
                        format!("settings-accent-{id}"),
                        label.t().into(),
                        selected == value,
                        true,
                        SettingsIntent::SystemAccent(value),
                    )
                })
                .collect(),
                window,
                cx,
            );
            interface = interface.child(self.row(
                Msg::SettingsAccent,
                self.snapshot.accent_description.clone(),
                choices,
            ));
        }
        let selected = self.snapshot.toolbar_display;
        let toolbar = self.choices(
            "settings-toolbar-display",
            [
                (ToolbarDisplay::IconAndText, Msg::ToolbarIconAndText),
                (ToolbarDisplay::IconOnly, Msg::ToolbarIconOnly),
            ]
            .into_iter()
            .map(|(value, label)| {
                (
                    format!("settings-toolbar-{}", value.as_str()),
                    label.t().into(),
                    selected == value,
                    true,
                    SettingsIntent::Toolbar(value),
                )
            })
            .collect(),
            window,
            cx,
        );
        let zoom = self.zoom_control(window, cx);
        let threshold = self.threshold_control(window, cx);
        let motion = self.toggle(
            "reduce-motion-toggle",
            Msg::SettingsReduceMotion,
            self.snapshot.reduce_motion,
            SettingsIntent::ReduceMotion(!self.snapshot.reduce_motion),
            window,
            cx,
        );
        interface = interface
            .child(self.row(
                Msg::SettingsToolbarDisplay,
                Msg::SettingsToolbarDisplayDesc.t(),
                toolbar,
            ))
            .child(self.row(Msg::SettingsZoom, Msg::SettingsZoomDesc.t(), zoom))
            .child(self.row(
                Msg::SettingsCompactThreshold,
                Msg::SettingsCompactThresholdDesc.t(),
                threshold,
            ))
            .child(self.row(
                Msg::SettingsReduceMotion,
                Msg::SettingsReduceMotionDesc.t(),
                motion,
            ));
        let folder = self
            .snapshot
            .theme_folder
            .clone()
            .unwrap_or_else(|| Msg::SettingsThemesFolderUnavailable.t().into());
        let open = self.button(
            "settings-theme-folder-open",
            Msg::SettingsCustomThemesOpen.t().into(),
            self.snapshot.can_open_theme_folder,
            theme().bg_base,
            SettingsIntent::OpenThemeFolder,
            window,
            cx,
        );
        let reload = self.button(
            "settings-theme-reload",
            Msg::SettingsCustomThemesReload.t().into(),
            true,
            theme().panel,
            SettingsIntent::ReloadThemes,
            window,
            cx,
        );
        let guide = self.button(
            "settings-theme-guide",
            Msg::SettingsCustomThemesGuide.t().into(),
            true,
            theme().panel,
            SettingsIntent::OpenThemeGuide,
            window,
            cx,
        );
        div()
            .flex()
            .flex_col()
            .gap(pt(20.))
            .child(section(Msg::SettingsColorTheme, themes))
            .child(section(Msg::SettingsInterface, interface))
            .child(section(
                Msg::SettingsCustomThemes,
                div()
                    .flex()
                    .flex_col()
                    .gap(pt(8.))
                    .child(self.group("settings-theme-folder-group").child(self.row(
                        Msg::SettingsCustomThemes,
                        folder,
                        open,
                    )))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(pt(8.))
                            .text_color(rgb(theme().link))
                            .child(reload)
                            .child(guide),
                    )
                    .child(
                        div()
                            .id("settings-custom-themes-help")
                            .relative()
                            .text_size(pt(11.5))
                            .text_color(rgb(theme().text_sub))
                            .child(Msg::SettingsCustomThemesDesc.t())
                            .children(cfg!(feature = "gui-e2e").then(|| {
                                self.measured(
                                    "settings-custom-themes-help".into(),
                                    Role::Label,
                                    Msg::SettingsCustomThemes.t().into(),
                                    Msg::SettingsCustomThemesDesc.t().into(),
                                    false,
                                    false,
                                    false,
                                )
                            })),
                    ),
            ))
            .into_any_element()
    }
    fn zoom_control(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let zoom = self.snapshot.zoom;
        let value = format!("{:.0}%", zoom * 100.);
        let dec = theme::step_zoom(zoom, false);
        let inc = theme::step_zoom(zoom, true);
        let decrease = self.stepper_button(
            "zoom-dec",
            i18n::settings_zoom_action(false, &value).into(),
            dec != zoom,
            SettingsIntent::Zoom(dec),
            window,
            cx,
        );
        let increase = self.stepper_button(
            "zoom-inc",
            i18n::settings_zoom_action(true, &value).into(),
            inc != zoom,
            SettingsIntent::Zoom(inc),
            window,
            cx,
        );
        div()
            .flex()
            .flex_nowrap()
            .items_center()
            .gap(pt(6.))
            .child(decrease)
            .child(
                div()
                    .id("zoom-value")
                    .relative()
                    .role(Role::Status)
                    .aria_label(i18n::settings_zoom_value(&value))
                    .children(cfg!(feature = "gui-e2e").then(|| {
                        self.measured(
                            "zoom-value".into(),
                            Role::Status,
                            i18n::settings_zoom_value(&value),
                            value.clone(),
                            false,
                            false,
                            false,
                        )
                    }))
                    .child(value),
            )
            .child(increase)
            .into_any_element()
    }
    fn threshold_control(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let value = self.snapshot.compact_threshold_rem;
        let description =
            i18n::settings_compact_threshold_value(value, value as f32 * theme::rem_size_px());
        let decrease = self.stepper_button(
            "compact-threshold-dec",
            i18n::settings_compact_threshold_action(false, value).into(),
            value > 40,
            SettingsIntent::CompactThreshold(value.saturating_sub(1).max(40)),
            window,
            cx,
        );
        let increase = self.stepper_button(
            "compact-threshold-inc",
            i18n::settings_compact_threshold_action(true, value).into(),
            value < 160,
            SettingsIntent::CompactThreshold(value.saturating_add(1).min(160)),
            window,
            cx,
        );
        div()
            .flex()
            .flex_col()
            .gap(pt(4.))
            .child(
                div()
                    .flex()
                    .flex_nowrap()
                    .items_center()
                    .gap(pt(6.))
                    .child(decrease)
                    .child(
                        div()
                            .id("compact-threshold-value")
                            .relative()
                            .role(Role::Status)
                            .aria_label(description.clone())
                            .child(format!("{value}rem"))
                            .children(cfg!(feature = "gui-e2e").then(|| {
                                self.measured(
                                    "compact-threshold-value".into(),
                                    Role::Status,
                                    description.clone(),
                                    format!("{value}rem"),
                                    false,
                                    false,
                                    false,
                                )
                            })),
                    )
                    .child(increase),
            )
            .child(
                div()
                    .relative()
                    .text_size(pt(11.5))
                    .text_color(rgb(theme().text_sub))
                    .children(cfg!(feature = "gui-e2e").then(|| {
                        self.measured(
                            "compact-threshold-help".into(),
                            Role::Label,
                            description.clone(),
                            description.clone(),
                            false,
                            false,
                            false,
                        )
                    }))
                    .child(description),
            )
            .into_any_element()
    }
    fn graph(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let compact = self.toggle(
            "compact-toggle",
            Msg::SettingsCompact,
            self.snapshot.graph_compact,
            SettingsIntent::GraphCompact(!self.snapshot.graph_compact),
            window,
            cx,
        );
        let lanes = self.toggle(
            "lane-compact-toggle",
            Msg::SettingsLaneCompact,
            self.snapshot.graph_lane_compact,
            SettingsIntent::GraphLaneCompact(!self.snapshot.graph_lane_compact),
            window,
            cx,
        );
        let selected = self.snapshot.graph_copy_target;
        let copy = self.choices(
            "settings-graph-copy",
            [
                (CopyTarget::Hash, "Hash", "hash"),
                (CopyTarget::Branch, "Branch", "branch"),
            ]
            .into_iter()
            .map(|(value, label, id)| {
                (
                    format!("settings-copy-{id}"),
                    label.into(),
                    selected == value,
                    true,
                    SettingsIntent::GraphCopyTarget(value),
                )
            })
            .collect(),
            window,
            cx,
        );
        div()
            .flex()
            .flex_col()
            .gap(pt(20.))
            .child(section(
                Msg::SettingsPresentation,
                self.group("settings-graph-group")
                    .child(self.row(Msg::SettingsCompact, Msg::SettingsCompactDesc.t(), compact))
                    .child(self.row(
                        Msg::SettingsLaneCompact,
                        Msg::SettingsLaneCompactDesc.t(),
                        lanes,
                    )),
            ))
            .child(section(
                Msg::SettingsCopySection,
                self.group("settings-copy-group").child(self.row(
                    Msg::SettingsGraphCopy,
                    Msg::SettingsGraphCopyDesc.t(),
                    copy,
                )),
            ))
            .into_any_element()
    }
    fn smart(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let intent = if !self.snapshot.smart_enabled && !self.snapshot.smart_consent_accepted {
            SettingsIntent::SmartConsentRequested
        } else {
            SettingsIntent::SmartEnabled(!self.snapshot.smart_enabled)
        };
        let enabled = self.toggle(
            "sc-enabled-toggle",
            Msg::SettingsSmartEnable,
            self.snapshot.smart_enabled,
            intent,
            window,
            cx,
        );
        let entries = SmartProvider::ALL
            .into_iter()
            .map(|provider| {
                let availability = self
                    .snapshot
                    .providers
                    .iter()
                    .find(|entry| entry.provider == provider);
                let available = availability.is_some_and(|entry| entry.available);
                let label = provider.display_name().into();
                (
                    format!("sc-provider-{}", provider.slug()),
                    label,
                    provider == self.snapshot.smart_provider,
                    available,
                    SettingsIntent::SmartProvider(provider),
                )
            })
            .collect();
        let providers = self.choices("settings-provider-choices", entries, window, cx);
        let mut provider_help = Msg::SettingsSmartProviderDesc.t().to_owned();
        for availability in &self.snapshot.providers {
            if !availability.available {
                if let Some(reason) = &availability.reason {
                    provider_help.push('\n');
                    provider_help.push_str(reason);
                }
            }
        }
        let mut rows = self
            .group("settings-smart-group")
            .child(self.row(
                Msg::SettingsSmartEnable,
                Msg::SettingsSmartEnableDesc.t(),
                enabled,
            ))
            .child(self.row(Msg::SettingsSmartProvider, provider_help, providers));
        if self.snapshot.smart_provider == SmartProvider::Ollama {
            let models_available =
                self.snapshot.smart_ollama_available && !self.snapshot.smart_detecting;
            let mut entries: Vec<_> = self
                .snapshot
                .models
                .iter()
                .map(|model| {
                    (
                        format!("sc-model-{model}"),
                        model.clone(),
                        self.snapshot.smart_model.as_ref() == Some(model),
                        models_available,
                        SettingsIntent::SmartModel(model.clone()),
                    )
                })
                .collect();
            if let Some(saved) = &self.snapshot.smart_model {
                if !self.snapshot.models.contains(saved) {
                    entries.push((
                        format!("sc-model-{saved}"),
                        i18n::smart_model_switch_note(saved).into(),
                        true,
                        false,
                        SettingsIntent::SmartModel(saved.clone()),
                    ));
                }
            }
            let models = if entries.is_empty() {
                div()
                    .child(Msg::SettingsSmartNoModels.t())
                    .into_any_element()
            } else {
                self.choices("settings-model-choices", entries, window, cx)
            };
            rows = rows.child(self.row(
                Msg::SettingsSmartModel,
                Msg::SettingsSmartModelDesc.t(),
                models,
            ));
        }
        let mut result = div()
            .flex()
            .flex_col()
            .gap(pt(16.))
            .child(section(Msg::SettingsGeneration, rows));
        if self.snapshot.smart_detecting {
            result = result.child(
                div()
                    .id("settings-smart-detecting")
                    .role(Role::Status)
                    .child(Msg::SmartDetecting.t()),
            );
        }
        if self.snapshot.smart_provider != SmartProvider::Ollama {
            let provider = self.snapshot.smart_provider;
            let bin = if provider == SmartProvider::ClaudeCode {
                "claude"
            } else {
                "codex"
            };
            result = result.child(
                div()
                    .id("settings-cli-warning")
                    .flex()
                    .flex_col()
                    .gap(pt(6.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(i18n::smart_cli_warning_title(provider.display_name())),
                    )
                    .children(
                        i18n::smart_cli_warning_lines(provider.display_name(), bin)
                            .into_iter()
                            .map(|line| div().child(line)),
                    ),
            );
        }
        result.into_any_element()
    }
    fn analyze(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let dirty = self.analyze_dirty(cx);
        let request = self.analyze_request(cx);
        let reset = self.button(
            "settings-analyze-reset",
            Msg::SettingsAnalyzeIgnoreReset.t().into(),
            true,
            theme().panel,
            SettingsIntent::ResetAnalyze(request.clone()),
            window,
            cx,
        );
        let save = self.button(
            "settings-analyze-save",
            Msg::SettingsAnalyzeIgnoreSave.t().into(),
            dirty,
            theme().panel,
            SettingsIntent::SaveAnalyze(request),
            window,
            cx,
        );
        let status = if dirty {
            Msg::SettingsAnalyzeIgnoreUnsaved
        } else if self.analyze.saved_feedback {
            Msg::SettingsAnalyzeIgnoreSaved
        } else {
            Msg::SettingsAnalyzeIgnoreClean
        };
        section(
            Msg::SettingsExclusions,
            div()
                .id("settings-analyze-section")
                .relative()
                .flex()
                .flex_col()
                .gap(pt(12.))
                .child(Msg::SettingsAnalyzeIgnore.t())
                .children(cfg!(feature = "gui-e2e").then(|| {
                    self.measured(
                        "settings-analyze-section".into(),
                        Role::Group,
                        Msg::SettingsExclusions.t().into(),
                        String::new(),
                        false,
                        false,
                        false,
                    )
                }))
                .child(
                    div()
                        .id("settings-analyze-help")
                        .relative()
                        .text_size(pt(11.5))
                        .text_color(rgb(theme().text_sub))
                        .child(Msg::SettingsAnalyzeIgnoreDesc.t())
                        .children(cfg!(feature = "gui-e2e").then(|| {
                            self.measured(
                                "settings-analyze-help".into(),
                                Role::Label,
                                Msg::SettingsAnalyzeIgnore.t().into(),
                                Msg::SettingsAnalyzeIgnoreDesc.t().into(),
                                false,
                                false,
                                false,
                            )
                        })),
                )
                .child(
                    div()
                        .id("settings-analyze-input")
                        .relative()
                        .min_h(pt(220.))
                        .child(
                            Input::new(&self.input)
                                .context_menu(kagi_ui_core::input::context_menu(&self.input, false))
                                .aria_label(Msg::A11yAnalyzeIgnorePatterns.t())
                                .appearance(true)
                                .bordered(true),
                        )
                        .children(cfg!(feature = "gui-e2e").then(|| {
                            self.measured(
                                "settings-analyze-input".into(),
                                Role::TextInput,
                                Msg::A11yAnalyzeIgnorePatterns.t().into(),
                                self.input.read(cx).value().to_string(),
                                false,
                                false,
                                false,
                            )
                        })),
                )
                .child(
                    div()
                        .id("settings-analyze-status")
                        .relative()
                        .role(Role::Status)
                        .child(status.t())
                        .children(cfg!(feature = "gui-e2e").then(|| {
                            self.measured(
                                "settings-analyze-status".into(),
                                Role::Status,
                                status.t().into(),
                                status.t().into(),
                                false,
                                false,
                                false,
                            )
                        })),
                )
                .when_some(self.analyze.error.clone(), |el, error| {
                    el.child(
                        div()
                            .id("settings-analyze-error")
                            .relative()
                            .text_color(rgb(theme().color_blocker))
                            .children(cfg!(feature = "gui-e2e").then(|| {
                                self.measured(
                                    "settings-analyze-error".into(),
                                    Role::Status,
                                    error.to_string(),
                                    error.to_string(),
                                    false,
                                    false,
                                    false,
                                )
                            }))
                            .child(error),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(pt(8.))
                        .child(reset)
                        .child(save),
                ),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::Pane;
    #[test]
    fn stable_pane_ids_and_legacy_migration() {
        for pane in Pane::ALL {
            assert_eq!(Pane::from_id(pane.id()), Some(pane));
        }
        assert_eq!(Pane::from_legacy(1), Pane::General);
        assert_eq!(Pane::from_legacy(2), Pane::SmartCommit);
        assert_eq!(Pane::from_legacy(3), Pane::Analyze);
        assert_eq!(Pane::from_id("unknown"), None);
    }
}
