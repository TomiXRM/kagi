//! Accepted stage-0 inspection; never reconstructs a writable resolution buffer.
use super::*;
use std::sync::Arc;

#[derive(Clone)]
pub struct SavedPreview {
    pub path: PathBuf,
    pub oid: String,
    pub bytes: Arc<[u8]>,
    lines: Arc<Vec<kagi_git::resolution::ResolvedLine>>,
}

impl SavedPreview {
    /// Inspect stage 0; gitlinks contain a commit identity, not blob bytes.
    /// The caller brackets this read with the accepted observation's revision.
    fn read_index(
        backend: &kagi_git::Backend,
        path: PathBuf,
        oid: String,
    ) -> Result<Self, kagi_git::GitError> {
        let bytes = if backend.index_file_mode(&path)? == Some(0o160000) {
            oid.as_bytes().to_vec()
        } else {
            backend
                .file_bytes(
                    kagi_git::backend::file_reads::FileReadSource::Index,
                    &path,
                    16 * 1024 * 1024,
                )?
                .ok_or_else(|| kagi_git::GitError::Other("Saved index entry is absent".into()))?
        };
        let text = if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
            use std::fmt::Write;
            let mut hex = String::new();
            for (offset, chunk) in bytes.chunks(16).enumerate() {
                let _ = write!(hex, "{:08x}  ", offset * 16);
                for byte in chunk {
                    let _ = write!(hex, "{byte:02x} ");
                }
                hex.push('\n');
            }
            std::borrow::Cow::Owned(hex)
        } else {
            String::from_utf8_lossy(&bytes)
        };
        let lines = Arc::new(kagi_git::resolution::text_to_lines(
            &text,
            kagi_git::resolution::LineOrigin::Context,
        ));
        Ok(Self {
            path,
            oid,
            bytes: bytes.into(),
            lines,
        })
    }
}

impl ConflictView {
    pub fn conflict_open_saved(&mut self, idx: usize, cx: &mut Context<Self>) {
        self.capture_result_for_navigation(cx);
        let Some(file) = self.mode.as_ref().and_then(|m| m.session.resolved.get(idx)) else {
            return;
        };
        let path = file.path.clone();
        self.selected_saved = Some(path);
        self.editing = None;
        self.editor_inputs = None;
        self.result_editing = false;
        self.reset_all_target = None;
        self.file_menu = None;
        self.show_tools = false;
        self.page = ConflictPage::Review;
        if let Some(mode) = &mut self.mode {
            mode.selected_file = None;
            mode.editing_file = None;
        }
        self.retire_raw_preview(cx);
        self.load_saved_preview(cx);
    }

    pub(super) fn clear_saved_reads(&mut self) {
        self.saved_cache.clear();
        self.saved_preview = None;
        self.saved_read = None;
        self.saved_error = false;
    }

    /// A newly accepted detection invalidates inspection reads, not draft owners.
    pub(in crate::ui) fn refresh_saved_reads(&mut self, cx: &mut Context<Self>) {
        self.saved_review_needed = false;
        self.clear_saved_reads();
        self.load_saved_preview(cx);
        cx.notify();
    }

