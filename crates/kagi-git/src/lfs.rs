//! Git LFS pointer handling at the backend boundary. Reads never invoke LFS.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::Duration,
};

use git2::{Index, IndexEntry, IndexTime, Repository};
use sha2::{Digest, Sha256};

use crate::GitError;

const POINTER_VERSION: &str = "version https://git-lfs.github.com/spec/v1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Pointer {
    pub oid: String,
    pub size: u64,
}

pub(crate) fn parse_pointer(bytes: &[u8]) -> Option<Pointer> {
    if bytes.len() > 1024 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    if lines.next()? != POINTER_VERSION {
        return None;
    }
    let oid = lines.next()?.strip_prefix("oid sha256:")?;
    if oid.len() != 64
        || !oid
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return None;
    }
    let size = lines.next()?.strip_prefix("size ")?.parse().ok()?;
    if lines.next().is_some() {
        return None;
    }
    Some(Pointer {
        oid: oid.to_owned(),
        size,
    })
}

fn hash_file(file: &mut File) -> Result<Pointer, GitError> {
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut buf = [0u8; 128 * 1024];
    loop {
        let len = file
            .read(&mut buf)
            .map_err(|e| GitError::Other(format!("cannot read LFS file: {e}")))?;
        if len == 0 {
            break;
        }
        hasher.update(&buf[..len]);
        size += len as u64;
    }
    Ok(Pointer {
        oid: hex::encode(hasher.finalize()),
        size,
    })
}

pub(crate) fn worktree_matches_index(
    repo: &Repository,
    index: &Index,
    path: &Path,
) -> Result<bool, GitError> {
    let Some(entry) = index.get_path(path, 0) else {
        return Ok(false);
    };
    let blob = repo
        .find_blob(entry.id)
        .map_err(|e| GitError::Other(format!("cannot read LFS index blob: {e}")))?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working tree".into()))?;
    let mut file = File::open(workdir.join(path))
        .map_err(|e| GitError::Other(format!("cannot read LFS worktree file: {e}")))?;
    let meta = file
        .metadata()
        .map_err(|e| GitError::Other(format!("cannot inspect LFS file: {e}")))?;
    if !meta.is_file() {
        return Ok(false);
    }
    // Git LFS passes empty files through without creating a pointer.
    if blob.content().is_empty() {
        return Ok(meta.len() == 0);
    }
    let Some(pointer) = parse_pointer(blob.content()) else {
        return Ok(false);
    };
    if meta.len() == blob.size() as u64 {
        let mut small = Vec::new();
        file.read_to_end(&mut small)
            .map_err(|e| GitError::Other(format!("cannot read LFS pointer: {e}")))?;
        if small == blob.content() {
            return Ok(true);
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|e| GitError::Other(format!("cannot rewind LFS file: {e}")))?;
    }
    if meta.len() != pointer.size {
        return Ok(false);
    }
    Ok(hash_file(&mut file)? == pointer)
}

