use super::*;
use draft_storage::{read, sibling};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

const MARKERS: &str = "<<<<<<< Current\na\n=======\nb\n>>>>>>> Incoming\ncontext\n<<<<<<< Current\nc\n=======\nd\n>>>>>>> Incoming\n";

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
        assert!(buffer.ensure_hunks(Path::new(path), MARKERS));
    }
    buffer
}

fn restored(repo: &Path, path: &Path) -> ResolutionBuffer {
    let mut buffer = live(repo);
    buffer.retain_drafts(read(repo, path).unwrap().unwrap().buffer);
    for file in ["a.txt", "b.txt"] {
        assert!(buffer.ensure_hunks(Path::new(file), MARKERS));
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
        later.save_draft_document(&store).unwrap();
        let saved = read(root.path(), &store).unwrap().unwrap().buffer;
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
