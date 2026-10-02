//! Home's GitHub section (#923 PR3, ADR-0219 decisions 2, 3 and 7): the
//! signed-in user's repositories from `gh repo list`, each one opened when a
//! local clone is known and cloned through the recorded clone write
//! otherwise.
//!
//! The list is read once per window (Refresh reads it again) on a background
//! thread, together with the `origin` of every recently opened repository so
//! a listed repository can be matched with its local clone.
//!
//! A clone goes plan → card (confirm) → `execute_clone` (preflight, `gh repo
//! clone`, verify, oplog) on a background thread, one at a time per window.
//! On success the new folder opens as a tab — Home itself becomes that tab
//! when it is in front.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{
    div, prelude::*, px, rgb, AnyElement, Context, Entity, FocusHandle, KeyDownEvent,
    PathPromptOptions, SharedString, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Icon, IconName, Sizable as _};
use kagi_git::github_repos::{OwnerRepos, RepoListing};
use kagi_git::ops::{plan_clone, CloneRequest};
use kagi_git::OperationPlan;

use super::i18n::Msg;
use super::modal_renderers::{modal_overlay, render_current_predicted, render_modal_title_row};
use super::modal_shell::{modal_card, modal_scroll_body, MODAL_W_LG};
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;
use kagi_ui_core::i18n::{plan_note_text, plan_title_text};

/// Settings key: the folder the last clone went into, the next default.
const CLONE_PARENT_KEY: &str = "clone_parent_dir";

/// The GitHub list as Home knows it.
#[derive(Clone, Debug, Default)]
pub enum GithubRepos {
    #[default]
    NotLoaded,
    Loading,
    Loaded {
        /// The user's own section first, then one per organization.
        sections: Vec<OwnerRepos>,
        /// Listed identity (`host/owner/repo`) → a local clone of it.
        local: HashMap<String, PathBuf>,
    },
    Failed(String),
}

/// Home's GitHub state, window-wide like Home itself.
#[derive(Default)]
pub struct HomeGithub {
    pub repos: GithubRepos,
    pub filter: Option<Entity<InputState>>,
    /// The clone running now (one per window, ADR-0219 decision 7).
    pub cloning: Option<CloneRequest>,
    /// Bumped per read, so an older read landing late is dropped.
    generation: u64,
}

/// The clone card: what will be cloned where, and the plan the user confirms.
#[derive(Clone, Debug)]
pub struct CloneModal {
    pub listing: RepoListing,
    pub request: CloneRequest,
    pub plan: OperationPlan,
}

impl CloneModal {
    fn new(listing: RepoListing, parent: &Path) -> Self {
        let request = CloneRequest {
            source: listing.clone_source(),
            dest: parent.join(listing.name()),
            is_fork: listing.is_fork,
        };
        let plan = plan_clone(&request);
        Self {
            listing,
            request,
            plan,
        }
    }
}