pub(crate) fn stage_pointer(
    repo: &Repository,
    index: &mut Index,
    path: &Path,
) -> Result<(), GitError> {
    if crate::special_repo::FilterCheck::new(repo)?.is_lockable(path)? {
        return Err(GitError::Other(
            "LFS lockable files require lock verification before Stage".into(),
        ));
    }
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working tree".into()))?;
    let abs = workdir.join(path);
    let mut file =
        File::open(&abs).map_err(|e| GitError::Other(format!("cannot open LFS file: {e}")))?;
    let meta = file
        .metadata()
        .map_err(|e| GitError::Other(format!("cannot inspect LFS file: {e}")))?;
    if !meta.is_file()
        || std::fs::symlink_metadata(&abs)
            .map_err(|e| GitError::Other(format!("cannot inspect LFS path: {e}")))?
            .file_type()
            .is_symlink()
    {
        return Err(GitError::Other("LFS Stage requires a regular file".into()));
    }
    let mode = stage_mode(repo, index, path, &meta)?;
    if meta.len() == 0 {
        let path_bytes = crate::conflicts::path_to_index_bytes(path)
            .ok_or_else(|| GitError::Other("LFS path cannot be represented in index".into()))?;
        return index
            .add_frombuffer(&index_entry(path_bytes, &meta, mode), b"")
            .map_err(|e| GitError::Other(format!("cannot stage empty LFS file: {e}")));
    }
    // Skip-smudge leaves the pointer itself in the worktree. Never import that
    // text as a new LFS object; it can only preserve an identical index entry.
    if meta.len() <= 1024 {
        let mut small = Vec::new();
        file.read_to_end(&mut small)
            .map_err(|e| GitError::Other(format!("cannot read LFS file: {e}")))?;
        if parse_pointer(&small).is_some() {
            let same = index
                .get_path(path, 0)
                .and_then(|entry| repo.find_blob(entry.id).ok())
                .is_some_and(|blob| blob.content() == small);
            return if same {
                Ok(())
            } else {
                Err(GitError::Other(
                    "LFS pointer placeholder differs from the index".into(),
                ))
            };
        }
        if small.starts_with(POINTER_VERSION.as_bytes()) {
            return Err(GitError::Other(
                "unsupported LFS pointer placeholder cannot be staged".into(),
            ));
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|e| GitError::Other(format!("cannot rewind LFS file: {e}")))?;
    }
    let expected = hash_file(&mut file)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|e| GitError::Other(format!("cannot rewind LFS file: {e}")))?;
    check_supported_configuration(repo, None)?;
    let rel = path
        .to_str()
        .ok_or_else(|| GitError::Other("non-UTF-8 LFS path is unsupported".into()))?;
    let out = crate::cli::run_git_with_input_file(
        workdir,
        &["lfs", "clean", "--", rel],
        file,
        Duration::from_secs(300),
    )
    .map_err(|e| crate::cli::context("Git LFS clean failed (install git-lfs if missing)", e))?;
    if out.status != 0 {
        return Err(GitError::Other(format!(
            "Git LFS clean failed: {}",
            out.stderr.trim()
        )));
    }
    if parse_pointer(out.stdout.as_bytes()) != Some(expected) {
        return Err(GitError::Other(
            "Git LFS clean returned a pointer that does not match the worktree file".into(),
        ));
    }
    let path_bytes = crate::conflicts::path_to_index_bytes(path)
        .ok_or_else(|| GitError::Other("LFS path cannot be represented in index".into()))?;
    let entry = index_entry(path_bytes, &meta, mode);
    index
        .add_frombuffer(&entry, out.stdout.as_bytes())
        .map_err(|e| GitError::Other(format!("cannot stage LFS pointer: {e}")))
}

fn stage_mode(
    repo: &Repository,
    index: &Index,
    path: &Path,
    meta: &std::fs::Metadata,
) -> Result<u32, GitError> {
    #[cfg(unix)]
    let actual = {
        use std::os::unix::fs::MetadataExt;
        if meta.mode() & 0o111 == 0 {
            0o100644
        } else {
            0o100755
        }
    };
    #[cfg(not(unix))]
    let actual = 0o100644;
    let honor_filemode = match repo
        .config()
        .map_err(|e| GitError::Other(format!("cannot read core.filemode: {e}")))?
        .get_bool("core.filemode")
    {
        Ok(value) => value,
        Err(error) if error.code() == git2::ErrorCode::NotFound => true,
        Err(error) => {
            return Err(GitError::Other(format!(
                "cannot read core.filemode: {error}"
            )))
        }
    };
    if !honor_filemode {
        if let Some(entry) = index.get_path(path, 0) {
            if matches!(entry.mode, 0o100644 | 0o100755) {
                return Ok(entry.mode);
            }
        }
    }
    Ok(actual)
}

fn index_entry(path: Vec<u8>, meta: &std::fs::Metadata, mode: u32) -> IndexEntry {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        IndexEntry {
            ctime: IndexTime::new(meta.ctime() as i32, meta.ctime_nsec() as u32),
            mtime: IndexTime::new(meta.mtime() as i32, meta.mtime_nsec() as u32),
            dev: meta.dev() as u32,
            ino: meta.ino() as u32,
            mode,
            uid: meta.uid(),
            gid: meta.gid(),
            file_size: meta.len() as u32,
            id: git2::Oid::ZERO_SHA1,
            flags: 0,
            flags_extended: 0,
            path,
        }
    }
    #[cfg(not(unix))]
    {
        IndexEntry {
            ctime: IndexTime::new(0, 0),
            mtime: IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode,
            uid: 0,
            gid: 0,
            file_size: meta.len() as u32,
            id: git2::Oid::ZERO_SHA1,
            flags: 0,
            flags_extended: 0,
            path,
        }
    }
}

