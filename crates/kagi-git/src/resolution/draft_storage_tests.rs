use super::*;
use draft_storage::{read, sibling};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

const MARKERS: &str = "<<<<<<< Current\na\n=======\nb\n>>>>>>> Incoming\ncontext\n<<<<<<< Current\nc\n=======\nd\n>>>>>>> Incoming\n";
const WRITER_BODIES: [&str; 2] = ["first Result 日本語  \n\nA\n", "second Result 日本語\nB\n"];

fn live(repo: &Path) -> ResolutionBuffer {
    let mut buffer = ResolutionBuffer::new(repo);
    buffer.live_context = Some((
        ConflictRevision::from_fingerprint("c".repeat(64)),
        repo.join(".git"),
    ));
    for path in ["a.txt", "b.txt"] {
        let mut file = FileResolution::empty();
        file.identity = Some(ConflictRevision::from_fingerprint("a".repeat(64)));
        buffer.files.insert(path.into(), file);
        assert!(buffer.ensure_hunks(Path::new(path), MARKERS, 7));
    }
    buffer
}

fn restored(repo: &Path, path: &Path) -> ResolutionBuffer {
    let mut buffer = live(repo);
    buffer.retain_drafts(read(repo, path).unwrap().unwrap().buffer);
    for file in ["a.txt", "b.txt"] {
        assert!(buffer.ensure_hunks(Path::new(file), MARKERS, 7));
    }
    buffer
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn writer_child() {
    let Some(root) = std::env::var_os("KAGI_DRAFT_ATOMIC_CHILD") else {
        return;
    };
    let root = PathBuf::from(root);
    let index: usize = std::env::var("KAGI_DRAFT_WRITER").unwrap().parse().unwrap();
    let same_file = std::env::var_os("KAGI_DRAFT_SAME_FILE").is_some();
    let store = root.join("buffer.json");
    let mut buffer = restored(&root, &store);
    let file = if same_file || index == 0 {
        "a.txt"
    } else {
        "b.txt"
    };
    let hunk = if same_file { index } else { 0 };
    assert!(buffer.apply_hunk_choice(Path::new(file), hunk, HunkChoice::AcceptIncoming));
    std::fs::write(root.join(format!("ready-{index}")), b"ready").unwrap();
    wait_for(&root.join("go"));
    buffer.save_draft_document(&store).unwrap();
    std::fs::write(root.join(format!("saved-{index}")), b"saved").unwrap();
}

fn spawn_writer(root: &Path, index: usize, same_file: bool) -> Child {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "resolution::draft_storage_tests::writer_child",
            "--nocapture",
        ])
        .env("KAGI_DRAFT_ATOMIC_CHILD", root)
        .env("KAGI_DRAFT_WRITER", index.to_string());
    if same_file {
        command.env("KAGI_DRAFT_SAME_FILE", "1");
    }
    command.spawn().unwrap()
}

fn two_writers(same_file: bool) {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("buffer.json");
    live(root.path()).save_draft_document(&store).unwrap();
    let lock = draft_storage::lock(&store).unwrap();
    let mut a = spawn_writer(root.path(), 0, same_file);
    let mut b = spawn_writer(root.path(), 1, same_file);
    wait_for(&root.path().join("ready-0"));
    wait_for(&root.path().join("ready-1"));
    std::fs::write(root.path().join("go"), b"go").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        !root.path().join("saved-0").exists(),
        "writer bypassed interprocess lock"
    );
    assert!(
        !root.path().join("saved-1").exists(),
        "writer bypassed interprocess lock"
    );
    drop(lock);
    assert!(a.wait().unwrap().success());
    assert!(b.wait().unwrap().success());
    let buffer = restored(root.path(), &store);
    let a = buffer.hunk_model(Path::new("a.txt")).unwrap();
    assert_eq!(
        a.hunks()[0].choice,
        HunkChoice::AcceptIncoming,
        "first writer's acknowledged decision lost"
    );
    let other = if same_file {
        a
    } else {
        buffer.hunk_model(Path::new("b.txt")).unwrap()
    };
    assert_eq!(
        other.hunks()[usize::from(same_file)].choice,
        HunkChoice::AcceptIncoming,
        "second writer's acknowledged decision lost"
    );
    // Repeated stale saves must not roll the merged document back.
    let stale = live(root.path());
    stale.save_draft_document(&store).unwrap();
    assert_eq!(
        restored(root.path(), &store).resolved_text(Path::new("a.txt")),
        buffer.resolved_text(Path::new("a.txt"))
    );
}
#[test]
fn concurrent_processes_preserve_distinct_file_decisions() {
    two_writers(false);
}