/// Where a clone goes unless the user picks another folder: where the last
/// clone went, else beside the most recently opened repository, else HOME.
pub fn default_clone_parent() -> PathBuf {
    let saved = super::settings::read_setting(CLONE_PARENT_KEY)
        .map(PathBuf::from)
        .filter(|p| p.is_dir());
    saved
        .or_else(|| {
            super::tabs::recent_repos()
                .first()
                .and_then(|p| p.parent().map(Path::to_path_buf))
        })
        .unwrap_or_else(home_dir)
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

impl KagiApp {
    /// Read the list unless it is loaded or loading.
    pub(crate) fn ensure_home_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.home_github.filter.is_none() {
            let input =
                cx.new(|cx| InputState::new(window, cx).placeholder(Msg::HomeGithubFilter.t()));
            cx.subscribe(&input, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            self.home_github.filter = Some(input);
        }
        if matches!(self.home_github.repos, GithubRepos::NotLoaded) {
            self.reload_home_github(cx);
        }
    }

    /// Read `gh repo list` and the local clones again.
    pub fn reload_home_github(&mut self, cx: &mut Context<Self>) {
        self.home_github.generation += 1;
        let generation = self.home_github.generation;
        self.home_github.repos = GithubRepos::Loading;
        let mut paths = super::tabs::recent_repos();
        paths.extend(
            self.tabs
                .iter()
                .filter(|t| t.remote.is_none())
                .map(|t| t.path.clone()),
        );
        let work = cx.background_spawn(async move {
            let list = kagi_git::github_repos::list_my_repos(&home_dir());
            let local: HashMap<String, PathBuf> = paths
                .into_iter()
                .filter_map(|p| kagi_git::github_repos::origin_identity(&p).map(|id| (id, p)))
                .collect();
            (list, local)
        });
        cx.spawn(async move |app, acx| {
            let (list, local) = work.await;
            let _ = app.update(acx, |app, cx| {
                if app.home_github.generation != generation {
                    return;
                }
                app.home_github.repos = match list {
                    Ok(sections) => {
                        let repos: usize = sections
                            .iter()
                            .filter_map(|s| s.list.as_ref().ok())
                            .map(|l| l.repos.len())
                            .sum();
                        klog!(
                            "home: github owners={} repos={} local={}",
                            sections.len(),
                            repos,
                            local.len()
                        );
                        GithubRepos::Loaded { sections, local }
                    }
                    Err(error) => {
                        klog!("home: github failed: {error}");
                        GithubRepos::Failed(error.to_string())
                    }
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// A listed repository was clicked: open its local clone, or review a
    /// clone of it.
    pub fn home_github_pick(&mut self, listing: RepoListing, cx: &mut Context<Self>) {
        let local = match &self.home_github.repos {
            GithubRepos::Loaded { local, .. } => local.get(&listing.identity()).cloned(),
            _ => None,
        };
        match local.filter(|p| p.is_dir()) {
            Some(path) => {
                self.open_repository(path, cx);
            }
            None => self.open_clone_card(listing, &default_clone_parent(), cx),
        }
        cx.notify();
    }

    pub fn open_clone_card(&mut self, listing: RepoListing, parent: &Path, cx: &mut Context<Self>) {
        self.modal_focus = Some(cx.focus_handle());
        let modal = CloneModal::new(listing, parent);
        klog!(
            "clone: plan {} -> {} blockers={}",
            modal.request.source,
            modal.request.dest.display(),
            modal.plan.blockers.len()
        );
        self.set_clone_modal(modal);
    }

    /// Pick another folder to clone into; the card is planned again for it.
    pub fn change_clone_parent(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(SharedString::from(Msg::CloneChooseFolder.t())),
        });
        cx.spawn(async move |app, acx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(parent) = paths.into_iter().next() else {
                return;
            };
            let _ = app.update(acx, |app, cx| {
                app.replan_clone(&parent, cx);
            });
        })
        .detach();
    }

    /// Plan the open card again with `parent` as the folder to clone into.
    pub fn replan_clone(&mut self, parent: &Path, cx: &mut Context<Self>) {
        let Some(listing) = self.clone_modal().map(|m| m.listing.clone()) else {
            return;
        };
        self.open_clone_card(listing, parent, cx);
        cx.notify();
    }

    pub fn cancel_clone(&mut self) {
        self.clear_clone_modal();
    }

    /// Confirm the card: run the planned clone in the background. Refused on
    /// the spot while another clone of this window is running.
    pub fn start_clone(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.clone_modal().cloned() else {
            return;
        };
        if !modal.plan.blockers.is_empty() {
            return;
        }
        if self.home_github.cloning.is_some() {
            self.push_toast(super::ToastKind::Error, Msg::CloneBusy.t(), cx);
            return;
        }
        self.clear_clone_modal();
        let CloneModal { request, plan, .. } = modal;
        klog!(
            "clone: start {} -> {}",
            request.source,
            request.dest.display()
        );
        self.home_github.cloning = Some(request.clone());
        let run = request.clone();
        let work = cx.background_spawn(async move { kagi_git::ops::execute_clone(&run, &plan) });
        cx.spawn(async move |app, acx| {
            let report = work.await;
            let _ = app.update(acx, |app, cx| app.finish_clone(request, report, cx));
        })
        .detach();
        cx.notify();
    }

    fn finish_clone(
        &mut self,
        request: CloneRequest,
        report: kagi_git::backend::recording::RunReport,
        cx: &mut Context<Self>,
    ) {
        self.home_github.cloning = None;
        klog!(
            "clone: done {} ok={}",
            request.dest.display(),
            report.result.is_ok()
        );
        self.notice_recording_failure("clone", &report.recording, &request.dest);
        self.present_recorded(&report.recording, cx);
        if report.result.is_ok() {
            if let Some(parent) = request.dest.parent() {
                super::settings::write_setting(CLONE_PARENT_KEY, Some(&parent.to_string_lossy()));
            }
            if let GithubRepos::Loaded { local, .. } = &mut self.home_github.repos {
                if let Some(id) = kagi_git::github_repos::origin_identity(&request.dest) {
                    local.insert(id, request.dest.clone());
                }
            }
            self.open_repository(request.dest, cx);
        }
        cx.notify();
    }

    /// The GitHub section of Home's main column: a search field, then one
    /// section per owner (the user first, then each organization).
    pub(crate) fn render_home_github(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let refresh = cx.listener(|app, _: &gpui::ClickEvent, _, cx| {
            app.reload_home_github(cx);
            cx.notify();
        });
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_lg()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(Msg::HomeGithubTitle.t())),
            )
            .child(super::e2e::measure_control(
                "home-github-refresh",
                Button::new("home-github-refresh")
                    .ghost()
                    .small()
                    .label(Msg::HomeGithubRefresh.t())
                    .on_click(refresh),
            ));
        let query = self
            .home_github
            .filter
            .as_ref()
            .map(|f| f.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();
        let body = match self.home_github.repos.clone() {
            GithubRepos::NotLoaded | GithubRepos::Loading => {
                muted(Msg::HomeGithubLoading.t().to_string())
            }
            GithubRepos::Failed(error) => div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().color_blocker))
                        .child(safe_text(&error)),
                )
                .child(muted(Msg::HomeGithubFailedHint.t().to_string()))
                .into_any_element(),
            GithubRepos::Loaded { sections, local } => {
                self.render_github_sections(&sections, &local, &query, cx)
            }
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .pt_6()
            .child(header)
            .children(self.home_github.filter.as_ref().map(search_field))
            .child(body)
            .into_any_element()
    }

    fn render_github_sections(
        &mut self,
        sections: &[OwnerRepos],
        local: &HashMap<String, PathBuf>,
        query: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cloning = self.home_github.cloning.as_ref().map(|r| r.source.clone());
        let mut col = div().flex().flex_col().gap_4();
        let mut shown = 0usize;
        for section in sections {
            let title = section
                .owner
                .clone()
                .unwrap_or_else(|| Msg::HomeGithubMine.t().to_string());
            let list = match &section.list {
                Ok(list) => list,
                Err(error) => {
                    // An unreadable organization still shows, with why;
                    // filtered out like an empty one while searching.
                    if query.is_empty() {
                        col = col.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(section_heading(&title))
                                .child(muted(Msg::HomeGithubOwnerFailed.t().replace("{}", error))),
                        );
                    }
                    continue;
                }
            };
            let mut rows = div().flex().flex_col();
            let mut in_section = 0usize;
            for listing in &list.repos {
                let matches = query.is_empty()
                    || listing.name_with_owner.to_lowercase().contains(query)
                    || listing.description.to_lowercase().contains(query);
                if !matches {
                    continue;
                }
                in_section += 1;
                let state = if cloning.as_deref() == Some(listing.clone_source().as_str()) {
                    Msg::HomeGithubCloning.t()
                } else if local.contains_key(&listing.identity()) {
                    Msg::HomeGithubOpen.t()
                } else {
                    Msg::HomeGithubClone.t()
                };
                rows = rows.child(super::e2e::measure_control(
                    format!("home-gh-{}", listing.name_with_owner),
                    github_row(listing.clone(), state, cx),
                ));
            }
            shown += in_section;
            if in_section == 0 && !query.is_empty() {
                continue;
            }
            let mut block = div()
                .flex()
                .flex_col()
                .gap_1()
                .child(section_heading(&title));
            if list.repos.is_empty() {
                block = block.child(muted(Msg::HomeGithubEmpty.t().to_string()));
            }
            block = block.child(rows);
            if list.truncated {
                block = block.child(muted(
                    Msg::HomeGithubTruncated
                        .t()
                        .replace("{}", &list.repos.len().to_string()),
                ));
            }
            col = col.child(block);
        }
        if shown == 0 && !query.is_empty() {
            col = col.child(muted(Msg::HomeGithubNoMatch.t().to_string()));
        }
        col.into_any_element()
    }
}

