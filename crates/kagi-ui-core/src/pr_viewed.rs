//! Where a pull request's "viewed" marks live (#351, ADR-0206):
//! `~/.kagi/pr-viewed/<owner>-<repo>-<pr>.json` (`$KAGI_LOG_DIR/pr-viewed/`
//! when set, like `settings.json`), one file per PR holding a flat
//! `{ "<path>": "<head blob id>" }` object.
//!
//! Writes replace the file atomically. A file that does not parse is moved
//! aside (`….json.corrupt[.N]`) and read as no marks; it is never written
//! over. This is local reading state, not a repository write: no oplog.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kagi_domain::pr_viewed::{viewed_file_name, ViewedFiles};

use crate::atomic_file::{rescue, write_atomic};

/// The file for a pull request, or `None` when its identity is unreadable
/// (such a PR's marks are kept in memory only).
pub fn viewed_path(base_repo: &str, number: u64) -> Option<PathBuf> {
    let dir = crate::settings::settings_path()?
        .parent()?
        .join("pr-viewed");
    Some(dir.join(viewed_file_name(base_repo, number)?))
}

/// The marks stored for a pull request; none when there is no file.
pub fn load(base_repo: &str, number: u64) -> ViewedFiles {
    viewed_path(base_repo, number)
        .map(|path| load_from(&path))
        .unwrap_or_default()
}

/// Store a pull request's marks.
pub fn save(base_repo: &str, number: u64, viewed: &ViewedFiles) -> std::io::Result<()> {
    let path = viewed_path(base_repo, number).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("no storage name for {base_repo} #{number}"),
        )
    })?;
    save_to(&path, viewed)
}

fn parse(text: &str) -> Option<ViewedFiles> {
    serde_json::from_str::<BTreeMap<String, String>>(text)
        .ok()
        .map(ViewedFiles::from_marks)
}

/// A missing or unreadable file reads as no marks; a corrupt one is moved
/// aside first so the next save cannot destroy it.
pub(crate) fn load_from(path: &Path) -> ViewedFiles {
    let Ok(text) = std::fs::read_to_string(path) else {
        return ViewedFiles::default();
    };
    parse(&text).unwrap_or_else(|| {
        if let Some(aside) = rescue(path, "pr-viewed") {
            klog!("pr-viewed: corrupt file moved to {}", aside.display());
        }
        ViewedFiles::default()
    })
}

/// Replace the file with `viewed`. A file that became corrupt since it was
/// read is moved aside first; if that fails, nothing is written.
pub(crate) fn save_to(path: &Path, viewed: &ViewedFiles) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if let Ok(text) = std::fs::read_to_string(path) {
        if parse(&text).is_none() && rescue(path, "pr-viewed").is_none() {
            return Err(std::io::Error::other(
                "corrupt pr-viewed file could not be moved aside; not overwritten",
            ));
        }
    }
    let mut text = serde_json::to_string_pretty(viewed.marks()).map_err(std::io::Error::other)?;
    text.push('\n');
    write_atomic(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "1111111111111111111111111111111111111111";

    fn marks(pairs: &[(&str, &str)]) -> ViewedFiles {
        let mut viewed = ViewedFiles::default();
        for (path, blob) in pairs {
            viewed.set(path, blob, true);
        }
        viewed
    }

    #[test]
    fn marks_round_trip_as_a_flat_object() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("pr-viewed").join("acme-widgets-7.json");
        let viewed = marks(&[("src/a.rs", A), ("dir/b c.md", A)]);
        save_to(&path, &viewed).unwrap();
        let on_disk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            on_disk,
            serde_json::json!({ "src/a.rs": A, "dir/b c.md": A })
        );
        assert_eq!(load_from(&path), viewed);
        assert_eq!(
            load_from(&tmp.path().join("missing.json")),
            ViewedFiles::default()
        );
    }

    #[test]
    fn a_corrupt_file_reads_empty_and_is_kept_aside_not_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("acme-widgets-7.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_from(&path), ViewedFiles::default());
        let aside = tmp.path().join("acme-widgets-7.json.corrupt");
        assert_eq!(std::fs::read_to_string(&aside).unwrap(), "{ not json");
        assert!(!path.exists(), "moved, not copied");

        // Corrupted again after it was read: the next save still keeps it.
        std::fs::write(&path, "[1, 2]").unwrap();
        save_to(&path, &marks(&[("a", A)])).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("acme-widgets-7.json.corrupt.1")).unwrap(),
            "[1, 2]"
        );
        assert_eq!(std::fs::read_to_string(&aside).unwrap(), "{ not json");
        assert_eq!(load_from(&path), marks(&[("a", A)]));
    }
}