#[test]
fn concurrent_processes_preserve_same_file_independent_hunks() {
    two_writers(true);
}

#[test]
fn recovery_writer_child() {
    let Some(root) = std::env::var_os("KAGI_DRAFT_RECOVERY_CHILD") else {
        return;
    };
    let root = PathBuf::from(root);
    let index: usize = std::env::var("KAGI_DRAFT_WRITER").unwrap().parse().unwrap();
    let first: usize = std::env::var("KAGI_DRAFT_RECOVERY_FIRST")
        .unwrap()
        .parse()
        .unwrap();
    let queued = std::env::var("KAGI_DRAFT_RECOVERY_QUEUED").unwrap() == "true";
    let store = buffer_file_path(&root).unwrap();
    let mut buffer = restored(&root, &store);
    buffer
        .set_manual_text(Path::new("a.txt"), WRITER_BODIES[index])
        .unwrap();
    let save = |buffer: &ResolutionBuffer| {
        if queued {
            let ticket = buffer.queue_autosave().unwrap();
            buffer.flush().unwrap();
            assert_eq!(ticket.wait().unwrap(), store);
        } else {
            assert_eq!(buffer.autosave().unwrap(), store);
        }
        assert!(!buffer.has_unflushed_drafts());
    };
    std::fs::write(root.join(format!("recovery-ready-{index}")), b"ready").unwrap();
    wait_for(&root.join(format!("recovery-go-{index}")));
    save(&buffer);
    assert!(!buffer.files[Path::new("a.txt")].pending_edit.is_pending());
    std::fs::write(root.join(format!("recovery-saved-{index}")), b"saved").unwrap();
    if index == first {
        for pass in 0..3 {
            wait_for(&root.join(format!("recovery-again-{pass}")));
            buffer
                .set_manual_text(Path::new("b.txt"), &format!("other file edit {pass}\n"))
                .unwrap();
            save(&buffer);
            // The writer still owns its original, unmerged live target body.
            assert_eq!(
                buffer.resolved_text(Path::new("a.txt")).unwrap().as_bytes(),
                WRITER_BODIES[first].as_bytes()
            );
            std::fs::write(root.join(format!("recovery-finished-{pass}")), b"saved").unwrap();
        }
    }
}

#[test]
fn two_writer_recovery_survives_other_file_autosaves() {
    isolated_async_test(
        "two_writer_recovery_survives_other_file_autosaves",
        |base| {
            for queued in [false, true] {
                for first in [0, 1] {
                    let root = tempfile::tempdir_in(base).unwrap();
                    let root = root.path();
                    let store = live(root).autosave().unwrap();
                    let mut children = [0, 1].map(|index| {
                        Command::new(std::env::current_exe().unwrap())
                            .args([
                                "--exact",
                                "resolution::draft_storage_tests::recovery_writer_child",
                                "--nocapture",
                            ])
                            .env("KAGI_DRAFT_RECOVERY_CHILD", root)
                            .env("KAGI_DRAFT_WRITER", index.to_string())
                            .env("KAGI_DRAFT_RECOVERY_FIRST", first.to_string())
                            .env("KAGI_DRAFT_RECOVERY_QUEUED", queued.to_string())
                            .spawn()
                            .unwrap()
                    });
                    for index in [0, 1] {
                        wait_for(&root.join(format!("recovery-ready-{index}")));
                    }
                    for index in [first, 1 - first] {
                        std::fs::write(root.join(format!("recovery-go-{index}")), b"go").unwrap();
                        wait_for(&root.join(format!("recovery-saved-{index}")));
                    }
                    // Rotate .prev more than once: keeping the lost body only in an
                    // older backup must not make this regression pass.
                    for pass in 0..3 {
                        std::fs::write(root.join(format!("recovery-again-{pass}")), b"go").unwrap();
                        wait_for(&root.join(format!("recovery-finished-{pass}")));
                        for document in [&store, &sibling(&store, ".prev")] {
                            let loaded = restored(root, document);
                            let target = Path::new("a.txt");
                            assert_eq!(
                                loaded.resolved_text(target).unwrap().as_bytes(),
                                WRITER_BODIES[1 - first].as_bytes()
                            );
                            let identity = loaded.file_identity(target).unwrap().clone();
                            assert_eq!(
                                loaded
                                    .recovery_drafts()
                                    .filter(|draft| draft.path == target)
                                    .count(),
                                1
                            );
                            // Consume the exact API that supplies the read-only
                            // recovery inspector; no fallback to .prev is involved.
                            let inspected = loaded
                                .into_recovery_drafts()
                                .find(|draft| draft.path == target)
                                .unwrap();
                            assert_eq!(inspected.identity, Some(identity));
                            assert_eq!(
                                lines_to_text(inspected.result.as_deref().unwrap()).as_bytes(),
                                WRITER_BODIES[first].as_bytes()
                            );
                        }
                    }
                    for child in &mut children {
                        assert!(child.wait().unwrap().success());
                    }
                }
            }
        },
    );
}