/// A large, borderless search field in a soft rounded box with a magnifier
/// — the search look of current desktop apps, not a form input.
fn search_field(input: &Entity<InputState>) -> AnyElement {
    // A flex row so the input takes the box's whole width: as a plain child
    // its `size_full` did not resolve, leaving the text area zero-wide —
    // typed characters and the placeholder were drawn into nothing.
    div()
        .flex()
        .flex_row()
        .items_center()
        .w_full()
        .px_2()
        .py_1()
        .rounded_xl()
        .bg(rgb(theme().panel))
        .border_1()
        .border_color(rgb(theme().surface))
        .child(
            Input::new(input)
                .appearance(false)
                .cleanable(true)
                .large()
                .flex_1()
                .prefix(
                    Icon::new(IconName::Search)
                        .size(theme::scaled_px(18.))
                        .text_color(rgb(theme().text_muted)),
                ),
        )
        .into_any_element()
}

fn section_heading(title: &str) -> impl IntoElement {
    div()
        .px_3()
        .text_sm()
        .text_color(rgb(theme().text_muted))
        .child(SharedString::from(title.to_string()))
}

fn muted(text: String) -> AnyElement {
    div()
        .px_3()
        .text_sm()
        .text_color(rgb(theme().text_muted))
        .child(SharedString::from(text))
        .into_any_element()
}

