//! Occupancy traversal tests (#633): the arithmetic `du` agrees with, and the
//! failure modes that must never become a smaller number.

use super::*;
use std::path::Path;

/// Root holding every case the walk must get right: a hardlink pair, a symlink
/// pointing at a large tree outside the root, and nested `target` directories.
#[cfg(unix)]
fn mixed_tree() -> (tempfile::TempDir, std::path::PathBuf) {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("root");
    let outside = base.path().join("outside");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&outside).unwrap();
    // 256 KiB outside the root: following the symlink would add all of it.
    std::fs::write(outside.join("huge.bin"), vec![7u8; 256 * 1024]).unwrap();

    std::fs::write(root.join("tracked.txt"), vec![1u8; 8 * 1024]).unwrap();
    std::fs::hard_link(root.join("tracked.txt"), root.join("alias.txt")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();

    let target = root.join("target");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("build.bin"), vec![2u8; 128 * 1024]).unwrap();
    let nested = target.join("debug").join("target");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("deep.bin"), vec![3u8; 16 * 1024]).unwrap();

    (base, root)
}

#[cfg(unix)]
fn allocated(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(path).unwrap().blocks() * 512
}

#[cfg(unix)]
#[test]
fn hardlinks_count_once_symlinks_are_not_followed_and_target_is_split_out() {
    let (_base, root) = mixed_tree();
    let target = root.join("target");
    let nested = target.join("debug").join("target");

    let expected_target = allocated(&target)
        + allocated(&target.join("build.bin"))
        + allocated(&target.join("debug"))
        + allocated(&nested)
        + allocated(&nested.join("deep.bin"));
    // `alias.txt` shares an inode with `tracked.txt`, so its bytes are counted
    // once; `link` contributes only the symlink itself.
    let expected_total = allocated(&root)
        + allocated(&root.join("tracked.txt"))
        + allocated(&root.join("link"))
        + expected_target;

    let usage = scan(&root, &AtomicBool::new(false)).unwrap();
    assert_eq!(usage.target_bytes, expected_target);
    assert_eq!(usage.allocated_bytes, expected_total);
    assert!(usage.allocated_bytes >= 128 * 1024);
}

/// The whole reason ignores are not consulted: `du` and this walk must agree
/// on a tree full of ignored build output.
#[cfg(unix)]
#[test]
fn the_total_matches_du_on_the_same_tree() {
    let (_base, root) = mixed_tree();
    let du = std::process::Command::new("du")
        .args(["-sk", root.to_str().unwrap()])
        .output()
        .expect("run du");
    assert!(du.status.success());
    let kib: u64 = String::from_utf8_lossy(&du.stdout)
        .split_whitespace()
        .next()
        .expect("du prints a size")
        .parse()
        .expect("du prints a number");

    let usage = scan(&root, &AtomicBool::new(false)).unwrap();
    assert_eq!(usage.allocated_bytes, kib * 1024);
}

#[cfg(unix)]
#[test]
fn a_symlinked_root_is_refused_rather_than_measured_through() {
    let base = tempfile::tempdir().unwrap();
    let real = base.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = base.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let error = scan(&link, &AtomicBool::new(false)).expect_err("a symlinked root is refused");
    assert!(error.contains("symlink"), "{error}");
}

#[cfg(unix)]
#[test]
fn an_unreadable_directory_is_an_error_not_a_smaller_number() {
    use std::os::unix::fs::PermissionsExt;

    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("root");
    let locked = root.join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::write(locked.join("file.bin"), vec![9u8; 4096]).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    let scanned = scan(&root, &AtomicBool::new(false));
    // Restore first so the temp dir can always be cleaned up.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = scanned.expect_err("an unreadable subtree cannot produce a total");
    assert!(
        error.contains("locked"),
        "the error names the path: {error}"
    );
}

#[test]
fn a_cancelled_scan_reports_no_partial_total() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..64 {
        std::fs::write(dir.path().join(format!("f{i}")), b"x").unwrap();
    }
    assert_eq!(
        scan(dir.path(), &AtomicBool::new(true)).unwrap_err(),
        CANCELLED,
        "a cancelled scan must not report what it summed so far"
    );
}

#[test]
fn a_file_is_not_a_worktree_root() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("f");
    std::fs::write(&file, b"x").unwrap();
    scan(&file, &AtomicBool::new(false)).expect_err("a file is not a worktree");
}

/// What `disk_windows.rs` would have read out of `FILE_STANDARD_INFO` and
/// `FILE_COMPRESSION_INFO` for one entry. No Windows runtime is involved: the
/// FFI is what cannot run here, the decision about its answers is what matters.
fn win_file(allocation: i64, compressed: Result<i64, String>) -> Result<u64, String> {
    windows_physical_bytes(Path::new("C:/w/f.bin"), false, allocation, || compressed)
}

#[test]
fn a_compressed_file_occupies_its_physical_bytes_not_its_allocated_range() {
    // NTFS compression: a 256 KiB range backed by 64 KiB of clusters.
    assert_eq!(
        win_file(256 * 1024, Ok(64 * 1024)),
        Ok(64 * 1024),
        "AllocationSize is the range, not the storage behind it"
    );
}

#[test]
fn an_unanswerable_compression_query_is_an_error_not_the_allocation_size() {
    win_file(256 * 1024, Err("compression query failed".into()))
        .expect_err("an unavailable physical size must not fall back to AllocationSize");
}

#[test]
fn a_directory_is_measured_without_a_compression_query() {
    // Asking would answer zero for a directory stream, so it must not be asked.
    assert_eq!(
        windows_physical_bytes(Path::new("C:/w/sub"), true, 4096, || panic!(
            "a directory must not be asked for compressed size"
        )),
        Ok(4096)
    );
}

#[test]
fn a_negative_size_is_refused_rather_than_wrapped_into_a_huge_total() {
    win_file(256 * 1024, Ok(-1)).expect_err("-1 is not 16 exabytes");
    windows_physical_bytes(Path::new("C:/w/sub"), true, -4096, || unreachable!())
        .expect_err("a directory's allocation is checked too");
}

#[test]
fn a_compressed_size_above_the_allocation_is_refused_as_a_broken_answer() {
    win_file(4096, Ok(8192)).expect_err("the specification makes compressed <= allocation a MUST");
}
