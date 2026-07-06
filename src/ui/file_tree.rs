//! File tree builder — T018
//!
//! Converts a flat list of [`FileStatus`] entries into a depth-first, sorted,
//! single-child-compressed tree representation suitable for UI rendering.
//!
//! # Algorithm summary
//!
//! 1. Insert each file path into an in-memory n-ary tree keyed on path
//!    components.  Each directory node stores its child directory nodes and
//!    file leaf nodes separately.
//! 2. Flatten the tree depth-first, emitting **directory nodes before file
//!    nodes** at each level.  Within each category (dirs / files) entries are
//!    sorted by name (Unicode lexicographic, case-sensitive).
//! 3. **Single-child directory compression** (VSCode style): if a directory
//!    node has exactly one child and that child is itself a directory (no
//!    sibling files), the two names are merged into one `a/b` label and the
//!    merge is applied recursively.

use std::collections::BTreeMap;
use std::path::PathBuf;

use gpui::SharedString;

use kagi_git::{ChangeKind, FileStatus};

// ──────────────────────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────────────────────

/// A single row emitted by [`build_file_tree`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRow {
    /// A directory entry (not clickable in the MVP; no collapse).
    Dir {
        /// Nesting depth (0 = root level).
        depth: usize,
        /// Compressed display name, e.g. `"src/sub"`.
        name: SharedString,
    },
    /// A file entry.
    File {
        /// Nesting depth (0 = root level).
        depth: usize,
        /// Terminal file name (not the full path).
        name: SharedString,
        /// Index into the original `files` slice — passed to `open_file_diff`.
        file_index: usize,
        /// Change kind, forwarded from [`FileStatus`]. `None` for a file with
        /// no working-tree change (T-WS-EDITOR-004 `TreeSource::All` — an
        /// unmodified file shown in the full-worktree tree carries no badge).
        change: Option<ChangeKind>,
    },
}

/// Build a tree-row representation from `files`.
///
/// - Inserts all entries into an n-ary directory tree via
///   [`PathBuf::components()`] (no raw string splitting).
/// - Applies single-child directory compression.
/// - Flattens depth-first with directories before files, each group sorted
///   by name (Unicode lexicographic, case-sensitive).
/// - The `file_index` stored in each [`TreeRow::File`] matches the original
///   index in `files` exactly (consumers must pass an already-truncated slice
///   if they want truncation behaviour).
pub fn build_file_tree(files: &[FileStatus]) -> Vec<TreeRow> {
    let mut root = DirNode::default();

    for (idx, f) in files.iter().enumerate() {
        root.insert(&f.path, idx, Some(f.change.clone()));
    }

    let mut out = Vec::new();
    root.flatten_into(&mut out, 0);
    out
}

/// Like [`build_file_tree`], but for inputs that may have no change kind
/// (T-WS-EDITOR-004: the Editor Workspace's "All files" tree source, where an
/// unmodified file has nothing to badge). Shares the same `DirNode`
/// insert/flatten — no duplicated compression algorithm.
pub fn build_file_tree_opt(files: &[(PathBuf, Option<ChangeKind>)]) -> Vec<TreeRow> {
    let mut root = DirNode::default();

    for (idx, (path, change)) in files.iter().enumerate() {
        root.insert(path, idx, change.clone());
    }

    let mut out = Vec::new();
    root.flatten_into(&mut out, 0);
    out
}

// ──────────────────────────────────────────────────────────────
// Internal tree node types
// ──────────────────────────────────────────────────────────────

/// A file leaf stored inside a [`DirNode`].
#[derive(Debug)]
struct FileLeaf {
    name: String,
    file_index: usize,
    change: Option<ChangeKind>,
}

/// An n-ary directory node.
///
/// Uses [`BTreeMap`] for automatic name-sorted iteration.
#[derive(Debug, Default)]
struct DirNode {
    /// Child directory nodes, keyed by component name.
    dirs: BTreeMap<String, DirNode>,
    /// File leaves within this directory.
    files: Vec<FileLeaf>,
}