/// One repository: its name over its description on the left; on the right
/// the last update, private / fork marks and what a click does, as a chip.
fn github_row(listing: RepoListing, state: &'static str, cx: &mut Context<KagiApp>) -> AnyElement {
    let chip = |text: &str| {
        div()
            .flex_shrink_0()
            .px_2()
            .rounded_md()
            .bg(rgb(theme().surface))
            .text_xs()
            .text_color(rgb(theme().text_sub))
            .child(SharedString::from(text.to_string()))
    };
    let mut meta = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .flex_shrink_0();
    if let Some(day) = listing.updated_at.get(..10) {
        meta = meta.child(
            div()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(day.to_string())),
        );
    }
    if listing.is_private {
        meta = meta.child(chip(Msg::HomeGithubPrivate.t()));
    }
    if listing.is_fork {
        meta = meta.child(chip(Msg::HomeGithubFork.t()));
    }
    meta = meta.child(chip(state).text_color(rgb(theme().color_branch)));
    let description = (!listing.description.is_empty()).then(|| {
        div()
            .text_sm()
            .text_color(rgb(theme().text_muted))
            .truncate()
            .child(safe_text(&listing.description))
    });
    let id = SharedString::from(format!("home-gh-row-{}", listing.name_with_owner));
    let name = listing.name().to_string();
    let pick = cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
        app.home_github_pick(listing.clone(), cx);
    });
    div()
        .id(id)
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_4()
        .px_3()
        .py_2()
        .rounded_lg()
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(pick)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .child(
                    div()
                        .text_base()
                        .text_color(rgb(theme().text_main))
                        .truncate()
                        .child(SharedString::from(name)),
                )
                .children(description),
        )
        .child(meta)
        .into_any_element()
}