/// Upload LFS content before publishing a ref. This scans commits in the
/// approved history for a pointer, so Git LFS remains optional in plain repos.
pub(crate) fn upload_for_push(
    repo: &Repository,
    repo_path: &Path,
    remote: &str,
    tip: git2::Oid,
) -> Result<(), GitError> {
    if !history_contains_pointer(repo, remote, tip)? {
        return Ok(());
    }
    check_supported_configuration(repo, Some(remote))?;
    let approved_tip = tip.to_string();
    let out = crate::cli::run_git_with_options(
        repo_path,
        &["lfs", "push", remote, &approved_tip],
        crate::cli::GitCliOptions {
            timeout: Some(Duration::from_secs(3600)),
            ..Default::default()
        },
    )
    .map_err(|e| crate::cli::context("LFS upload failed (install git-lfs if missing)", e))?;
    if out.status != 0 {
        return Err(GitError::Other(format!(
            "LFS upload failed: {}",
            out.stderr.trim()
        )));
    }
    Ok(())
}

fn history_contains_pointer(
    repo: &Repository,
    remote: &str,
    tip: git2::Oid,
) -> Result<bool, GitError> {
    let tip_tree = repo
        .find_commit(tip)
        .and_then(|commit| commit.tree())
        .map_err(|e| GitError::Other(format!("cannot inspect push tip tree: {e}")))?;
    let mut found = scan_lfs_tree(repo, &tip_tree)?;
    let mut walk = repo
        .revwalk()
        .map_err(|e| GitError::Other(format!("cannot inspect push history: {e}")))?;
    walk.push(tip)
        .map_err(|e| GitError::Other(format!("cannot inspect push tip: {e}")))?;
    let prefix = format!("refs/remotes/{remote}/");
    let refs = repo
        .references()
        .map_err(|e| GitError::Other(format!("cannot inspect remote tracking refs: {e}")))?;
    for reference in refs {
        let reference = reference
            .map_err(|e| GitError::Other(format!("cannot inspect remote tracking ref: {e}")))?;
        if reference.name().is_ok_and(|name| name.starts_with(&prefix)) {
            if let Some(oid) = reference.target() {
                walk.hide(oid)
                    .map_err(|e| GitError::Other(format!("cannot bound push history: {e}")))?;
            }
        }
    }
    for oid in walk {
        let commit = repo
            .find_commit(
                oid.map_err(|e| GitError::Other(format!("cannot walk push history: {e}")))?,
            )
            .map_err(|e| GitError::Other(format!("cannot read push commit: {e}")))?;
        let tree = commit
            .tree()
            .map_err(|e| GitError::Other(format!("cannot read push tree: {e}")))?;
        let parent = if commit.parent_count() == 0 {
            None
        } else {
            Some(
                commit
                    .parent(0)
                    .and_then(|c| c.tree())
                    .map_err(|e| GitError::Other(format!("cannot read parent tree: {e}")))?,
            )
        };
        let diff = repo
            .diff_tree_to_tree(parent.as_ref(), Some(&tree), None)
            .map_err(|e| GitError::Other(format!("cannot inspect push files: {e}")))?;
        let attrs = crate::special_repo::FilterCheck::for_tree(repo, &tree)?;
        for delta in diff.deltas() {
            if delta.new_file().path().is_some_and(|path| {
                path.file_name()
                    .is_some_and(|name| name == ".gitattributes")
            }) {
                found |= scan_lfs_tree(repo, &tree)?;
            }
            let file = delta.new_file();
            if file.id().is_zero() {
                continue;
            }
            let path = file
                .path()
                .ok_or_else(|| GitError::Other("push file has no path".into()))?;
            if file.size() > 1024 {
                if attrs.is_lfs(path)? {
                    return Err(GitError::Other(
                        "nonstandard LFS blob in push history".into(),
                    ));
                }
                continue;
            }
            let blob = match repo.find_blob(file.id()) {
                Ok(blob) => blob,
                Err(_) => continue,
            };
            if blob.content().starts_with(POINTER_VERSION.as_bytes()) && attrs.is_lfs(path)? {
                if parse_pointer(blob.content()).is_none() {
                    return Err(GitError::Other(
                        "unsupported LFS pointer in push history".into(),
                    ));
                }
                if attrs.is_lockable(path)? {
                    return Err(GitError::Other(
                        "LFS lockable history requires lock verification before Push".into(),
                    ));
                }
                found = true;
            }
        }
    }
    Ok(found)
}

