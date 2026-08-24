//! Where a Markdown image's path points, and whether it is allowed to.
//!
//! See `markdown/mod.rs` for how this fits with `extract.rs`.

use std::path::{Component, Path, PathBuf};

use super::extract::has_uri_scheme;

/// Filesystem context for resolving image paths in a repository Markdown file.
#[derive(Clone, Debug)]
pub struct MarkdownImageBase {
    repo_root: PathBuf,
    document_dir: PathBuf,
}

impl MarkdownImageBase {
    /// Build a base from the repository root and the Markdown file's repo-relative path.
    pub fn repo_file(repo_root: impl Into<PathBuf>, document: &Path) -> Self {
        Self {
            repo_root: repo_root.into(),
            document_dir: document
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .to_path_buf(),
        }
    }

    pub(super) fn resolve(&self, url: &str) -> Option<PathBuf> {
        let raw = url.split(['?', '#']).next().unwrap_or(url);
        if raw.is_empty() || has_uri_scheme(raw) {
            return None;
        }

        let relative = if let Some(repo_relative) = raw.strip_prefix('/') {
            PathBuf::from(repo_relative)
        } else {
            self.document_dir.join(raw)
        };
        let relative = normalize_repo_relative(&relative)?;
        Some(self.repo_root.join(relative))
    }
}

fn normalize_repo_relative(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => out.push(part),
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_paths_relative_to_the_markdown_file() {
        let base = MarkdownImageBase::repo_file("/repo", Path::new("docs/guide/readme.md"));
        assert_eq!(
            base.resolve("../images/screen.png"),
            Some(PathBuf::from("/repo/docs/images/screen.png"))
        );
        assert_eq!(
            base.resolve("/assets/logo.png#dark"),
            Some(PathBuf::from("/repo/assets/logo.png"))
        );
    }

    #[test]
    fn rejects_repo_escape_and_uri_sources() {
        let base = MarkdownImageBase::repo_file("/repo", Path::new("README.md"));
        assert_eq!(base.resolve("../secret.png"), None);
        assert_eq!(base.resolve("https://example.com/image.png"), None);
        assert_eq!(base.resolve("data:image/png;base64,AAAA"), None);
    }
}