#[test]
fn interrupted_temp_write_leaves_the_last_good_document_readable() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("buffer.json");
    let mut buffer = live(root.path());
    buffer.apply_hunk_choice(Path::new("a.txt"), 0, HunkChoice::AcceptIncoming);
    buffer.save_draft_document(&store).unwrap();
    let before = std::fs::read(&store).unwrap();
    let temp = root
        .path()
        .join(format!(".buffer.json.{}.tmp", std::process::id()));
    std::fs::write(&temp, b"{\"files\":[").unwrap();
    assert_eq!(
        restored(root.path(), &store).resolved_text(Path::new("a.txt")),
        buffer.resolved_text(Path::new("a.txt"))
    );
    assert_eq!(std::fs::read(&store).unwrap(), before);
    // Also recover a torn primary left by older Kagi versions. The verified
    // .prev must not be overwritten with the corrupt primary during recovery.
    std::fs::write(&store, b"{\"files\":[").unwrap();
    assert_eq!(
        restored(root.path(), &store).resolved_text(Path::new("a.txt")),
        buffer.resolved_text(Path::new("a.txt"))
    );
    buffer.save_draft_document(&store).unwrap();
    assert!(read(root.path(), &store).unwrap().is_some());
    assert_eq!(std::fs::read(sibling(&store, ".prev")).unwrap(), before);
    assert!(
        !temp.exists(),
        "successful replacement cleans an abandoned same-pid temp"
    );
}

#[test]
fn corrupt_document_without_good_backup_refuses_overwrite() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("buffer.json");
    let torn = b"{\"files\":[";
    std::fs::write(&store, torn).unwrap();
    assert!(read(root.path(), &store).is_err());
    assert!(live(root.path()).save_draft_document(&store).is_err());
    assert_eq!(std::fs::read(&store).unwrap(), torn);
}

#[cfg(unix)]
#[test]
fn unreadable_primary_with_good_backup_refuses_restore_and_stale_autosave() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("buffer.json");
    let previous = sibling(&store, ".prev");
    let mut newest = live(root.path());
    newest.save_draft_document(&store).unwrap();
    let mut stale = restored(root.path(), &store);
    newest
        .set_manual_text(Path::new("a.txt"), "newer primary Result\n")
        .unwrap();
    newest.save_draft_document(&store).unwrap();
    stale
        .set_manual_text(Path::new("b.txt"), "stale writer Result\n")
        .unwrap();
    let primary_before = std::fs::read(&store).unwrap();
    let previous_before = std::fs::read(&previous).unwrap();
    assert_ne!(primary_before, previous_before);
    assert!(
        parse_buffer_json(root.path(), std::str::from_utf8(&previous_before).unwrap()).is_some()
    );

    let permissions = std::fs::metadata(&store).unwrap().permissions();
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o000)).unwrap();
    let denied = std::fs::read(&store);
    let read_error = read(root.path(), &store).err();
    let restore_error = draft_storage::read_for_restore(root.path(), &store).err();
    let save_error = stale.save_draft_document(&store).err();
    // Restore before asserting so fixture cleanup also works on a failed oracle.
    std::fs::set_permissions(&store, permissions).unwrap();

    let denied = denied.expect_err("requires a Unix user subject to file permission bits");
    assert_eq!(denied.kind(), std::io::ErrorKind::PermissionDenied);
    for error in [read_error, restore_error, save_error] {
        let error = error.expect("an unreadable primary must refuse backup fallback");
        let message = error.to_string();
        assert!(message.contains(&store.display().to_string()), "{message}");
        assert!(message.contains(&denied.to_string()), "{message}");
    }
    assert_eq!(std::fs::read(&store).unwrap(), primary_before);
    assert_eq!(std::fs::read(&previous).unwrap(), previous_before);
    assert!(!sibling(&store, ".corrupt").exists());
    assert!(!sibling(&previous, ".corrupt").exists());
}

