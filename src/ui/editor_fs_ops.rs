//! Pure + filesystem helpers for the Editor Workspace's tree context-menu file
//! operations (T-WS-EDITOR-007): name validation, the macOS Trash move (with a
//! same-volume collision-suffix numbering scheme), and `.gitignore` append.
//!
//! Kept separate from `editor_workspace.rs` (already at the ~2,200 LOC file-size
//! ceiling — CLAUDE.md targets 800) and from `operations/editor_fs.rs` (the
//! `KagiApp` entry points that call these) so the validation / collision-suffix
//! / gitignore logic stays unit-testable without a `Context`.

use std::path::{Path, PathBuf};

/// Validate a bare filename/dirname component typed into the Rename / New
/// File / New Folder prompt. Char-safe (`.chars()`, never byte-indexed —
/// Japanese filenames are common in this repo's fixtures) and rejects
/// anything that could escape the target directory or collide with `.git`.
/// Pure; unit-tested below.
pub fn validate_fs_name(name: &str) -> Result<(), &'static str> {
    if name.trim().is_empty() {
        return Err("Name cannot be empty");
    }
    if name == "." || name == ".." || name == ".git" {
        return Err("Invalid name");
    }
    if name.chars().any(|c| c == '/' || c == '\\') {
        return Err("Name cannot contain a path separator");
    }
    Ok(())
}

/// `true` if `rel` (repo-relative) has `.git` as its first path component —
/// used to refuse any Rename/Delete/New target that would touch the git
/// database itself (CLAUDE.md invariant: fs mutations here are plain
/// `std::fs`, never allowed to reach into `.git`). Pure; unit-tested below.
pub fn path_touches_git_dir(rel: &Path) -> bool {
    rel.components().next() == Some(std::path::Component::Normal(std::ffi::OsStr::new(".git")))
}

/// `true` only on macOS — the Trash-move Delete item is gated to this
/// platform for now (ticket note: non-macOS gets no Delete item; the menu
/// builder checks this to hide it).
pub const TRASH_SUPPORTED: bool = cfg!(target_os = "macos");

/// The `name N.ext` / `name N` collision suffix used when moving `name` into
/// `~/.Trash` and something of the same name is already there. Splits on the
/// LAST `.` so a double extension collides as `archive.tar 2.gz` (accepted
/// quirk, see test), and leaves a dotfile (`.gitignore`) alone — there's no
/// non-empty stem to split before the leading dot. Pure — never touches the
/// filesystem; unit-tested below.
pub fn trash_collision_name(name: &str, n: usize) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem} {n}.{ext}"),
        _ => format!("{name} {n}"),
    }
}

/// Where `~/.Trash` is. Errors if `$HOME` isn't set (defensive — always set
/// in a real macOS user session).
fn home_trash_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join(".Trash"))
}

/// Cap on collision-suffix probing — never loop forever against a
/// pathological `~/.Trash`.
const TRASH_COLLISION_CAP: usize = 1000;

/// Move `full_path` (absolute) to `~/.Trash`, appending a numeric collision
/// suffix (`name 2`, `name 3`, …) if something of the same name is already
/// there. Same-volume only: a cross-volume `rename` failure is surfaced as an
/// `Err` and NOTHING is deleted — never falls back to a permanent delete
/// (CLAUDE.md invariant #3). Callers must check `TRASH_SUPPORTED` first; this
/// function itself only compiles on macOS.
#[cfg(target_os = "macos")]
pub fn trash_path(full_path: &Path) -> Result<PathBuf, String> {
    let trash_dir = home_trash_dir()?;
    std::fs::create_dir_all(&trash_dir).map_err(|e| format!("cannot create ~/.Trash: {e}"))?;
    let name = full_path
        .file_name()
        .ok_or("path has no file name")?
        .to_string_lossy()
        .into_owned();
    let mut candidate = trash_dir.join(&name);
    let mut n = 2usize;
    while candidate.exists() {
        if n > TRASH_COLLISION_CAP {
            return Err("~/.Trash has too many same-named items".to_string());
        }
        candidate = trash_dir.join(trash_collision_name(&name, n));
        n += 1;
    }
    std::fs::rename(full_path, &candidate)
        .map_err(|e| format!("move to Trash failed (same-volume only): {e}"))?;
    Ok(candidate)
}

