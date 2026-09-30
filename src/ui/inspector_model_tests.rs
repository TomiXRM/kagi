use std::path::PathBuf;

use gpui::SharedString;
use kagi_git::{ChangeKind, FileDiffStat, FileStatus};
use kagi_ui_core::file_tree::TreeRow;

use super::*;

fn file(path: &str, change: ChangeKind) -> FileStatus {
    FileStatus {
        path: PathBuf::from(path),
        change,
    }
}

fn stat(path: &str, additions: usize) -> FileDiffStat {
    FileDiffStat {
        path: PathBuf::from(path),
        change: ChangeKind::Modified,
        additions,
        deletions: 0,
        is_binary: false,
    }
}

/// `(file_index, name)` of every file row in tree order.
fn tree_files(files: &InspectorFiles) -> Vec<(usize, String)> {
    files
        .tree
        .iter()
        .filter_map(|row| match row {
            TreeRow::File {
                file_index, name, ..
            } => Some((*file_index, name.to_string())),
            TreeRow::Dir { .. } => None,
        })
        .collect()
}

/// Past the cap only the first MAX_FILES are shown, each keeping the index of
/// the source list the click / menu / copy handlers resolve against; the
/// tally still counts every file.
#[test]
fn truncation_keeps_source_indices_and_counts_everything() {
    let mut list: Vec<FileStatus> = (0..MAX_FILES)
        .map(|i| file(&format!("src/f{i:03}.rs"), ChangeKind::Modified))
        .collect();
    list.extend((0..5).map(|i| file(&format!("new/n{i}.rs"), ChangeKind::Added)));

    let derived = InspectorFiles::derive(&list, None, None);

    assert_eq!(derived.files.len(), MAX_FILES);
    assert_eq!(derived.hidden, 5);
    for (fi, name) in tree_files(&derived) {
        assert_eq!(list[fi].path, PathBuf::from("src").join(&name));
    }
    assert_eq!(tree_files(&derived).len(), MAX_FILES);
    assert_eq!(
        derived.counts,
        ChangeCounts {
            modified: MAX_FILES,
            added: 5,
            ..ChangeCounts::default()
        }
    );
}

/// Generated files leave the tree and are flagged for the fold; flags shorter
/// than the shown list fold nothing rather than misaligning.
#[test]
fn generated_files_fold_out_of_the_tree() {
    let list = vec![
        file("Cargo.lock", ChangeKind::Modified),
        file("src/main.rs", ChangeKind::Modified),
    ];

    let folded = InspectorFiles::derive(&list, None, Some(&[true, false]));
    assert_eq!(folded.generated_count, 1);
    assert!(folded.files[0].generated && !folded.files[1].generated);
    assert_eq!(tree_files(&folded), vec![(1, "main.rs".to_string())]);

    let short = InspectorFiles::derive(&list, None, Some(&[true]));
    assert_eq!(short.generated_count, 0);
    assert_eq!(tree_files(&short).len(), 2);
}

/// Diffstat is matched by path, not position.
#[test]
fn diffstat_follows_the_path() {
    let list = vec![
        file("a.rs", ChangeKind::Modified),
        file("b.rs", ChangeKind::Modified),
    ];
    let stats = [stat("b.rs", 7), stat("a.rs", 3)];

    let derived = InspectorFiles::derive(&list, Some(&stats), None);

    let additions: Vec<_> = derived
        .files
        .iter()
        .map(|f| f.stat.as_ref().map(|s| s.additions))
        .collect();
    assert_eq!(additions, vec![Some(3), Some(7)]);
}

/// The files slot re-derives when a load lands or the cache is cleared, and
/// not otherwise; the message slot re-converts only for another commit.
#[test]
fn slots_rederive_only_when_their_key_changes() {
    let mut ui = TabUiState::default();
    let before = derivations();

    ui.sync_inspector_commit_files(4);
    assert!(ui.inspector_model.files().is_none(), "nothing loaded yet");

    ui.diff_caches
        .insert_row(4, Some(vec![file("a.rs", ChangeKind::Added)]), None, None);
    for _ in 0..3 {
        ui.sync_inspector_commit_files(4);
    }
    assert_eq!(derivations().0 - before.0, 1, "one load, one derivation");
    assert_eq!(&*ui.inspector_model.files().unwrap().files[0].path, "a.rs");

    // Reload: the cache is cleared and the row reloads with new content.
    ui.diff_caches.clear();
    ui.diff_caches
        .insert_row(4, Some(vec![file("b.rs", ChangeKind::Deleted)]), None, None);
    ui.sync_inspector_commit_files(4);
    assert_eq!(&*ui.inspector_model.files().unwrap().files[0].path, "b.rs");

    let sha = SharedString::from("0123abcd");
    for _ in 0..3 {
        ui.inspector_model.sync_message(&sha, "subject\n\nbody <b>");
    }
    assert_eq!(derivations().1 - before.1, 1);
    assert!(ui.inspector_model.message_html().contains("&lt;b&gt;"));
    ui.inspector_model
        .sync_message(&SharedString::from("fedc9876"), "other");
    assert_eq!(derivations().1 - before.1, 2);
}