#[test]
fn invalid_utf8_primary_restores_good_backup_without_quarantine() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("buffer.json");
    let mut buffer = live(root.path());
    buffer
        .set_manual_text(Path::new("a.txt"), "backup Result\n")
        .unwrap();
    buffer.save_draft_document(&store).unwrap();
    let previous = sibling(&store, ".prev");
    let previous_before = std::fs::read(&previous).unwrap();
    let corrupt = b"\xff";
    std::fs::write(&store, corrupt).unwrap();

    let (disk, warnings) = draft_storage::read_for_restore(root.path(), &store).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(
        disk.unwrap().buffer.resolved_text(Path::new("a.txt")),
        buffer.resolved_text(Path::new("a.txt"))
    );
    assert_eq!(std::fs::read(&store).unwrap(), corrupt);
    assert_eq!(std::fs::read(&previous).unwrap(), previous_before);
    assert!(!sibling(&store, ".corrupt").exists());
}

#[test]
fn failed_autosave_then_preservation_clone_keeps_the_newest_edit() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("buffer.json");
    let path = Path::new("a.txt");
    let mut buffer = live(root.path());
    buffer.save_draft_document(&store).unwrap();
    assert!(!buffer.has_unflushed_drafts());
    buffer
        .set_manual_text(path, "newest unsaved Result\n")
        .unwrap();
    let held = draft_storage::lock(&store).unwrap();
    let error = buffer.save_draft_document(&store).unwrap_err();
    assert!(error.to_string().contains("timed out after 2 s"));
    assert!(
        buffer.has_unflushed_drafts(),
        "failed synchronous saves still own live edits"
    );
    let preserved = buffer.clone();
    assert!(!preserved.files[path].pending_edit.is_pending());
    drop(held);
    preserved.save_draft_document(&store).unwrap();
    assert_eq!(
        restored(root.path(), &store).resolved_text(path),
        Some("newest unsaved Result\n".to_string()),
        "a preservation clone discarded the edit whose prior autosave failed"
    );
}

#[test]
fn same_hunk_collision_retains_both_complete_drafts() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("buffer.json");
    live(root.path()).save_draft_document(&store).unwrap();
    let mut a = restored(root.path(), &store);
    let mut b = restored(root.path(), &store);
    a.apply_hunk_choice(Path::new("a.txt"), 0, HunkChoice::AcceptIncoming);
    b.apply_hunk_choice(Path::new("a.txt"), 0, HunkChoice::AcceptCurrent);
    a.save_draft_document(&store).unwrap();
    b.save_draft_document(&store).unwrap();
    let saved = read(root.path(), &store).unwrap().unwrap().buffer;
    assert_eq!(
        saved.resolved_text(Path::new("a.txt")),
        b.resolved_text(Path::new("a.txt"))
    );
    assert!(saved
        .recovery_drafts()
        .any(|entry| entry.result.map(lines_to_text) == a.resolved_text(Path::new("a.txt"))));
}