impl DirNode {
    /// Insert a file at the given path.
    ///
    /// All intermediate directory components are created on demand.
    fn insert(&mut self, path: &PathBuf, file_index: usize, change: Option<ChangeKind>) {
        use std::path::Component;

        let components: Vec<String> = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(os) => Some(os.to_string_lossy().into_owned()),
                _ => None, // skip prefix, root-dir, cur-dir, parent-dir
            })
            .collect();

        if components.is_empty() {
            return;
        }

        let file_name = components.last().unwrap().clone();

        // Walk/create all intermediate directory nodes.
        let mut node = self;
        let dir_components = &components[..components.len() - 1];
        for comp in dir_components {
            node = node.dirs.entry(comp.clone()).or_default();
        }

        // Insert the file leaf into the final directory.
        node.files.push(FileLeaf {
            name: file_name,
            file_index,
            change,
        });
    }

    /// Recursively flatten `self` into `out`, applying single-child
    /// directory compression before emitting.
    ///
    /// Compression: if a directory has exactly one child (which must be a
    /// directory with no sibling files), the two names are joined as `a/b`
    /// and the merge recurses until the leaf dir has either files or multiple
    /// children.
    fn flatten_into(&self, out: &mut Vec<TreeRow>, depth: usize) {
        // ── Directories first (BTreeMap is already sorted) ──────────────
        for (dir_name, child_node) in &self.dirs {
            // Try to compress.
            let (compressed_name, leaf_node) = compress(dir_name, child_node);
            out.push(TreeRow::Dir {
                depth,
                name: SharedString::from(compressed_name),
            });
            leaf_node.flatten_into(out, depth + 1);
        }

        // ── Files (sort by name) ─────────────────────────────────────────
        let mut sorted_files: Vec<&FileLeaf> = self.files.iter().collect();
        sorted_files.sort_by(|a, b| a.name.cmp(&b.name));

        for leaf in sorted_files {
            out.push(TreeRow::File {
                depth,
                name: SharedString::from(leaf.name.clone()),
                file_index: leaf.file_index,
                change: leaf.change.clone(),
            });
        }
    }
}

/// Compress a single-child directory chain.
///
/// Returns the concatenated label (e.g. `"a/b/c"`) and a reference to the
/// leaf [`DirNode`] where actual content lives.
fn compress<'a>(name: &str, node: &'a DirNode) -> (String, &'a DirNode) {
    let mut label = name.to_owned();
    let mut current = node;

    loop {
        // Compression condition: exactly one directory child AND no files.
        if current.dirs.len() == 1 && current.files.is_empty() {
            let (child_name, child_node) = current.dirs.iter().next().unwrap();
            label.push('/');
            label.push_str(child_name);
            current = child_node;
        } else {
            break;
        }
    }

    (label, current)
}

