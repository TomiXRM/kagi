//! The single observation of "is this repository in the middle of something?"
//!
//! One detector (`conflicts::detect_conflict_session`), one fingerprint. Every
//! consumer reads this: the conflict family freezes [`ConflictRevision`] into
//! its requests and re-reads it live at preflight, and the per-tab read model
//! carries the pure [`ObservedOperation`] the header strip renders (#704 /
//! ADR-0196). Split out of `conflict_ops.rs` so both sides name the same
//! function rather than growing a second implementation over the same `.git`
//! files.

use crate::backend::*;
use kagi_domain::conflict_family::{
    BufferRevision, ConflictDraft, ConflictObservation, ConflictRevision, ObservedOperation,
};
use sha2::{Digest, Sha256};

#[path = "conflict_observe/approval_metadata.rs"]
mod approval_metadata;

#[derive(Clone, Debug)]
pub struct ConflictSnapshot {
    pub session: conflicts::ConflictSession,
    pub observation: ConflictObservation,
}

impl ConflictSnapshot {
    /// The pure value the per-tab read model carries (#704). Everything the
    /// header strip needs to name the operation, show its progress and freeze
    /// an abort request — and nothing that would tempt the UI to re-derive the
    /// observation for itself.
    pub fn in_progress(&self) -> ObservedOperation {
        ObservedOperation {
            observation: self.observation.clone(),
            step: match self.session.op {
                conflicts::ConflictOp::Rebase { step, total, .. } => Some((step, total)),
                _ => None,
            },
        }
    }
}

pub(crate) fn digest(parts: impl IntoIterator<Item = Vec<u8>>) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        digest_part(&mut hash, &part);
    }
    hex::encode(hash.finalize())
}

/// Shared framing for owned observation parts and borrowed per-file stages.
pub(crate) fn digest_part(hash: &mut Sha256, part: &[u8]) {
    hash.update((part.len() as u64).to_be_bytes());
    hash.update(part);
}

pub(crate) fn buffer_revision(
    path: &Path,
    draft: &ConflictDraft,
    identity: &ConflictRevision,
) -> BufferRevision {
    let mut hash = Sha256::new();
    digest_part(&mut hash, path.as_os_str().as_encoded_bytes());
    digest_part(&mut hash, identity.as_str().as_bytes());
    match draft {
        ConflictDraft::Delete => digest_part(&mut hash, b"delete"),
        ConflictDraft::Text { bytes, origins } => {
            digest_part(&mut hash, b"text");
            digest_part(&mut hash, bytes);
            for origin in origins {
                digest_part(&mut hash, &[origin.tag() as u8]);
            }
        }
        ConflictDraft::Raw { oid, mode } => {
            digest_part(&mut hash, b"raw");
            digest_part(&mut hash, oid.as_bytes());
            digest_part(&mut hash, &mode.to_be_bytes());
        }
    }
    BufferRevision::from_fingerprint(hex::encode(hash.finalize()))
}

pub(crate) fn revision_label(revision: &ConflictRevision) -> String {
    revision.as_str().chars().take(12).collect()
}