#[test]
fn independent_hunk_merge_is_order_independent() {
    let mut documents = Vec::new();
    for reverse in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let store = root.path().join("buffer.json");
        live(root.path()).save_draft_document(&store).unwrap();
        let mut a = restored(root.path(), &store);
        let mut b = restored(root.path(), &store);
        let path = Path::new("a.txt");
        assert!(a.apply_hunk_choice(path, 0, HunkChoice::AcceptIncoming));
        assert!(b.set_hunk_line_selection(path, 1, SelectionSide::Current, 0, true));
        let (first, later) = if reverse { (&b, &a) } else { (&a, &b) };
        first.save_draft_document(&store).unwrap();
        let disk = read(root.path(), &store).unwrap().unwrap().buffer;
        let snapshot = later
            .disk_snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let merged = later.merge_draft_files(Some(&disk), &snapshot, true);
        assert!(
            !merged.matches_record(path, &later.files[path], &later.files[path]),
            "a recovery equal to the pre-merge editor is not the assembled record"
        );
        later.save_draft_document(&store).unwrap();
        let saved = read(root.path(), &store).unwrap().unwrap().buffer;
        assert!(merged.matches_record(path, &later.files[path], &saved.files[path]));
        let mut record = serde_json::to_value(file_record(path, &saved.files[path])).unwrap();
        // This is writer bookkeeping, not conflict content or a decision.
        record.as_object_mut().unwrap().remove("editing_origin");
        documents.push(record);
        // autosave(&self) cannot mutate the editor's live model. A subsequent
        // edit by the merging writer must retain the imported disk-only hunk.
        let own_hunk = if reverse { 0 } else { 1 };
        let mut expected = restored(root.path(), &store);
        expected.apply_hunk_choice(path, own_hunk, HunkChoice::BothCurrentFirst);
        let later = if reverse { &mut a } else { &mut b };
        later.apply_hunk_choice(path, own_hunk, HunkChoice::BothCurrentFirst);
        later.save_draft_document(&store).unwrap();
        assert_eq!(
            restored(root.path(), &store).resolved_text(path),
            expected.resolved_text(path),
            "a later edit erased a previously merged decision"
        );
    }
    assert_eq!(
        documents[0], documents[1],
        "lock ordering changed independent decisions"
    );
}

#[test]
fn lock_holder_child() {
    let Some(root) = std::env::var_os("KAGI_DRAFT_LOCK_CHILD") else {
        return;
    };
    let root = PathBuf::from(root);
    let _lock = draft_storage::lock(&root.join("buffer.json")).unwrap();
    std::fs::write(root.join("locked"), b"locked").unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

#[test]
fn killed_writer_releases_advisory_lock_without_sidecar_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("buffer.json");
    let mut holder = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "resolution::draft_storage_tests::lock_holder_child",
            "--nocapture",
        ])
        .env("KAGI_DRAFT_LOCK_CHILD", root.path())
        .spawn()
        .unwrap();
    wait_for(&root.path().join("locked"));
    let start = Instant::now();
    let error = live(root.path()).save_draft_document(&store).unwrap_err();
    assert!(error.to_string().contains("timed out after 2 s"));
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "unbounded lock wait"
    );
    // std Child::kill sends SIGKILL on Unix. The lock file intentionally stays:
    // no create/delete/PID cleanup may be necessary after an owner's death.
    holder.kill().unwrap();
    assert!(!holder.wait().unwrap().success());
    assert!(sibling(&store, ".lock").exists());
    live(root.path()).save_draft_document(&store).unwrap();
    assert!(read(root.path(), &store).unwrap().is_some());
}

#[test]
#[ignore = "machine-local autosave latency measurement; requires isolated KAGI_LOG_DIR"]
fn autosave_latency_three_files_five_hunks_200kb() {
    assert!(
        std::env::var_os("KAGI_LOG_DIR").is_some(),
        "use isolated KAGI_LOG_DIR"
    );
    let root = tempfile::tempdir().unwrap();
    let mut buffer = ResolutionBuffer::new(root.path());
    let result = "a realistic resolved line with enough content to represent a source file and preserve its text\n".repeat(735);
    let mut result_bytes = 0;
    for (index, hunk_count) in [2, 2, 1].into_iter().enumerate() {
        let path = PathBuf::from(format!("source-{index}.txt"));
        buffer.files.insert(path.clone(), FileResolution::empty());
        let markers =
            "<<<<<<< Current\ncurrent\n=======\nincoming\n>>>>>>> Incoming\nunchanged context\n"
                .repeat(hunk_count);
        assert!(buffer.ensure_hunks(&path, &markers, 7));
        assert_eq!(buffer.hunk_model(&path).unwrap().hunks().len(), hunk_count);
        buffer.set_manual_text(&path, &result).unwrap();
        result_bytes += result.len();
    }
    buffer.autosave().unwrap();
    let mut milliseconds = [0.0_f64; 100];
    for elapsed in &mut milliseconds {
        let start = Instant::now();
        buffer.autosave().unwrap();
        *elapsed = start.elapsed().as_secs_f64() * 1000.0;
    }
    milliseconds.sort_by(f64::total_cmp);
    eprintln!(
        "AUTOSAVE_LATENCY files=3 hunks=5 result_bytes={result_bytes} n=100 p50_ms={:.3} p95_ms={:.3} min_ms={:.3} max_ms={:.3}",
        milliseconds[49], milliseconds[94], milliseconds[0], milliseconds[99]
    );
    let mut capture_ms = [0.0_f64; 100];
    for elapsed in &mut capture_ms {
        let start = Instant::now();
        buffer.queue_autosave().unwrap();
        *elapsed = start.elapsed().as_secs_f64() * 1000.0;
    }
    buffer.flush().unwrap();
    capture_ms.sort_by(f64::total_cmp);
    eprintln!(
        "AUTOSAVE_CAPTURE_LATENCY files=3 hunks=5 result_bytes={result_bytes} n=100 p50_ms={:.3} p95_ms={:.3} min_ms={:.3} max_ms={:.3}",
        capture_ms[49], capture_ms[94], capture_ms[0], capture_ms[99]
    );
}

