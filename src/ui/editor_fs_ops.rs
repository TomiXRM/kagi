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

/// `true` if `a` and `b` refer to the same filesystem object (same device +
/// inode on Unix, same volume + file index on Windows). Used by the Rename
/// path to permit a case-only rename (`File.txt` → `file.txt`) on a
/// case-insensitive filesystem (macOS APFS default), where the new name
/// already "exists" as the same inode as the old — `Path::canonicalize` is
/// not used because its returned casing is inconsistent on case-insensitive
/// volumes. Returns `false` if either path can't be stat'd. Stat'd via
/// `symlink_metadata` (NOT `metadata`, which follows symlinks): a symlink
/// pointing at the source is NOT treated as same-file — only a literal
/// same-directory-entry match (the case-only rename) is, so renaming onto a
/// symlink to the source is rejected rather than replacing the symlink.
pub fn same_file(a: &Path, b: &Path) -> bool {
    let (Ok(am), Ok(bm)) = (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        am.dev() == bm.dev() && am.ino() == bm.ino()
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        am.volume_serial_number() == bm.volume_serial_number() && am.file_index() == bm.file_index()
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

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
/// (CLAUDE.md invariant #3). Compiles on every OS so call sites need no cfg
/// gating (a cfg'd-out version broke the Linux/Windows release builds —
/// v0.8.0 tag); on non-macOS it always returns `Err`, and the Delete menu
/// item is hidden anyway (`TRASH_SUPPORTED`).
pub fn trash_path(full_path: &Path) -> Result<PathBuf, String> {
    if !TRASH_SUPPORTED {
        return Err("Trash is only supported on macOS".to_string());
    }
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

/// Escape `rel` (repo-relative, forward-slash) into a `.gitignore` pattern
/// that matches the literal path verbatim: anchor with a leading `/`, backslash-
/// escape `*` `?` `[` `]` anywhere and `#` `!` at the path start (gitignore's
/// line-start comment / negation metacharacters), and backslash-escape trailing
/// spaces (gitignore trims them otherwise). Pure; unit-tested below.
fn gitignore_escape(rel: &str) -> String {
    let mut s = String::with_capacity(rel.len() + 2);
    s.push('/');
    for (i, ch) in rel.chars().enumerate() {
        match ch {
            '*' | '?' | '[' | ']' => {
                s.push('\\');
                s.push(ch);
            }
            '#' | '!' if i == 0 => {
                s.push('\\');
                s.push(ch);
            }
            _ => s.push(ch),
        }
    }
    // Backslash-escape a run of trailing spaces so gitignore doesn't trim them.
    let trailing = s.len() - s.trim_end_matches(' ').len();
    if trailing > 0 {
        let cut = s.len() - trailing;
        s.truncate(cut);
        for _ in 0..trailing {
            s.push_str("\\ ");
        }
    }
    s
}

/// Append `rel_path` (repo-relative, forward-slash) as a new line in
/// `<repo_path>/.gitignore`, creating the file if it doesn't exist yet.
/// Idempotent: a line already present is left alone rather than duplicated —
/// matched against the escaped/anchored form, the raw (pre-escaping) form, or
/// the unanchored form, comparing without trimming (so an escaped trailing
/// space dedups correctly). If the file exists but
/// can't be read (unreadable / non-UTF-8), the error is returned rather than
/// treated as empty — so existing content is never destroyed by an overwrite.
/// Pure I/O; unit-tested below.
pub fn add_gitignore_entry(repo_path: &Path, rel_path: &str) -> std::io::Result<()> {
    let gi_path = repo_path.join(".gitignore");
    let existing = match std::fs::read_to_string(&gi_path) {
        Ok(s) => s,
        // A genuinely missing file is the "create it" path; any other read
        // failure (unreadable, non-UTF-8) must surface, not be overwritten.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let entry = gitignore_escape(rel_path);
    // Also treat the raw (pre-escaping) and unanchored forms as duplicates:
    // a .gitignore written before escaping/anchoring existed has e.g.
    // `target/` / `*.log` unescaped, and re-adding the same path must not
    // append the anchored escaped form (`/target/`, `/\*.log`) beside it.
    // Compare with only `\r` trimmed — NOT `.trim()`, which would strip an
    // escaped trailing space (`/name\ `) and re-add the line as a dupe.
    let unanchored = entry.strip_prefix('/').unwrap_or(&entry[..]);
    let is_dupe = |l: &str| {
        let l = l.trim_end_matches('\r');
        l == entry.as_str() || l == rel_path || l == unanchored
    };
    if existing.lines().any(is_dupe) {
        return Ok(());
    }
    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&entry);
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
        // Entries are now anchored with a leading `/`.
        assert_eq!(content.lines().filter(|l| *l == "/target/").count(), 1);

        add_gitignore_entry(&dir, "*.log").unwrap();
        let content = std::fs::read_to_string(&gi).unwrap();
        assert_eq!(content.lines().count(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn gitignore_entry_escapes_metachars_and_anchors() {
        let dir = std::env::temp_dir().join(format!("kagi-gitignore-esc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let gi = dir.join(".gitignore");
        let _ = std::fs::remove_file(&gi);

        add_gitignore_entry(&dir, "src/[v1]*.go").unwrap();
        add_gitignore_entry(&dir, "#notes.txt").unwrap();
        add_gitignore_entry(&dir, "!keep").unwrap();
        add_gitignore_entry(&dir, "trailing space ").unwrap();
        let content = std::fs::read_to_string(&gi).unwrap();
        assert!(content.lines().any(|l| l == "/src/\\[v1\\]\\*.go"));
        assert!(content.lines().any(|l| l == "/\\#notes.txt"));
        assert!(content.lines().any(|l| l == "/\\!keep"));
        assert!(content.lines().any(|l| l == "/trailing space\\ "));

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

    #[test]
    fn gitignore_trailing_space_readd_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("kagi-gitignore-ts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let gi = dir.join(".gitignore");
        let _ = std::fs::remove_file(&gi);

        add_gitignore_entry(&dir, "trailing space ").unwrap();
        add_gitignore_entry(&dir, "trailing space ").unwrap();
        let content = std::fs::read_to_string(&gi).unwrap();
        // The escaped form keeps the trailing space as `\ `; re-adding must
        // NOT duplicate the line (the old `l.trim()` comparison stripped it).
        assert_eq!(
            content
                .lines()
                .filter(|l| *l == "/trailing space\\ ")
                .count(),
            1
        );
        assert_eq!(content.lines().count(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn gitignore_raw_preexisting_entry_not_duplicated() {
        let dir = std::env::temp_dir().join(format!("kagi-gitignore-raw-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let gi = dir.join(".gitignore");
        let _ = std::fs::remove_file(&gi);

        // Simulate a .gitignore written before escaping/anchoring existed:
        // raw, unanchored lines for the same paths.
        std::fs::write(&gi, "target/\n*.log\n").unwrap();

        add_gitignore_entry(&dir, "target/").unwrap();
        add_gitignore_entry(&dir, "*.log").unwrap();
        let content = std::fs::read_to_string(&gi).unwrap();
        // Neither raw line is duplicated by the anchored escaped form...
        assert_eq!(content.lines().filter(|l| *l == "target/").count(), 1);
        assert_eq!(content.lines().filter(|l| *l == "*.log").count(), 1);
        // ...and no escaped siblings were appended.
        assert_eq!(content.lines().filter(|l| *l == "/target/").count(), 0);
        assert_eq!(content.lines().filter(|l| *l == "/\\*.log").count(), 0);
        assert_eq!(content.lines().count(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn same_file_rejects_symlink_to_source() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("kagi-samefile-sym-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("real.txt");
        let link = dir.join("link.txt");
        std::fs::write(&real, b"x").unwrap();
        let _ = std::fs::remove_file(&link);
        symlink(&real, &link).unwrap();

        // `link` is a symlink pointing AT `real`; renaming real→link must NOT
        // be treated as a same-file (case-only) rename — that would replace
        // the symlink. They are different directory entries.
        assert!(!same_file(&real, &link));
        assert!(!same_file(&link, &real));
        // Sanity: a path is still the same file as itself.
        assert!(same_file(&real, &real));

        std::fs::remove_dir_all(&dir).ok();
    }
}