pub(crate) fn short_text_hash(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub(crate) fn observation(repo: &Repository) -> Result<Option<ConflictSnapshot>, GitError> {
    let mut index = repo
        .index()
        .map_err(|error| GitError::Other(error.message().to_owned()))?;
    index
        .read(true)
        .map_err(|error| GitError::Other(error.message().to_owned()))?;
    let Some(session) = conflicts::detect_conflict_session_from_index(repo, &index)? else {
        return Ok(None);
    };
    let (mut parts, operation_tail, _) = operation_parts(repo, &session, false)?;
    let mut entries: Vec<Vec<u8>> = index
        .iter()
        .map(|entry| {
            let mut value = entry.path;
            value.extend_from_slice(&entry.mode.to_be_bytes());
            value.extend_from_slice(&entry.flags.to_be_bytes());
            value.extend_from_slice(&entry.flags_extended.to_be_bytes());
            value.extend_from_slice(entry.id.as_bytes());
            value
        })
        .collect();
    entries.sort();
    parts.extend(entries);
    parts.extend(operation_tail);
    let revision = ConflictRevision::from_fingerprint(digest(parts));
    let paths = session.files.iter().map(|file| file.path.clone()).collect();
    Ok(Some(ConflictSnapshot {
        observation: ConflictObservation {
            revision,
            kind: session.op.kind(),
            paths,
        },
        session,
    }))
}

/// Stable operation ownership for per-file drafts, not a full write approval.
/// Message, authorship and replay options belong only to the full observation.
pub(crate) fn draft_context(repo: &Repository) -> Option<ConflictRevision> {
    // Only operation metadata participates; do not classify/read every file's
    // blob content again at the live write-identity boundary.
    let session = conflicts::ConflictSession {
        op: conflicts::classify_op(repo, repo.state())?,
        files: Vec::new(),
        resolved: Vec::new(),
    };
    let (mut parts, tail, known) = operation_parts(repo, &session, true).ok()?;
    if !known {
        return None;
    }
    parts.extend(tail);
    Some(ConflictRevision::from_fingerprint(digest(parts)))
}

type OperationParts = (Vec<Vec<u8>>, Vec<Vec<u8>>, bool);

fn operation_parts(
    repo: &Repository,
    session: &conflicts::ConflictSession,
    strict_draft: bool,
) -> Result<OperationParts, GitError> {
    let mut known = false;
    let mut prefix = vec![
        format!("{:?}", repo.state()).into_bytes(),
        format!("{:?}", session.op).into_bytes(),
    ];
    if let Ok(head) = repo.head() {
        known = head.target().is_some();
        prefix.push(head.name_bytes().to_vec());
        prefix.push(
            head.target()
                .map(|oid| oid.to_string())
                .unwrap_or_default()
                .into_bytes(),
        );
    }
    let mut parts = Vec::new();
    if strict_draft {
        known &= read_draft_metadata(repo.path(), &mut parts);
        known = known && draft_metadata_known(repo, session, &parts);
    } else {
        // Full approval owns the persisted inputs Git will consume, not just
        // the minimal revision used to retain edits to the same conflict file.
        parts.push(b"operation-state-v1".to_vec());
        parts.push(approval_metadata::digest(repo)?);
    }
    // #707 review: the ref an abort would *overwrite*, and where it points now.
    // `head-name` names it, but during a rebase HEAD is detached, so nothing
    // above says where `refs/heads/<pre-rebase-branch>` currently is — an
    // external `git update-ref` on it passed preflight and was then clobbered
    // by the restore. Absent / symbolic / unreadable are their own markers: an
    // abort must not silently create or overwrite a ref that appeared since.
    parts.push(b"restore-ref".to_vec());
    match crate::ops::conflict_abort::restore_ref(repo, session) {
        Some(restore) => {
            known &= restore.target.is_some();
            parts.push(restore.name.into_bytes());
            parts.push(
                restore
                    .target
                    .unwrap_or_else(|| "unresolved".into())
                    .into_bytes(),
            );
        }
        None => parts.push(b"detached".to_vec()),
    }
    Ok((prefix, parts, known))
}

fn read_draft_metadata(git_dir: &Path, parts: &mut Vec<Vec<u8>>) -> bool {
    let mut known = true;
    for relative in [
        "MERGE_HEAD",
        "REBASE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "ORIG_HEAD",
        "sequencer/head",
        "sequencer/todo",
        "sequencer/abort-safety",
        "rebase-merge/head-name",
        "rebase-apply/head-name",
        "rebase-merge/done",
        "rebase-merge/git-rebase-todo",
        "rebase-merge/msgnum",
        "rebase-merge/end",
        "rebase-apply/next",
        "rebase-apply/last",
    ] {
        match std::fs::read(git_dir.join(relative)) {
            Ok(bytes) => {
                parts.push(relative.as_bytes().to_vec());
                parts.push(bytes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => known = false,
        }
    }
    known
}

/// Optional absent files are not failures. Required operation metadata must be
/// present and parseable before a best-effort observation can authorize drafts.
fn draft_metadata_known(
    repo: &Repository,
    session: &conflicts::ConflictSession,
    parts: &[Vec<u8>],
) -> bool {
    let metadata = |name: &str| {
        parts
            .chunks_exact(2)
            .find(|pair| pair[0] == name.as_bytes())
            .map(|pair| pair[1].as_slice())
    };
    let text = |name| {
        metadata(name)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .map(str::trim)
            .filter(|text| !text.is_empty())
    };
    let oid = |name| {
        text(name).is_some_and(|text| {
            text.lines()
                .all(|line| git2::Oid::from_str(line.trim()).is_ok())
        })
    };
    for name in ["ORIG_HEAD", "sequencer/head", "sequencer/abort-safety"] {
        if metadata(name).is_some() && !oid(name) {
            return false;
        }
    }
    if matches!(
        repo.state(),
        git2::RepositoryState::CherryPickSequence | git2::RepositoryState::RevertSequence
    ) && (!oid("sequencer/head") || metadata("sequencer/todo").is_none())
    {
        return false;
    }
    match &session.op {
        conflicts::ConflictOp::Merge { .. } => oid("MERGE_HEAD"),
        conflicts::ConflictOp::CherryPick { .. } => oid("CHERRY_PICK_HEAD"),
        conflicts::ConflictOp::Revert { .. } => oid("REVERT_HEAD"),
        conflicts::ConflictOp::StashConflict => true,
        conflicts::ConflictOp::Rebase { .. } => {
            // These also feed the detector's operation description. A read
            // error must not silently turn its source into a fallback label.
            for name in [
                "rebase-merge/stopped-sha",
                "rebase-merge/orig-head",
                "rebase-apply/original-commit",
                "rebase-apply/orig-head",
            ] {
                match std::fs::read(repo.path().join(name)) {
                    Ok(bytes) => {
                        if std::str::from_utf8(&bytes)
                            .ok()
                            .is_none_or(|text| git2::Oid::from_str(text.trim()).is_err())
                        {
                            return false;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return false,
                }
            }
            let (head, step, total, merge) = match repo.state() {
                git2::RepositoryState::Rebase => (
                    "rebase-apply/head-name",
                    "rebase-apply/next",
                    "rebase-apply/last",
                    false,
                ),
                _ => (
                    "rebase-merge/head-name",
                    "rebase-merge/msgnum",
                    "rebase-merge/end",
                    true,
                ),
            };
            let progress = text(step)
                .and_then(|value| value.parse::<usize>().ok())
                .zip(text(total).and_then(|value| value.parse::<usize>().ok()))
                .is_some_and(|(step, total)| step > 0 && total >= step);
            oid("REBASE_HEAD")
                && text(head).is_some()
                && progress
                && (!merge
                    || (metadata("rebase-merge/done").is_some()
                        && metadata("rebase-merge/git-rebase-todo").is_some()))
        }
    }
}
pub(crate) fn text_mode(repo: &Repository, path: &Path) -> Result<u32, GitError> {
    let index = repo
        .index()
        .map_err(|error| GitError::Other(error.message().to_owned()))?;
    let worktree_mode = repo
        .workdir()
        .and_then(|root| conflicts::original_file_mode(&root.join(path)));
    conflicts::text_resolution_mode(repo, &index, path, worktree_mode)
}

pub(crate) fn verify_save(
    repo: &Repository,
    path: &Path,
    draft: &ConflictDraft,
    expected_mode: u32,
    expected_oid: git2::Oid,
) -> Result<(), GitError> {
    let index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
    if index.get_path(path, 1).is_some()
        || index.get_path(path, 2).is_some()
        || index.get_path(path, 3).is_some()
    {
        return Err(GitError::Other(format!(
            "{} remains unmerged after save",
            path.display()
        )));
    }
    let entry = index
        .get_path(path, 0)
        .ok_or_else(|| GitError::Other(format!("{} was not staged", path.display())))?;
    match draft {
        ConflictDraft::Delete => {
            return Err(GitError::Other(
                "deletion requires deletion verification".into(),
            ));
        }
        ConflictDraft::Text { bytes, .. } => {
            let root = repo
                .workdir()
                .ok_or_else(|| GitError::Other("repository has no working tree".into()))?;
            let actual = std::fs::read(root.join(path))
                .map_err(|e| GitError::Other(format!("verify {} failed: {e}", path.display())))?;
            if actual != *bytes || entry.id != expected_oid || entry.mode != expected_mode {
                return Err(GitError::Other(format!(
                    "{} bytes, blob, or mode differ after save",
                    path.display()
                )));
            }
        }
        ConflictDraft::Raw { oid, mode } => {
            if entry.id.to_string() != *oid || entry.mode != *mode {
                return Err(GitError::Other(format!(
                    "{} raw OID or mode differs after save",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn verify_dir_file(repo: &Repository, plan: &ops::DirFilePlan) -> Result<(), GitError> {
    ops::verify_dir_file_resolution(repo, plan)
}

/// #704 / ADR-0196: measure the abort rather than trusting it. The operation
/// state has to be gone, and HEAD has to stand where the restore said it put
/// it — a `cleanup_state` that silently left `MERGE_HEAD` behind is exactly
/// the dead end this issue is about, and must not be recorded as a success.
pub(crate) fn verify_abort(
    repo: &Repository,
    restored: &conflicts::AbortOutcome,
) -> Result<(), GitError> {
    if let Some(live) = observation(repo)? {
        return Err(GitError::Other(format!(
            "{} is still in progress after the abort",
            live.observation.kind.slug()
        )));
    }
    let Some(target) = &restored.restored_to else {
        return Ok(());
    };
    let head = repo
        .head()
        .ok()
        .and_then(|head| head.target())
        .map(|oid| oid.to_string());
    if head.as_deref() != Some(target.as_str()) {
        return Err(GitError::Other(format!(
            "HEAD is {} after the abort, not the restored {target}",
            head.unwrap_or_else(|| "unresolvable".into())
        )));
    }
    if !restored.restored_paths.is_empty() {
        verify_restored_paths(repo, target, &restored.restored_paths)?;
    }
    Ok(())
}

/// Compare both sides independently: a matching HEAD cannot prove that checkout
/// restored the index or the worktree. A fresh in-memory index has no stat cache,
/// so worktree verification reads content even for same-size corruption.
fn verify_restored_paths(
    repo: &Repository,
    target: &str,
    paths: &[PathBuf],
) -> Result<(), GitError> {
    let verify = || -> Result<bool, git2::Error> {
        let tree = repo.find_commit(git2::Oid::from_str(target)?)?.tree()?;
        let mut options = git2::DiffOptions::new();
        options
            .disable_pathspec_match(true)
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_typechange(true);
        for path in paths {
            options.pathspec(path.as_path());
        }
        let index = repo.index()?;
        if repo
            .diff_tree_to_index(Some(&tree), Some(&index), Some(&mut options))?
            .deltas()
            .len()
            != 0
        {
            return Ok(false);
        }
        let mut expected = git2::Index::new()?;
        expected.read_tree(&tree)?;
        Ok(repo
            .diff_index_to_workdir(Some(&expected), Some(&mut options))?
            .deltas()
            .len()
            == 0)
    };
    match verify() {
        Ok(true) => Ok(()),
        Ok(false) => Err(GitError::Other(
            "restored index or worktree differs after stash abort".into(),
        )),
        Err(error) => Err(GitError::Other(format!(
            "stash abort verification failed: {}",
            error.message()
        ))),
    }
}

#[cfg(all(test, unix))]
mod stash_abort_tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn stash_abort_verification_rejects_non_utf8_index_and_worktree_corruption() {
        assert_corruption_rejected(Path::new(std::ffi::OsStr::from_bytes(b"restored-\xff.txt")));
    }

    #[test]
    fn stash_abort_verification_rejects_unicode_index_and_worktree_corruption() {
        assert_corruption_rejected(Path::new("restored-雪.txt"));
    }

    fn assert_corruption_rejected(path: &Path) {
        for corruption in ["missing index", "wrong index blob", "worktree bytes"] {
            let temp = tempfile::tempdir().unwrap();
            let repo = Repository::init(temp.path()).unwrap();
            if let Err(error) = std::fs::write(temp.path().join(path), b"restored\n") {
                if path.to_str().is_none() && error.raw_os_error() == Some(libc::EILSEQ) {
                    eprintln!(
                        "skip: filesystem rejects non-UTF-8 filenames (EILSEQ); Unicode corruption coverage runs separately"
                    );
                    return;
                }
                panic!("cannot create verification fixture: {error}");
            }
            let mut index = repo.index().unwrap();
            index.add_path(path).unwrap();
            index.write().unwrap();
            let tree_oid = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_oid).unwrap();
            let signature = git2::Signature::now("Test", "test@example.com").unwrap();
            let head = repo
                .commit(Some("HEAD"), &signature, &signature, "base", &tree, &[])
                .unwrap();
            let outcome = conflicts::AbortOutcome {
                restored_to: Some(head.to_string()),
                buffer_preserved_at: None,
                restored_paths: vec![path.to_path_buf()],
            };
            verify_abort(&repo, &outcome).expect("uncorrupted restore verifies");

            match corruption {
                "missing index" => {
                    index.remove_path(path).unwrap();
                    index.write().unwrap();
                }
                "wrong index blob" => {
                    let mut entry = index.get_path(path, 0).unwrap();
                    entry.id = repo.blob(b"corrupt!\n").unwrap();
                    index.add(&entry).unwrap();
                    index.write().unwrap();
                }
                "worktree bytes" => {
                    // Same length, unchanged HEAD and staged blob: verification
                    // must inspect worktree content, not only metadata or HEAD.
                    std::fs::write(temp.path().join(path), b"corrupt!\n").unwrap();
                }
                _ => unreachable!(),
            }
            assert_eq!(repo.head().unwrap().target(), Some(head));
            assert!(
                verify_abort(&repo, &outcome).is_err(),
                "verification accepted {corruption}"
            );
        }
    }
}