fn isolated_async_test(name: &str, run: impl FnOnce(&Path)) {
    if let Some(root) = std::env::var_os("KAGI_DRAFT_ASYNC_TEST_ROOT") {
        run(Path::new(&root));
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("resolution::draft_storage_tests::{name}"),
            "--nocapture",
        ])
        .env("KAGI_DRAFT_ASYNC_TEST_ROOT", root.path())
        .env("KAGI_LOG_DIR", root.path().join("logs"))
        .status()
        .unwrap();
    assert!(
        status.success(),
        "{name} failed in its isolated draft store"
    );
}

#[test]
fn queued_latest_snapshot_and_clone_flush_share_one_generation() {
    isolated_async_test(
        "queued_latest_snapshot_and_clone_flush_share_one_generation",
        |root| {
            let mut buffer = live(root);
            let operation = buffer.clone();
            let file = Path::new("a.txt");
            assert!(buffer.apply_hunk_choice(file, 0, HunkChoice::AcceptIncoming));
            let first = buffer.queue_autosave().unwrap();
            assert!(buffer.apply_hunk_choice(file, 1, HunkChoice::AcceptIncoming));
            let last = buffer.queue_autosave().unwrap();
            assert_eq!(last.generation(), first.generation() + 1);
            assert!(!first.is_current());
            assert!(last.is_current());
            assert!(operation.has_unflushed_drafts());
            operation.flush().unwrap();
            let path = last.wait().unwrap();
            assert_eq!(first.wait().unwrap(), path);
            assert!(!buffer.has_unflushed_drafts());
            assert!(!buffer.files[file].pending_edit.is_pending());
            let loaded = restored(root, &path);
            assert_eq!(
                loaded.hunk_model(file).unwrap().hunks()[0].choice,
                HunkChoice::AcceptIncoming
            );
            assert_eq!(
                loaded.hunk_model(file).unwrap().hunks()[1].choice,
                HunkChoice::AcceptIncoming
            );
            // A stale operation clone drains the same queue and cannot revive its
            // old unresolved body after the latest acknowledged edit.
            operation.autosave().unwrap();
            let disk = restored(root, &path);
            assert_eq!(disk.resolved_text(file), buffer.resolved_text(file));
        },
    );
}

#[test]
fn queued_failure_stays_unflushed_until_a_successful_retry() {
    isolated_async_test(
        "queued_failure_stays_unflushed_until_a_successful_retry",
        |root| {
            let mut buffer = live(root);
            let stale_operation = buffer.clone();
            let file = Path::new("a.txt");
            buffer
                .set_manual_text(file, "newest manual body\n")
                .unwrap();
            let path = buffer_file_path(root).unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let lock = draft_storage::lock(&path).unwrap();
            let failed = buffer.queue_autosave().unwrap();
            let error = failed.flush().unwrap_err().to_string();
            assert!(error.contains("lock"), "{error}");
            assert!(failed.wait().is_err());
            assert!(buffer.has_unflushed_drafts());
            assert!(buffer.files[file].pending_edit.is_pending());
            drop(lock);
            assert!(
                stale_operation.autosave().is_err(),
                "an old operation clone must not acknowledge the failed live edit"
            );
            assert!(!path.exists());
            assert!(
                buffer.flush().is_err(),
                "failure is retained, not mistaken for no pending write"
            );
            let retry = buffer.queue_autosave().unwrap();
            retry.flush().unwrap();
            retry.wait().unwrap();
            assert!(!buffer.has_unflushed_drafts());
            assert!(!buffer.files[file].pending_edit.is_pending());
            let disk = restored(root, &path);
            assert_eq!(disk.resolved_text(file), buffer.resolved_text(file));
        },
    );
}

