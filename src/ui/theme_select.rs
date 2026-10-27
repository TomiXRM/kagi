//! Settings' appearance theme picker and the custom-theme reload (#922). The
//! gpui-component `Select` is an entity that needs a `Window`, so it cannot be
//! built in `KagiApp::new`; it is built with the rest of the window-bound
//! entity state in `e2e::build_kagi_entity`, which the real window and the
//! offscreen GUI E2E mount share (ADR-0166).
//!
//! Theme files are read only at startup (`theme::init_active`) and by the
//! explicit "Reload Themes" command — never from render or a watcher.

use gpui::{App, AppContext as _, Context, Entity, SharedString, Window};
use gpui_component::select::SelectEvent;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use super::external_editor::os_opener;
use super::i18n;
use super::settings_view::{self, ThemeOption, ThemeSelectState};
use super::theme::{self, ThemeLoadError};

/// Renderer-fed semantics, not a native VoiceOver snapshot.
#[cfg(feature = "gui-e2e")]
#[derive(Clone, Debug)]
pub struct AppearanceAx {
    pub role: gpui::Role,
    pub name: String,
    pub value: String,
    pub expanded: Option<bool>,
    pub disabled: bool,
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static APPEARANCE_AX: std::cell::RefCell<std::collections::HashMap<String, AppearanceAx>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn record_ax(
    id: &str,
    role: gpui::Role,
    name: &str,
    value: &str,
    expanded: Option<bool>,
    disabled: bool,
) {
    APPEARANCE_AX.with(|ax| {
        ax.borrow_mut().insert(
            id.into(),
            AppearanceAx {
                role,
                name: name.into(),
                value: value.into(),
                expanded,
                disabled,
            },
        )
    });
}

#[cfg(feature = "gui-e2e")]
pub fn appearance_ax_for_e2e(id: &str) -> Option<AppearanceAx> {
    APPEARANCE_AX.with(|ax| ax.borrow().get(id).cloned())
}

#[cfg(feature = "gui-e2e")]
pub fn take_theme_option_ax_for_e2e() -> Vec<AppearanceAx> {
    APPEARANCE_AX.with(|ax| {
        let mut ax = ax.borrow_mut();
        let mut options = Vec::new();
        ax.retain(|id, value| {
            if id.starts_with("theme-option:") {
                options.push(value.clone());
                false
            } else {
                true
            }
        });
        options
    })
}
use super::{commands, KagiApp, ToastKind};

/// A toast is a short preview; the log line keeps the whole reason.
const TOAST_DETAIL_CHARS: usize = 160;

/// Theme registry and menus are process-global, so requests from different
/// windows must share one generation rather than each window keeping its own.
static THEME_RELOAD_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Build the picker and apply + persist a confirmed theme via `set_theme`,
/// then report the theme files that failed to load at startup — the window
/// exists now, so their toasts can be seen.
pub(super) fn install(kagi: &Entity<KagiApp>, window: &mut Window, cx: &mut App) {
    let theme_select = cx.new(|cx| {
        ThemeSelectState::new(
            settings_view::theme_options(),
            Some(settings_view::current_theme_index()),
            window,
            cx,
        )
    });
    kagi.update(cx, |_, cx| {
        cx.observe(&theme_select, |_, _, cx| cx.notify()).detach();
    });
    kagi.update(cx, |app, cx| {
        cx.subscribe(
            &theme_select,
            |this, _state, event: &SelectEvent<Vec<ThemeOption>>, cx| {
                if let SelectEvent::Confirm(Some(slug)) = event {
                    this.set_theme(slug, cx);
                    cx.notify();
                }
            },
        )
        .detach();
        app.theme_select = Some(theme_select);
        // Drained once per process: a second window does not repeat them.
        let subject = match app.toast_subject_for_window(window, cx) {
            crate::ui::toast_stack::ToastSubject::Settings => {
                crate::ui::toast_stack::ToastSubject::Settings
            }
            _ => crate::ui::toast_stack::ToastSubject::App,
        };
        app.report_theme_load_errors(theme::take_theme_load_errors(), subject, cx);
    });
}

impl KagiApp {
    /// Create the user theme directory if missing and reveal it in the OS file
    /// manager. The filesystem call and process launch stay off GPUI's UI thread.
    pub(crate) fn open_themes_folder(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = theme::themes_dir() else {
            self.push_toast_for(
                crate::ui::toast_stack::ToastSubject::Settings,
                ToastKind::Error,
                i18n::Msg::SettingsThemesFolderUnavailable.t(),
                cx,
            );
            return;
        };
        let task = cx.background_spawn(async move {
            std::fs::create_dir_all(&dir)?;
            Command::new(os_opener()).arg(&dir).spawn().map(|_| ())
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |app, cx| {
                if let Err(error) = result {
                    eprintln!("theme folder: {error}");
                    app.push_toast_for(
                        crate::ui::toast_stack::ToastSubject::Settings,
                        ToastKind::Error,
                        i18n::themes_folder_failed_fmt(&bounded(&error.to_string())),
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    /// Re-read the custom theme files (#922) off-thread; publish only the last
    /// requested generation on the foreground, including the Settings picker,
    /// native / Linux menus, gpui-component colours and running terminals.
    pub(crate) fn reload_themes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let subject = self.toast_subject_for_window(window, cx);
        let generation = THEME_RELOAD_GENERATION
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        #[cfg(feature = "gui-e2e")]
        let hold = THEME_RELOAD_HOLD.with(|slot| slot.borrow_mut().take());
        let task = cx.background_spawn(async move {
            let loaded = theme::read_custom_themes();
            #[cfg(feature = "gui-e2e")]
            if let Some(hold) = hold {
                hold.await;
            }
            loaded
        });
        cx.spawn_in(window, async move |this, cx| {
            let loaded = task.await;
            let _ = this.update_in(cx, |app, window, cx| {
                if THEME_RELOAD_GENERATION.load(Ordering::Relaxed) != generation {
                    return;
                }
                // A folder that cannot be listed is not an empty one: the
                // themes already loaded stay, and the reason is reported.
                let (themes, errors) = match loaded {
                    Ok(loaded) => loaded,
                    Err(error) => {
                        let kept = theme::all_themes().iter().filter(|t| t.is_custom()).count();
                        klog!("theme: reload kept custom={kept}");
                        app.report_theme_load_errors(vec![error], subject, cx);
                        cx.notify();
                        return;
                    }
                };
                theme::install_custom_themes(themes);
                let custom = theme::all_themes().iter().filter(|t| t.is_custom()).count();
                let active = SharedString::from(theme::theme().slug.clone());
                klog!(
                    "theme: reload custom={} errors={} active={}",
                    custom,
                    errors.len(),
                    active
                );
                theme::sync_gpui_component_theme(cx);
                app.apply_terminal_config(cx);
                commands::publish_menus(cx);
                if let Some(select) = app.theme_select.clone() {
                    select.update(cx, |state, cx| {
                        state.set_items(settings_view::theme_options(), window, cx);
                        state.set_selected_value(&active, window, cx);
                    });
                }
                app.push_toast_for(
                    subject,
                    ToastKind::Success,
                    i18n::themes_reloaded_fmt(custom),
                    cx,
                );
                app.report_theme_load_errors(errors, subject, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Log every rejected theme file and show one bounded toast per file.
    fn report_theme_load_errors(
        &mut self,
        errors: Vec<ThemeLoadError>,
        subject: crate::ui::toast_stack::ToastSubject,
        cx: &mut Context<Self>,
    ) {
        for error in errors {
            let detail = error.to_string();
            klog!("theme: load error {}", detail);
            let display = i18n::op::user_text::theme_load_error(&error);
            let message = i18n::theme_load_failed_fmt(&bounded(&display));
            self.push_toast_for(subject, ToastKind::Error, message, cx);
        }
    }
}
#[cfg(feature = "gui-e2e")]
thread_local! {
    static THEME_RELOAD_HOLD: std::cell::RefCell<Option<gpui::Task<()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    /// Hold the next off-thread read after it has parsed the files, so a newer
    /// reload can publish first. Native Tier A only.
    pub fn hold_next_theme_reload_for_e2e(hold: gpui::Task<()>) {
        THEME_RELOAD_HOLD.with(|slot| assert!(slot.borrow_mut().replace(hold).is_none()));
    }
}

/// `detail` cut to [`TOAST_DETAIL_CHARS`] characters, with an ellipsis when cut.
fn bounded(detail: &str) -> String {
    match detail.char_indices().nth(TOAST_DETAIL_CHARS) {
        Some((end, _)) => format!("{}\u{2026}", &detail[..end]),
        None => detail.to_string(),
    }
}