fn scan_lfs_tree(repo: &Repository, tree: &git2::Tree<'_>) -> Result<bool, GitError> {
    let attrs = crate::special_repo::FilterCheck::for_tree(repo, tree)?;
    visit_lfs_pointer_tree(repo, tree, Path::new(""), &attrs)
}

fn visit_lfs_pointer_tree(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    parent: &Path,
    attrs: &crate::special_repo::FilterCheck,
) -> Result<bool, GitError> {
    let mut found = false;
    for entry in tree.iter() {
        let name = index_path(entry.name_bytes()).ok_or_else(|| {
            GitError::Other("non-UTF-8 target path cannot be checked for LFS".into())
        })?;
        let path = parent.join(name);
        if entry.kind() == Some(git2::ObjectType::Tree) {
            let child = repo
                .find_tree(entry.id())
                .map_err(|e| GitError::Other(format!("cannot inspect LFS subtree: {e}")))?;
            found |= visit_lfs_pointer_tree(repo, &child, &path, attrs)?;
        } else if entry.kind() == Some(git2::ObjectType::Blob) && attrs.is_lfs(&path)? {
            let blob = repo
                .find_blob(entry.id())
                .map_err(|e| GitError::Other(format!("cannot inspect LFS blob: {e}")))?;
            if blob.content().starts_with(POINTER_VERSION.as_bytes()) {
                if parse_pointer(blob.content()).is_none() {
                    return Err(GitError::Other(
                        "unsupported LFS pointer in push tree".into(),
                    ));
                }
                if attrs.is_lockable(&path)? {
                    return Err(GitError::Other(
                        "LFS lockable history requires lock verification before Push".into(),
                    ));
                }
                found = true;
            } else if blob.size() > 1024 {
                return Err(GitError::Other("nonstandard LFS blob in push tree".into()));
            }
        }
    }
    Ok(found)
}

fn check_supported_configuration(repo: &Repository, remote: Option<&str>) -> Result<(), GitError> {
    if let Some(workdir) = repo.workdir() {
        match std::fs::symlink_metadata(workdir.join(".lfsconfig")) {
            Ok(_) => {
                return Err(GitError::Other(
                    ".lfsconfig needs review before an LFS write".into(),
                ))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(GitError::Other(format!(
                    "cannot inspect .lfsconfig: {error}"
                )))
            }
        }
    }
    if repo
        .index()
        .map_err(|e| GitError::Other(format!("cannot inspect LFS config index: {e}")))?
        .get_path(Path::new(".lfsconfig"), 0)
        .is_some()
        || repo
            .head()
            .ok()
            .and_then(|head| head.peel_to_tree().ok())
            .and_then(|tree| tree.get_path(Path::new(".lfsconfig")).ok())
            .is_some()
    {
        return Err(GitError::Other(
            "tracked .lfsconfig needs review before an LFS write".into(),
        ));
    }
    let config = repo
        .config()
        .map_err(|e| GitError::Other(format!("cannot inspect LFS config: {e}")))?;
    let mut entries = config
        .entries(None)
        .map_err(|e| GitError::Other(format!("cannot inspect LFS config: {e}")))?;
    let pushurl_key =
        remote.map(|remote| format!("remote.{}.pushurl", remote.to_ascii_lowercase()));
    let url_key = remote.map(|remote| format!("remote.{}.url", remote.to_ascii_lowercase()));
    let mut pushurl_count = 0;
    let mut url_count = 0;
    while let Some(entry) = entries.next() {
        let entry =
            entry.map_err(|e| GitError::Other(format!("cannot inspect LFS config entry: {e}")))?;
        let name = entry
            .name()
            .map_err(|e| GitError::Other(format!("cannot read LFS config key: {e}")))?;
        let lower = name.to_ascii_lowercase();
        if pushurl_key.as_deref() == Some(lower.as_str()) {
            pushurl_count += 1;
        }
        if url_key.as_deref() == Some(lower.as_str()) {
            url_count += 1;
        }
        let unsupported = lower == "lfs.url"
            || lower == "lfs.pushurl"
            || lower == "lfs.storage"
            || lower == "lfs.allowincompletepush"
            || lower.starts_with("lfs.customtransfer.")
            || lower.starts_with("lfs.extension.")
            || lower == "lfs.standalonetransferagent"
            || remote.is_some_and(|r| {
                lower == format!("remote.{}.lfsurl", r.to_ascii_lowercase())
                    || lower == format!("remote.{}.lfspushurl", r.to_ascii_lowercase())
            });
        if unsupported {
            return Err(GitError::Other(format!(
                "custom {name} needs review before an LFS write"
            )));
        }
    }
    if pushurl_count > 1 || (pushurl_count == 0 && url_count > 1) {
        return Err(GitError::Other(
            "multiple Git push URLs need LFS upload to each destination".into(),
        ));
    }
    Ok(())
}