#[test]
fn older_durable_completion_does_not_acknowledge_a_newer_memory_edit() {
    isolated_async_test(
        "older_durable_completion_does_not_acknowledge_a_newer_memory_edit",
        |root| {
            let mut buffer = live(root);
            let file = Path::new("a.txt");
            buffer
                .set_manual_text(file, "first captured body\n")
                .unwrap();
            let first = buffer.queue_autosave().unwrap();
            buffer.set_manual_text(file, "newer memory body\n").unwrap();
            let path = first.wait().unwrap();
            assert!(buffer.files[file].pending_edit.is_pending());
            assert_ne!(
                restored(root, &path).resolved_text(file),
                buffer.resolved_text(file)
            );
            let last = buffer.queue_autosave().unwrap();
            buffer.flush().unwrap();
            assert!(!first.is_current());
            assert!(last.is_current());
            assert!(!buffer.files[file].pending_edit.is_pending());
            assert_eq!(
                restored(root, &path).resolved_text(file),
                buffer.resolved_text(file)
            );
        },
    );
}

#[test]
fn queued_autosave_restore_retains_worktree_fingerprint() {
    isolated_async_test(
        "queued_autosave_restore_retains_worktree_fingerprint",
        |root| {
            let mut buffer = live(root);
            let path = Path::new("a.txt");
            std::fs::write(root.join(path), MARKERS).unwrap();
            let fingerprint = worktree_state::fingerprint(&root.join(path)).unwrap();
            buffer.files.get_mut(path).unwrap().worktree_fingerprint = Some(fingerprint.clone());
            buffer.files.get_mut(path).unwrap().worktree_changed = true;
            buffer.set_manual_text(path, "captured Result\n").unwrap();
            let saved = buffer.queue_autosave().unwrap();
            buffer.flush().unwrap();
            let restored = restored(root, &saved.wait().unwrap());
            assert_eq!(
                restored.worktree_fingerprint(path),
                Some(fingerprint.as_str())
            );
            assert!(restored.worktree_changed(path));
            assert_eq!(restored.resolved_text(path), buffer.resolved_text(path));
        },
    );
}

#[test]
fn queued_hunk_recovery_remains_inspection_only_after_reload() {
    isolated_async_test(
        "queued_hunk_recovery_remains_inspection_only_after_reload",
        |root| {
            let mut buffer = live(root);
            let file = Path::new("a.txt");
            assert!(buffer.apply_hunk_choice(file, 0, HunkChoice::AcceptIncoming));
            let body = buffer.resolved_text(file).unwrap();
            buffer.autosave().unwrap();
            // A changed materialization invalidates old hunk authority and moves
            // the complete body into D1's explicitly non-readoptable recovery.
            buffer.hunks.remove(file);
            assert!(buffer.ensure_hunks(file, &MARKERS.replace("a\n", "changed\n"), 7));
            assert!(!buffer.has_resolution(file));
            assert_eq!(buffer.recovery_drafts().count(), 1);
            let saved = buffer.queue_autosave().unwrap();
            buffer.flush().unwrap();
            let path = saved.wait().unwrap();
            let decoded = read(root, &path).unwrap().unwrap().buffer;
            assert!(decoded.recoveries[0].file.recovery_only);
            // Even returning to the original markers/identity cannot promote
            // this recovery after it crosses the queued persistence boundary.
            let loaded = restored(root, &path);
            assert!(!loaded.has_resolution(file));
            let recoveries: Vec<_> = loaded.recovery_drafts().collect();
            assert_eq!(recoveries.len(), 1);
            assert_eq!(recoveries[0].path, file);
            assert_eq!(
                kagi_domain::resolution::lines_to_text(recoveries[0].result.unwrap()),
                body
            );
        },
    );
}