// ──────────────────────────────────────────────────────────────
// Unit tests
// ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper to build FileStatus entries quickly ────────────

    fn mk(path: &str, change: ChangeKind) -> FileStatus {
        FileStatus {
            path: PathBuf::from(path),
            change,
        }
    }

    fn added(path: &str) -> FileStatus {
        mk(path, ChangeKind::Added)
    }

    #[allow(dead_code)]
    fn modified(path: &str) -> FileStatus {
        mk(path, ChangeKind::Modified)
    }

    /// Assert that the total number of File rows equals `files.len()`.
    fn assert_file_count(rows: &[TreeRow], expected: usize) {
        let count = rows
            .iter()
            .filter(|r| matches!(r, TreeRow::File { .. }))
            .count();
        assert_eq!(
            count, expected,
            "TreeRow::File count ({count}) != input files count ({expected})"
        );
    }

    // ── Test 1: flat list (no directories) ───────────────────────────────
    #[test]
    fn test_flat_files() {
        let files = vec![added("b.txt"), added("a.txt"), added("c.txt")];
        let rows = build_file_tree(&files);

        assert_file_count(&rows, 3);
        // No Dir rows expected.
        assert!(rows.iter().all(|r| matches!(r, TreeRow::File { .. })));
        // Sorted by name.
        let names: Vec<&str> = rows
            .iter()
            .map(|r| match r {
                TreeRow::File { name, .. } => name.as_ref(),
                _ => "",
            })
            .collect();
        assert_eq!(names, vec!["a.txt", "b.txt", "c.txt"]);
    }

    // ── Test 2: nested directories ────────────────────────────────────────
    #[test]
    fn test_nested_dirs() {
        let files = vec![added("src/a.rs"), added("src/sub/b.rs"), added("docs/c.md")];
        let rows = build_file_tree(&files);

        assert_file_count(&rows, 3);

        // Extract Dir names.
        let dir_names: Vec<&str> = rows
            .iter()
            .filter_map(|r| match r {
                TreeRow::Dir { name, .. } => Some(name.as_ref()),
                _ => None,
            })
            .collect();

        // docs and src should appear, plus src/sub (compressed) if applicable.
        // Since src has both a.rs AND sub/, compression doesn't apply to src.
        // sub/ is a single-child-dir of src with only b.rs → no further dir nesting.
        assert!(dir_names.contains(&"docs"), "expected 'docs' dir");
        assert!(dir_names.contains(&"src"), "expected 'src' dir");
        assert!(dir_names.contains(&"sub"), "expected 'sub' dir under src");

        // Verify depths.
        let depth_map: Vec<(Option<&str>, usize)> = rows
            .iter()
            .map(|r| match r {
                TreeRow::Dir { depth, name } => (Some(name.as_ref()), *depth),
                TreeRow::File { depth, name, .. } => (Some(name.as_ref()), *depth),
            })
            .collect();

        // docs is at depth 0, c.md at depth 1.
        let docs_depth = depth_map
            .iter()
            .find(|(n, _)| *n == Some("docs"))
            .map(|(_, d)| *d);
        assert_eq!(docs_depth, Some(0));
        let c_md_depth = depth_map
            .iter()
            .find(|(n, _)| *n == Some("c.md"))
            .map(|(_, d)| *d);
        assert_eq!(c_md_depth, Some(1));

        // src is at depth 0, sub at depth 1, b.rs at depth 2.
        let src_depth = depth_map
            .iter()
            .find(|(n, _)| *n == Some("src"))
            .map(|(_, d)| *d);
        assert_eq!(src_depth, Some(0));
        let sub_depth = depth_map
            .iter()
            .find(|(n, _)| *n == Some("sub"))
            .map(|(_, d)| *d);
        assert_eq!(sub_depth, Some(1));
        let b_rs_depth = depth_map
            .iter()
            .find(|(n, _)| *n == Some("b.rs"))
            .map(|(_, d)| *d);
        assert_eq!(b_rs_depth, Some(2));
    }

    // ── Test 3: single-child directory compression ────────────────────────
    #[test]
    fn test_single_child_compression() {
        // a/ → b/ → c.rs  should compress to a Dir named "a/b" at depth 0,
        // then c.rs at depth 1.
        let files = vec![added("a/b/c.rs")];
        let rows = build_file_tree(&files);

        assert_file_count(&rows, 1);
        assert_eq!(rows.len(), 2); // one Dir + one File

        match &rows[0] {
            TreeRow::Dir { depth, name } => {
                assert_eq!(*depth, 0);
                assert_eq!(name.as_ref(), "a/b");
            }
            other => panic!("expected Dir, got {:?}", other),
        }
        match &rows[1] {
            TreeRow::File {
                depth,
                name,
                file_index,
                ..
            } => {
                assert_eq!(*depth, 1);
                assert_eq!(name.as_ref(), "c.rs");
                assert_eq!(*file_index, 0);
            }
            other => panic!("expected File, got {:?}", other),
        }
    }

    // ── Test 4: mixed dirs and files, sort order ─────────────────────────
    #[test]
    fn test_mixed_sort_order() {
        // At the root level: files z.txt, a.txt, and a directory "mdir/".
        // Dirs should come before files; within each group, sorted by name.
        let files = vec![added("z.txt"), added("mdir/inner.txt"), added("a.txt")];
        let rows = build_file_tree(&files);

        assert_file_count(&rows, 3);

        // First row must be Dir "mdir".
        match &rows[0] {
            TreeRow::Dir { depth, name } => {
                assert_eq!(*depth, 0);
                assert_eq!(name.as_ref(), "mdir");
            }
            other => panic!("expected Dir first, got {:?}", other),
        }
        // Then files sorted: a.txt, z.txt.
        let file_names: Vec<&str> = rows[1..]
            .iter()
            .filter_map(|r| match r {
                TreeRow::File { name, .. } => Some(name.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(file_names, vec!["inner.txt", "a.txt", "z.txt"]);
    }

    // ── Test 5: Japanese (non-ASCII) paths ────────────────────────────────
    #[test]
    fn test_japanese_paths() {
        // Paths with Japanese characters must not crash and must preserve identity.
        let files = vec![
            added("ドキュメント/ファイルA.txt"),
            added("ドキュメント/ファイルB.txt"),
            added("src/main.rs"),
        ];
        let rows = build_file_tree(&files);

        assert_file_count(&rows, 3);

        // The Japanese dir should appear.
        let has_jp_dir = rows.iter().any(|r| match r {
            TreeRow::Dir { name, .. } => name.as_ref() == "ドキュメント",
            _ => false,
        });
        assert!(has_jp_dir, "expected Japanese directory name");

        // Compression must not apply (two children in ドキュメント/).
        let jp_dir_depth = rows.iter().find_map(|r| match r {
            TreeRow::Dir { name, depth } if name.as_ref() == "ドキュメント" => Some(*depth),
            _ => None,
        });
        assert_eq!(jp_dir_depth, Some(0));
    }

    // ── Test 6: file_index mapping is preserved after compression ─────────
    #[test]
    fn test_file_index_preserved() {
        // Three files: the second one is under a deep compressed path.
        let files = vec![
            added("top.txt"),      // index 0
            added("x/y/deep.txt"), // index 1 — x/y will be compressed
            added("other.txt"),    // index 2
        ];
        let rows = build_file_tree(&files);

        assert_file_count(&rows, 3);

        // Find the file named "deep.txt" and verify its file_index == 1.
        let deep = rows.iter().find(|r| match r {
            TreeRow::File { name, .. } => name.as_ref() == "deep.txt",
            _ => false,
        });
        match deep {
            Some(TreeRow::File { file_index, .. }) => {
                assert_eq!(*file_index, 1, "deep.txt must retain file_index=1");
            }
            _ => panic!("deep.txt not found in tree rows"),
        }
    }

    // ── Test 7: multi-level compression chain ────────────────────────────
    #[test]
    fn test_multi_level_compression() {
        // a/b/c/d.rs — a→b→c should all compress into "a/b/c".
        let files = vec![added("a/b/c/d.rs")];
        let rows = build_file_tree(&files);

        assert_file_count(&rows, 1);
        assert_eq!(rows.len(), 2);

        match &rows[0] {
            TreeRow::Dir { name, depth } => {
                assert_eq!(*depth, 0);
                assert_eq!(name.as_ref(), "a/b/c");
            }
            other => panic!("expected compressed Dir, got {:?}", other),
        }
    }

    // ── Test 8: renamed file carries ChangeKind::Renamed ─────────────────
    #[test]
    fn test_renamed_file() {
        let files = vec![FileStatus {
            path: PathBuf::from("new_name.rs"),
            change: ChangeKind::Renamed {
                from: PathBuf::from("old_name.rs"),
            },
        }];
        let rows = build_file_tree(&files);

        assert_file_count(&rows, 1);
        match &rows[0] {
            TreeRow::File { change, .. } => {
                assert!(matches!(change, Some(ChangeKind::Renamed { .. })));
            }
            other => panic!("expected File, got {:?}", other),
        }
    }
}