pub(crate) fn worktree_write_blocker(
    repo: &Repository,
    op: &kagi_domain::operation::Operation,
) -> Result<Option<kagi_domain::plan_note::PlanNote>, GitError> {
    use kagi_domain::operation::Operation;
    let writes_worktree = matches!(
        op,
        Operation::Checkout { .. }
            | Operation::CheckoutCommit { .. }
            | Operation::CreateBranchWithCheckout {
                checkout_after: true,
                ..
            }
            | Operation::CreateWorktree { .. }
            | Operation::OpenWorktreeForBranch { .. }
            | Operation::StashPush { .. }
            | Operation::StashApply { .. }
            | Operation::StashPop { .. }
            | Operation::CherryPick { .. }
            | Operation::MergeBranch { .. }
            | Operation::MergeIntoConflict { .. }
            | Operation::CheckoutTrackingBranch { .. }
            | Operation::SwitchToLatestBranch { .. }
            | Operation::Revert { .. }
            | Operation::Pull
            | Operation::PullBranchFf { .. }
            | Operation::UndoCommit
            | Operation::ResetCurrentToHead { .. }
            | Operation::RebaseCurrentOnto { .. }
            | Operation::SyncToRemote { .. }
            | Operation::RestoreSnapshot { .. }
            | Operation::ApplySuggestion { .. }
    );
    if !writes_worktree {
        return Ok(None);
    }
    if let Some(note) = current_lfs_blocker(repo)? {
        return Ok(Some(note));
    }
    let target = match op {
        Operation::Checkout { branch }
        | Operation::OpenWorktreeForBranch { branch, .. }
        | Operation::MergeBranch { target: branch }
        | Operation::MergeIntoConflict { target: branch }
        | Operation::RebaseCurrentOnto { onto: branch }
        | Operation::SyncToRemote { branch }
        | Operation::PullBranchFf {
            branch_name: branch,
        } => Some(branch.clone()),
        Operation::CheckoutTrackingBranch { remote_branch, .. }
        | Operation::SwitchToLatestBranch { remote_branch, .. } => Some(remote_branch.clone()),
        Operation::CheckoutCommit { id }
        | Operation::CherryPick { id }
        | Operation::Revert { id }
        | Operation::ResetCurrentToHead { target: id }
        | Operation::CreateBranchWithCheckout { at: id, .. }
        | Operation::CreateWorktree { start: id, .. } => Some(id.0.clone()),
        Operation::StashApply { index } | Operation::StashPop { index } => {
            Some(format!("stash@{{{index}}}"))
        }
        Operation::RestoreSnapshot { id } => Some(format!("refs/kagi/snapshots/{id}")),
        Operation::ApplySuggestion { head, .. } => Some(head.0.clone()),
        Operation::UndoCommit => Some("HEAD^".to_string()),
        _ => None,
    };
    if let Some(target) = target {
        if let Ok(object) = repo.revparse_single(&target) {
            if let Ok(tree) = object.peel_to_tree() {
                if let Some(note) = tree_lfs_blocker(repo, &tree)? {
                    return Ok(Some(note));
                }
            }
        }
    }
    if let Operation::Revert { id } = op {
        if let Ok(commit) = repo.find_commit(
            id.0.parse()
                .map_err(|e| GitError::Other(format!("cannot inspect revert target: {e}")))?,
        ) {
            if let Ok(parent) = commit.parent(0) {
                if let Ok(tree) = parent.tree() {
                    return tree_lfs_blocker(repo, &tree);
                }
            }
        }
    }
    Ok(None)
}

