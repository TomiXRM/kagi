//! "Open in external editor" (R2, requirements session 2026-08-12).
//!
//! One `settings.json` key drives every entry point:
//!
//! ```json
//! { "external_editor": "code -g {file}:{line}" }
//! ```
//!
//! `{file}` → absolute path (shell-quoted), `{line}` → 1-based line (1 when the
//! caller has none). Unset → the OS opener (`open` / `xdg-open`), which cannot
//! jump to a line but needs zero setup.

use std::path::{Path, PathBuf};

use gpui::{Context, SharedString};

use super::i18n::Msg;
use super::settings;
use super::types::ToastKind;
use super::KagiApp;

pub(crate) const KEY_EXTERNAL_EDITOR: &str = "external_editor";

/// Expand the command template. Pure — unit-tested below.
///
/// The path is single-quoted for `sh -c` (embedded `'` becomes `'\''`);
/// `{line}` falls back to 1 so a `{file}:{line}` template still forms a valid
/// argument when the caller has no line to jump to.
fn expand_template(template: &str, file: &Path, line: Option<u32>) -> String {
    let quoted = format!("'{}'", file.to_string_lossy().replace('\'', r"'\''"));
    template
        .replace("{file}", &quoted)
        .replace("{line}", &line.unwrap_or(1).to_string())
}

/// The platform opener used when `external_editor` is unset.
fn os_opener() -> &'static str {
    if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    }
}

impl KagiApp {
    /// Open `rel_path` (repo-relative) in the user's external editor.
    ///
    /// Never blocks and never fails hard: a broken template surfaces as an
    /// error toast, an unset one falls back to the OS opener.
    pub fn open_in_external_editor(
        &mut self,
        rel_path: &Path,
        line: Option<u32>,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.repo_path.clone() else {
            return;
        };
        let abs: PathBuf = repo.join(rel_path);

        let template = settings::read_setting(KEY_EXTERNAL_EDITOR).filter(|t| !t.trim().is_empty());
        let spawned = match template {
            Some(t) => {
                let cmd = expand_template(&t, &abs, line);
                klog!("external-editor: {}", cmd);
                std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&cmd)
                    .current_dir(&repo)
                    .spawn()
                    .map(|_| ())
            }
            None => {
                klog!("external-editor: os-open {}", abs.display());
                std::process::Command::new(os_opener())
                    .arg(&abs)
                    .spawn()
                    .map(|_| ())
            }
        };

        match spawned {
            Ok(()) => self.push_toast(
                ToastKind::Info,
                SharedString::from(format!(
                    "{}: {}",
                    Msg::OpenInExternalEditor.t(),
                    rel_path.display()
                )),
                cx,
            ),
            Err(e) => self.push_toast(
                ToastKind::Error,
                SharedString::from(format!("{}: {}", Msg::OpenInExternalEditor.t(), e)),
                cx,
            ),
        }
    }
}

impl KagiApp {
    /// Open one or more ABSOLUTE paths in the external editor (#321: the two
    /// materialized sides of a binary conflict, for a manual compare). Each path
    /// is opened through the same `external_editor` template / OS-opener plumbing
    /// as [`Self::open_in_external_editor`]; never blocks, never fails hard.
    pub(crate) fn open_files_external(&mut self, abs_paths: &[PathBuf], cx: &mut Context<Self>) {
        let template = settings::read_setting(KEY_EXTERNAL_EDITOR).filter(|t| !t.trim().is_empty());
        for abs in abs_paths {
            let spawned = match &template {
                Some(t) => {
                    let cmd = expand_template(t, abs, None);
                    klog!("external-editor: {}", cmd);
                    std::process::Command::new("sh")
                        .arg("-c")
                        .arg(&cmd)
                        .spawn()
                        .map(|_| ())
                }
                None => {
                    klog!("external-editor: os-open {}", abs.display());
                    std::process::Command::new(os_opener())
                        .arg(abs)
                        .spawn()
                        .map(|_| ())
                }
            };
            if let Err(e) = spawned {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("{}: {}", Msg::OpenInExternalEditor.t(), e)),
                    cx,
                );
                return;
            }
        }
        self.push_toast(
            ToastKind::Info,
            SharedString::from(Msg::ConflictOpenBothExternal.t()),
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_file_and_line() {
        assert_eq!(
            expand_template("code -g {file}:{line}", Path::new("/a/b.rs"), Some(42)),
            "code -g '/a/b.rs':42"
        );
    }

    #[test]
    fn line_defaults_to_1_so_the_template_stays_valid() {
        assert_eq!(
            expand_template("vim +{line} {file}", Path::new("/a/b.rs"), None),
            "vim +1 '/a/b.rs'"
        );
    }

    #[test]
    fn quotes_paths_with_spaces_and_apostrophes() {
        assert_eq!(
            expand_template("edit {file}", Path::new("/a dir/it's.rs"), None),
            r"edit '/a dir/it'\''s.rs'"
        );
    }
}
