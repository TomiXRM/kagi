//! Per-file "viewed" marks in a PR tab's file list (#351, ADR-0207).
//!
//! A mark holds the file's head-side blob id; the file reads as viewed only
//! while the fetched PR head still has that blob (`kagi_domain::pr_viewed`).
//! Marks are this machine's reading state, stored per PR under
//! `~/.kagi/pr-viewed/` (`kagi_ui_core::pr_viewed`) — not a repository write,
//! so nothing goes to the Operation Log.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::prelude::*;
use gpui::{div, Context, SharedString};
use kagi_domain::github::PullRequest;
use kagi_domain::pr_viewed::ViewedFiles;
use kagi_git::FileStatus;

use super::{KagiApp, PrView};
use crate::ui::i18n;
use crate::ui::theme;
use crate::ui::types::ToastKind;

/// A PR tab's marks plus the head blob of each of the PR's files.
#[derive(Debug, Default)]
pub struct PrViewed {
    marks: ViewedFiles,
    /// Head-side blob per PR file, from the fetched head. Empty until the
    /// head is loaded, and again once the listed head moves past it: an
    /// unknown blob is never viewed.
    head_blobs: HashMap<PathBuf, String>,
}

fn key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl PrViewed {
    /// The marks stored for `pr` (none for a PR with no readable identity).
    pub(crate) fn load(pr: &PullRequest) -> Self {
        Self {
            marks: kagi_ui_core::pr_viewed::load(&pr.base_repo, pr.number),
            head_blobs: HashMap::new(),
        }
    }

    /// The loaded head's blob for each of the PR's files (same order).
    pub(crate) fn set_head_blobs(&mut self, files: &[FileStatus], blobs: Vec<String>) {
        self.head_blobs = files
            .iter()
            .map(|file| file.path.clone())
            .zip(blobs)
            .collect();
    }

    /// The listed head moved past the loaded one: every blob is unknown
    /// until the new head is fetched.
    pub(crate) fn clear_head_blobs(&mut self) {
        self.head_blobs.clear();
    }

    fn head_blob(&self, path: &Path) -> &str {
        self.head_blobs.get(path).map(String::as_str).unwrap_or("")
    }

    pub fn is_viewed(&self, path: &Path) -> bool {
        self.marks.is_viewed(&key(path), self.head_blob(path))
    }

    pub fn viewed_count(&self, files: &[FileStatus]) -> usize {
        files
            .iter()
            .filter(|file| self.is_viewed(&file.path))
            .count()
    }

    /// Mark `path` viewed at its current head blob, or clear its mark.
    pub(crate) fn set(&mut self, path: &Path, viewed: bool) {
        let blob = self.head_blob(path).to_string();
        self.marks.set(&key(path), &blob, viewed);
    }
}

/// The marks the file list shows: the whole-PR list only — not one commit's
/// files, not the conflicts list.
pub(super) fn marks_shown(tab: Option<&super::PrTab>, conflicts_view: bool) -> Option<&PrViewed> {
    tab.filter(|t| !conflicts_view && t.selected_commit.is_none())
        .map(|t| &t.viewed)
}

/// `" · 2 / 5 viewed"` for the file-list header.
pub(super) fn progress_suffix(viewed: &PrViewed, files: &[FileStatus]) -> String {
    format!(
        " · {}",
        i18n::pr_viewed::progress(viewed.viewed_count(files), files.len())
    )
}

/// Row `ix`'s viewed checkbox. Its click does not also select the row.
pub(super) fn checkbox(ix: usize, checked: bool, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let app = cx.entity();
    let checkbox = gpui_component::checkbox::Checkbox::new(("pr-file-viewed", ix))
        .checked(checked)
        .tooltip(i18n::pr_viewed::checkbox_tooltip())
        .on_click(move |_checked: &bool, _window, cx| {
            cx.stop_propagation();
            app.update(cx, |app, cx| app.pr_mode_toggle_viewed(ix, cx));
        });
    let control = div()
        .flex_shrink_0()
        .w(theme::scaled_px(18.))
        .child(checkbox);
    crate::ui::e2e::measure_control(format!("pr-file-viewed-{ix}"), control)
}

impl KagiApp {
    /// Toggle the viewed mark of the active PR tab's file `ix` and store the
    /// PR's marks. Only the whole-PR file list carries marks (not a single
    /// commit's files, not the conflicts list).
    pub fn pr_mode_toggle_viewed(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(m) = self.pr_mode_mut() else { return };
        if m.view == PrView::Conflicts {
            return;
        }
        let Some(tab) = m.active.and_then(|active| m.tabs.get_mut(active)) else {
            return;
        };
        if tab.selected_commit.is_some() {
            return;
        }
        let Some(path) = tab.files.get(ix).map(|file| file.path.clone()) else {
            return;
        };
        let viewed = !tab.viewed.is_viewed(&path);
        tab.viewed.set(&path, viewed);
        let number = tab.pr.number;
        let saved = kagi_ui_core::pr_viewed::save(&tab.pr.base_repo, number, &tab.viewed.marks);
        klog!(
            "pr-viewed: #{} {} viewed={}",
            number,
            path.display(),
            tab.viewed.is_viewed(&path)
        );
        if let Err(error) = saved {
            let text = SharedString::from(i18n::pr_viewed::save_failed(&error));
            self.push_toast(ToastKind::Error, text, cx);
        }
        cx.notify();
    }
}