    pub(super) fn load_saved_preview(&mut self, cx: &mut Context<Self>) {
        let Some(mode) = self.mode.as_ref() else {
            return;
        };
        let Some(file) = self
            .selected_saved
            .as_ref()
            .and_then(|p| mode.session.resolved.iter().find(|f| &f.path == p))
        else {
            return;
        };
        let key = (file.path.clone(), file.oid.clone());
        self.saved_preview = self.saved_cache.get(&key).cloned();
        if self.saved_preview.is_some()
            || self.saved_read.as_ref() == Some(&key)
            || self.saved_review_needed
        {
            return;
        }
        self.saved_read = Some(key.clone());
        self.saved_error = false;
        let revision = mode.revision.clone();
        let repo = self.repo_path.clone();
        let (path, oid) = key.clone();
        let expected = revision.clone();
        let task = cx.background_spawn(async move {
            let backend = kagi_git::Backend::open(&repo)?;
            let before = backend.conflict_snapshot()?;
            if before
                .as_ref()
                .is_none_or(|s| s.observation.revision != expected)
            {
                return Err(kagi_git::GitError::Other(
                    "Conflict observation changed".into(),
                ));
            }
            let preview = SavedPreview::read_index(&backend, path, oid)?;
            let after = backend.conflict_snapshot()?;
            if after
                .as_ref()
                .is_none_or(|s| s.observation.revision != expected)
            {
                return Err(kagi_git::GitError::Other(
                    "Conflict observation changed".into(),
                ));
            }
            Ok(preview)
        });
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                if view.mode.as_ref().is_none_or(|m| m.revision != revision)
                    || view.saved_read.as_ref() != Some(&key)
                {
                    return;
                }
                view.saved_read = None;
                match result {
                    Ok(preview) => {
                        view.saved_cache.insert(key, preview.clone());
                        if view.selected_saved.as_ref() == Some(&preview.path) {
                            view.saved_preview = Some(preview);
                        }
                    }
                    Err(_) => {
                        view.saved_error = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_saved_header(&self) -> gpui::AnyElement {
        div()
            .relative()
            .flex()
            .items_center()
            .flex_shrink_0()
            .h(theme::scaled_px(26.))
            .px(theme::scaled_px(12.))
            .gap(theme::scaled_px(8.))
            .bg(rgb(theme().panel))
            .border_b_1()
            .border_color(rgb(theme().surface))
            .text_size(theme::scaled_px(11.5))
            .font_weight(gpui::FontWeight::MEDIUM)
            .child(
                gpui::svg()
                    .path("icons/lock-keyhole.svg")
                    .size(theme::scaled_px(12.))
                    .text_color(rgb(theme().text_sub)),
            )
            .child(Msg::ConflictStagedContents.t())
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .child(SharedString::from(
                        self.selected_saved
                            .as_ref()
                            .and_then(|p| p.file_name())
                            .map(|name| format!("— {}", name.to_string_lossy()))
                            .unwrap_or_default(),
                    )),
            )
            .child(e2e::measure_inside("conflict-saved-source-header"))
            .into_any_element()
    }

    pub(super) fn render_saved(
        &self,
        chrome: &EditorChrome,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let body = if self.saved_review_needed {
            div().into_any_element()
        } else if let Some(preview) = &self.saved_preview {
            crate::ui::conflict_editor::render_saved_preview(
                preview.lines.clone(),
                chrome,
                &preview.path,
                cx,
            )
        } else {
            div()
                .p(theme::scaled_px(12.))
                .text_color(rgb(theme().text_sub))
                .child(if self.saved_error {
                    Msg::DiffPdfReadFailed.t()
                } else {
                    Msg::EditorWorkspaceLoading.t()
                })
                .into_any_element()
        };
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .bg(rgb(theme().bg_base))
            .child(e2e::measure_inside("conflict-saved-preview"))
            .child(body)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::SavedPreview;
    use std::path::Path;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn submodule_conflict() -> (tempfile::TempDir, String) {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path();
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["config", "user.email", "saved-preview@example.com"]);
        git(dir, &["config", "user.name", "Saved preview"]);
        let mut targets = Vec::new();
        for label in ["base", "current", "incoming"] {
            git(dir, &["commit", "--allow-empty", "-qm", label]);
            targets.push(git(dir, &["rev-parse", "HEAD"]));
        }
        git(
            dir,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{},sub", targets[0]),
            ],
        );
        git(dir, &["commit", "-qm", "base gitlink"]);
        git(dir, &["checkout", "-qb", "side"]);
        git(
            dir,
            &[
                "update-index",
                "--cacheinfo",
                &format!("160000,{},sub", targets[2]),
            ],
        );
        git(dir, &["commit", "-qm", "incoming gitlink"]);
        git(dir, &["checkout", "-q", "main"]);
        git(
            dir,
            &[
                "update-index",
                "--cacheinfo",
                &format!("160000,{},sub", targets[1]),
            ],
        );
        git(dir, &["commit", "-qm", "current gitlink"]);
        let merge = Command::new("git")
            .args(["merge", "side"])
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(!merge.status.success(), "gitlink merge must conflict");
        (temp, targets.remove(2))
    }

    #[test]
    fn saved_gitlink_preview_displays_chosen_commit_oid() {
        let (temp, incoming) = submodule_conflict();
        let backend = kagi_git::Backend::open(temp.path()).unwrap();
        let path = Path::new("sub");
        let before = backend.conflict_snapshot().unwrap().unwrap();
        assert!(before.session.files.iter().any(|file| {
            file.path == path && file.kind == kagi_git::conflicts::ConflictKind::Submodule
        }));
        let mut buffer = backend.resolution_buffer_from_repo().unwrap();
        buffer
            .apply_choice(path, kagi_git::ResolutionChoice::Incoming)
            .unwrap();
        backend.execute_conflict_save(&buffer, path).unwrap();
        let saved = backend.conflict_snapshot().unwrap().unwrap();
        let file = saved
            .session
            .resolved
            .iter()
            .find(|file| file.path == path)
            .unwrap();
        assert_eq!(file.oid, incoming);
        assert_eq!(backend.index_file_mode(path).unwrap(), Some(0o160000));

        // Exercise the same read and line construction used by the Saved pane,
        // not a formatter with fabricated side metadata.
        let preview = SavedPreview::read_index(&backend, file.path.clone(), file.oid.clone())
            .expect("saved gitlink must not be read as a blob");
        assert_eq!(preview.bytes.as_ref(), incoming.as_bytes());
        assert_eq!(preview.lines.len(), 1);
        assert_eq!(preview.lines[0].text, incoming);
        assert_eq!(
            backend
                .conflict_snapshot()
                .unwrap()
                .unwrap()
                .observation
                .revision,
            saved.observation.revision,
            "inspection must not mutate the accepted stage-0 state"
        );
    }
}