/// A worktree writer can overwrite any currently tracked LFS path, even when
/// the destination tree does not contain that path.
pub(crate) fn current_lfs_blocker(
    repo: &Repository,
) -> Result<Option<kagi_domain::plan_note::PlanNote>, GitError> {
    use kagi_domain::plan_note::{CommonNote, PlanNote};
    let attrs = crate::special_repo::FilterCheck::new(repo)?;
    let indexed_attrs = crate::special_repo::FilterCheck::for_index(repo)?;
    let head_attrs = repo
        .head()
        .ok()
        .and_then(|head| head.peel_to_tree().ok())
        .map(|tree| crate::special_repo::FilterCheck::for_tree(repo, &tree))
        .transpose()?;
    let index = repo
        .index()
        .map_err(|e| GitError::Other(format!("cannot inspect LFS paths: {e}")))?;
    for entry in index.iter() {
        let Some(path) = index_path(&entry.path) else {
            continue;
        };
        let head_lfs = match &head_attrs {
            Some(attrs) => attrs.is_lfs(&path)?,
            None => false,
        };
        if attrs.is_lfs(&path)? || indexed_attrs.is_lfs(&path)? || head_lfs {
            return Ok(Some(PlanNote::Common(CommonNote::ExternalFilterSync {
                path: path.display().to_string(),
                filter: "lfs".into(),
            })));
        }
    }
    Ok(None)
}

/// Check the target tree using its committed nested attributes, without
/// checking out any file or invoking a filter.
pub(crate) fn tree_lfs_blocker(
    repo: &Repository,
    tree: &git2::Tree<'_>,
) -> Result<Option<kagi_domain::plan_note::PlanNote>, GitError> {
    let attrs = crate::special_repo::FilterCheck::for_tree(repo, tree)?;
    visit_lfs_tree(repo, tree, Path::new(""), &attrs)
}

fn visit_lfs_tree(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    parent: &Path,
    attrs: &crate::special_repo::FilterCheck,
) -> Result<Option<kagi_domain::plan_note::PlanNote>, GitError> {
    use kagi_domain::plan_note::{CommonNote, PlanNote};
    for entry in tree.iter() {
        let Some(name) = index_path(entry.name_bytes()) else {
            return Err(GitError::Other(
                "non-UTF-8 target path cannot be checked for LFS".into(),
            ));
        };
        let path = parent.join(name);
        match entry.kind() {
            Some(git2::ObjectType::Tree) => {
                let nested = repo
                    .find_tree(entry.id())
                    .map_err(|e| GitError::Other(format!("cannot inspect target tree: {e}")))?;
                if let Some(blocker) = visit_lfs_tree(repo, &nested, &path, attrs)? {
                    return Ok(Some(blocker));
                }
            }
            Some(git2::ObjectType::Blob) if attrs.is_lfs(&path)? => {
                return Ok(Some(PlanNote::Common(CommonNote::ExternalFilterSync {
                    path: path.display().to_string(),
                    filter: "lfs".into(),
                })));
            }
            _ => {}
        }
    }
    Ok(None)
}

fn index_path(bytes: &[u8]) -> Option<std::path::PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
    }
    #[cfg(not(unix))]
    {
        std::str::from_utf8(bytes)
            .ok()
            .map(std::path::PathBuf::from)
    }
}