/// The clone card: the plan's current → predicted box, its warnings and
/// blockers, the destination with "Change…", and Clone (absent when blocked).
pub(crate) fn render_clone_modal(
    modal: CloneModal,
    focus_handle: Option<FocusHandle>,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let plan = &modal.plan;
    let blocked = !plan.blockers.is_empty();
    let cancel = cx.listener(|app, _: &gpui::ClickEvent, window, cx| {
        app.cancel_clone();
        if let Some(root) = app.root_focus.clone() {
            window.focus(&root, cx);
        }
        cx.notify();
    });
    let confirm = cx.listener(|app, _: &gpui::ClickEvent, window, cx| {
        app.start_clone(cx);
        if let Some(root) = app.root_focus.clone() {
            window.focus(&root, cx);
        }
        cx.notify();
    });
    let change = cx.listener(|app, _: &gpui::ClickEvent, _, cx| {
        app.change_clone_parent(cx);
    });

    let mut body = modal_scroll_body()
        .child(
            div()
                .flex_shrink_0()
                .child(render_current_predicted(plan, None)),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from(Msg::CloneDestination.t())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_sm()
                        .font_family(super::MONO_FONT)
                        .text_color(rgb(theme().text_main))
                        .child(safe_text(&modal.request.dest.display().to_string())),
                )
                .child(super::e2e::measure_control(
                    "clone-change-folder",
                    Button::new("clone-change-folder")
                        .ghost()
                        .small()
                        .label(Msg::CloneChooseFolder.t())
                        .on_click(change),
                )),
        );
    for (i, note) in plan.warnings.iter().enumerate() {
        body = body.child(super::plan_card_rows::render_note_row(
            SharedString::from(format!("clone-warning-{i}")),
            false,
            "!",
            theme().color_warning,
            &plan_note_text(note),
            false,
        ));
    }
    for (i, note) in plan.blockers.iter().enumerate() {
        body = body.child(super::plan_card_rows::render_note_row(
            SharedString::from(format!("clone-blocker-{i}")),
            true,
            "\u{2717}",
            theme().color_blocker,
            &plan_note_text(note),
            false,
        ));
    }
    if let Some(command) = plan.equivalent_command.as_deref() {
        body = body.child(
            div()
                .flex_shrink_0()
                .text_xs()
                .font_family(super::MONO_FONT)
                .text_color(rgb(theme().text_muted))
                .child(safe_text(command)),
        );
    }

    let mut buttons =
        div()
            .flex()
            .flex_row()
            .gap_2()
            .justify_end()
            .child(super::e2e::measure_control(
                "clone-cancel",
                Button::new("clone-cancel")
                    .label(Msg::PlanCancel.t())
                    .ghost()
                    .small()
                    .on_click(cancel),
            ));
    if !blocked {
        buttons = buttons.child(super::e2e::measure_control(
            "clone-confirm",
            Button::new("clone-confirm")
                .primary()
                .small()
                .label(Msg::CloneConfirm.t())
                .on_click(confirm),
        ));
    }
    let card = modal_card(MODAL_W_LG)
        .child(div().flex_shrink_0().child(render_modal_title_row(
            SharedString::from(plan_title_text(&plan.title)),
            None,
        )))
        .child(body)
        .child(div().flex_shrink_0().child(buttons));
    let esc = cx.listener(|app, e: &KeyDownEvent, window, cx| {
        if e.keystroke.key == "escape" {
            app.cancel_clone();
            if let Some(root) = app.root_focus.clone() {
                window.focus(&root, cx);
            }
            cx.stop_propagation();
            cx.notify();
        }
    });
    let base = div().on_key_down(esc);
    let card = match focus_handle {
        Some(focus) => base.track_focus(&focus).child(card),
        None => base.child(card),
    };
    modal_overlay(super::e2e::measure_control("active-modal/clone", card)).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(name_with_owner: &str) -> RepoListing {
        RepoListing {
            name_with_owner: name_with_owner.to_string(),
            host: "github.com".to_string(),
            is_fork: false,
            is_private: false,
            description: String::new(),
            updated_at: String::new(),
        }
    }

    /// The card's destination is the chosen folder plus the repository's own
    /// name, cloned from the host the list came from.
    #[test]
    fn the_card_clones_into_a_folder_named_after_the_repository() {
        let parent = tempfile::tempdir().unwrap();
        let modal = CloneModal::new(listing("acme/widgets"), parent.path());
        assert_eq!(modal.request.dest, parent.path().join("widgets"));
        assert_eq!(modal.request.source, "github.com/acme/widgets");
        assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
    }
}