/// Append `rel_path` (repo-relative, forward-slash) as a new line in
/// `<repo_path>/.gitignore`, creating the file if it doesn't exist yet.
/// Idempotent: a line already present (exact match after trimming) is left
/// alone rather than duplicated. Pure I/O; unit-tested below.
pub fn add_gitignore_entry(repo_path: &Path, rel_path: &str) -> std::io::Result<()> {
    let gi_path = repo_path.join(".gitignore");
    let existing = std::fs::read_to_string(&gi_path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == rel_path) {
        return Ok(());
    }
    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(rel_path);
    out.push('\n');
    std::fs::write(&gi_path, out)
}

/// Count filesystem entries under `dir` (recursively), capped at `cap` — used
/// for the "N files" note in the delete-folder confirm modal. Returns
/// `(count, truncated)`; `truncated` is `true` when counting stopped early
/// (the real count may be higher — kept cheap for a large tree). A subdir
/// read error is silently skipped: this is a best-effort UI note, not a
/// safety check (the actual delete is one `rename` of the whole directory).
pub fn count_dir_entries_capped(dir: &Path, cap: usize) -> (usize, bool) {
    let mut count = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            count += 1;
            if count >= cap {
                return (count, true);
            }
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    (count, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_empty_and_dots() {
        assert!(validate_fs_name("").is_err());
        assert!(validate_fs_name("   ").is_err());
        assert!(validate_fs_name(".").is_err());
        assert!(validate_fs_name("..").is_err());
        assert!(validate_fs_name(".git").is_err());
    }

    #[test]
    fn validate_rejects_separators() {
        assert!(validate_fs_name("a/b").is_err());
        assert!(validate_fs_name("a\\b").is_err());
    }

    #[test]
    fn path_touches_git_dir_detects_top_level_only() {
        assert!(path_touches_git_dir(Path::new(".git")));
        assert!(path_touches_git_dir(Path::new(".git/HEAD")));
        assert!(!path_touches_git_dir(Path::new("src/.git-hooks/x")));
        assert!(!path_touches_git_dir(Path::new("src/main.rs")));
    }

    #[test]
    fn validate_accepts_plain_and_japanese_names() {
        assert!(validate_fs_name("main.rs").is_ok());
        assert!(validate_fs_name("日本語ファイル.txt").is_ok());
        assert!(validate_fs_name(".gitignore").is_ok());
    }

    #[test]
    fn trash_collision_name_keeps_extension() {
        assert_eq!(trash_collision_name("report.txt", 2), "report 2.txt");
        assert_eq!(
            trash_collision_name("archive.tar.gz", 2),
            "archive.tar 2.gz"
        );
    }

    #[test]
    fn trash_collision_name_handles_dotfiles_and_no_ext() {
        assert_eq!(trash_collision_name(".gitignore", 2), ".gitignore 2");
        assert_eq!(trash_collision_name("README", 3), "README 3");
    }

    #[test]
    fn gitignore_append_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("kagi-gitignore-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let gi = dir.join(".gitignore");
        let _ = std::fs::remove_file(&gi);

        add_gitignore_entry(&dir, "target/").unwrap();
        add_gitignore_entry(&dir, "target/").unwrap();
        let content = std::fs::read_to_string(&gi).unwrap();
        assert_eq!(content.lines().filter(|l| *l == "target/").count(), 1);

        add_gitignore_entry(&dir, "*.log").unwrap();
        let content = std::fs::read_to_string(&gi).unwrap();
        assert_eq!(content.lines().count(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn count_dir_entries_capped_counts_recursively() {
        let dir = std::env::temp_dir().join(format!("kagi-count-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), b"x").unwrap();
        std::fs::write(dir.join("sub/b.txt"), b"x").unwrap();

        let (count, truncated) = count_dir_entries_capped(&dir, 100);
        assert_eq!(count, 3); // a.txt + sub/ + sub/b.txt
        assert!(!truncated);

        let (count, truncated) = count_dir_entries_capped(&dir, 2);
        assert_eq!(count, 2);
        assert!(truncated);

        std::fs::remove_dir_all(&dir).ok();
    }
}
