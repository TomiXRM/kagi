//! Source-faithful prefix probes and bounded application-owned file reads.
use std::{io::Read, path::Path};

use super::Backend;
use crate::{CommitId, GitError};

#[derive(Clone, Copy, Debug)]
pub enum FileReadSource<'a> {
    Commit(&'a CommitId),
    Head,
    Index,
    /// Unstaged base: stage 0, then conflict ours/ancestor, otherwise HEAD.
    IndexOrHead,
    /// Frozen conflict-side identity; never consults the current index or path.
    /// Gitlinks have no blob and return `None` without object lookup.
    ConflictStage {
        oid: &'a str,
        mode: u32,
    },
    Workdir,
}

impl Backend {
    /// Stage-0 mode, without looking up the object (gitlinks need not exist in
    /// this repository's object database). No HEAD or conflict-stage fallback.
    pub fn index_file_mode(&self, path: &Path) -> Result<Option<u32>, GitError> {
        let index = self.repo.index().map_err(git_error)?;
        Ok(index.get_path(path, 0).map(|entry| entry.mode))
    }

    /// Source mode and complete object identity, without requiring gitlink objects.
    pub fn file_identity(
        &self,
        source: FileReadSource<'_>,
        path: &Path,
    ) -> Result<Option<(Option<String>, u32)>, GitError> {
        if let FileReadSource::ConflictStage {
            oid,
            mode: 0o160000,
        } = source
        {
            return Ok(Some((Some(oid.to_owned()), 0o160000)));
        }
        if matches!(source, FileReadSource::Workdir) {
            let root = self
                .workdir()
                .ok_or_else(|| GitError::Other("No working directory".into()))?;
            return match std::fs::symlink_metadata(root.join(path)) {
                Ok(meta) if meta.file_type().is_symlink() => Ok(Some((None, 0o120000))),
                Ok(meta) if meta.is_file() => Ok(Some((None, 0o100644))),
                Ok(meta) if meta.is_dir() => {
                    // Only a recorded gitlink may inspect a nested worktree.
                    let index = self.repo.index().map_err(git_error)?;
                    let Some(entry) = index
                        .get_path(path, 0)
                        .filter(|entry| entry.mode == 0o160000)
                    else {
                        return Ok(None);
                    };
                    let oid = self
                        .repo
                        .submodules()
                        .map_err(git_error)?
                        .into_iter()
                        .find(|submodule| submodule.path() == path)
                        .and_then(|submodule| submodule.workdir_id())
                        .unwrap_or(entry.id);
                    Ok(Some((Some(oid.to_string()), entry.mode)))
                }
                Ok(_) => Err(GitError::Other("File side is not a regular file".into())),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(io_error(error)),
            };
        }
        self.file_entry(source, path)
            .map(|entry| entry.map(|(oid, mode)| (Some(oid.to_string()), mode)))
    }

    /// Read the link itself, never its target. `None` means this is not a symlink.
    fn workdir_link(&self, path: &Path, max_bytes: usize) -> Result<Option<Vec<u8>>, GitError> {
        let root = self
            .workdir()
            .ok_or_else(|| GitError::Other("No working directory".into()))?;
        let path = root.join(path);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error(error)),
        };
        if !metadata.file_type().is_symlink() {
            return Ok(None);
        }
        if metadata.len() > max_bytes as u64 {
            return Err(too_large(max_bytes));
        }
        let target = std::fs::read_link(path).map_err(io_error)?;
        let bytes = target.as_os_str().as_encoded_bytes();
        if bytes.len() > max_bytes {
            return Err(too_large(max_bytes));
        }
        Ok(Some(bytes.to_vec()))
    }

    /// Size without copying content; absent sources and frozen gitlinks have no
    /// blob size. Invalid or unreadable present objects remain errors.
    pub fn file_size(
        &self,
        source: FileReadSource<'_>,
        path: &Path,
    ) -> Result<Option<u64>, GitError> {
        if matches!(source, FileReadSource::Workdir) {
            let root = self
                .workdir()
                .ok_or_else(|| GitError::Other("No working directory".into()))?;
            if let Ok(metadata) = std::fs::symlink_metadata(root.join(path)) {
                if metadata.file_type().is_symlink() {
                    return Ok(Some(metadata.len()));
                }
            }
            return self
                .open_workdir_file(path)?
                .map(|file| {
                    file.metadata()
                        .map(|metadata| metadata.len())
                        .map_err(io_error)
                })
                .transpose();
        }
        self.file_blob_id(source, path)?
            .map(|oid| blob_size(&self.repo, oid).map(|size| size as u64))
            .transpose()
    }

    /// Fill a caller-owned prefix buffer, returning bytes read, without a full
    /// application-owned blob copy. Symlink callers read link bytes, not targets.
    pub fn file_prefix(
        &self,
        source: FileReadSource<'_>,
        path: &Path,
        buffer: &mut [u8],
    ) -> Result<Option<usize>, GitError> {
        if matches!(source, FileReadSource::Workdir) {
            if let Some(bytes) = self.workdir_link(path, usize::MAX)? {
                let count = bytes.len().min(buffer.len());
                buffer[..count].copy_from_slice(&bytes[..count]);
                return Ok(Some(count));
            }
            let Some(mut file) = self.open_workdir_file(path)? else {
                return Ok(None);
            };
            let mut count = 0;
            while count < buffer.len() {
                match file.read(&mut buffer[count..]) {
                    Ok(0) => break,
                    Ok(read) => count += read,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(io_error(error)),
                }
            }
            return Ok(Some(count));
        }
        let Some(oid) = self.file_blob_id(source, path)? else {
            return Ok(None);
        };
        let blob = self.repo.find_blob(oid).map_err(git_error)?;
        let count = blob.content().len().min(buffer.len());
        buffer[..count].copy_from_slice(&blob.content()[..count]);
        Ok(Some(count))
    }

    /// Probe only `prefix.len()` bytes without allocating an application Vec.
    /// Git's compressed/delta object storage can still materialize the blob inside
    /// libgit2; this is not a bound on libgit2's cache or process memory.
    pub fn file_has_prefix(
        &self,
        source: FileReadSource<'_>,
        path: &Path,
        prefix: &[u8],
    ) -> Result<Option<bool>, GitError> {
        if matches!(source, FileReadSource::Workdir) {
            if let Some(bytes) = self.workdir_link(path, usize::MAX)? {
                return Ok(Some(bytes.starts_with(prefix)));
            }
            let Some(mut file) = self.open_workdir_file(path)? else {
                return Ok(None);
            };
            let mut buffer = [0; 1024];
            for expected in prefix.chunks(buffer.len()) {
                let actual = &mut buffer[..expected.len()];
                match file.read_exact(actual) {
                    Ok(()) if actual == expected => {}
                    Ok(()) => return Ok(Some(false)),
                    Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                        return Ok(Some(false));
                    }
                    Err(error) => return Err(io_error(error)),
                }
            }
            return Ok(Some(true));
        }
        let Some(oid) = self.file_blob_id(source, path)? else {
            return Ok(None);
        };
        let blob = self.repo.find_blob(oid).map_err(git_error)?;
        Ok(Some(blob.content().starts_with(prefix)))
    }

    /// Read a whole side only after classification. An oversized or unreadable
    /// side is an error, never absence. The size cap precedes the application
    /// Vec allocation, and workdir growth cannot cross it during the read.
    pub fn file_bytes(
        &self,
        source: FileReadSource<'_>,
        path: &Path,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, GitError> {
        if matches!(source, FileReadSource::Workdir) {
            if let Some(bytes) = self.workdir_link(path, max_bytes)? {
                return Ok(Some(bytes));
            }
            let Some(mut file) = self.open_workdir_file(path)? else {
                return Ok(None);
            };
            let size = file.metadata().map_err(io_error)?.len();
            if size > max_bytes as u64 {
                return Err(too_large(max_bytes));
            }
            return read_bounded(&mut file, size as usize, max_bytes).map(Some);
        }
        let Some(oid) = self.file_blob_id(source, path)? else {
            return Ok(None);
        };
        blob_bytes(&self.repo, oid, max_bytes).map(Some)
    }

    fn open_workdir_file(&self, path: &Path) -> Result<Option<std::fs::File>, GitError> {
        let root = self
            .workdir()
            .ok_or_else(|| GitError::Other("No working directory".into()))?;
        let path = root.join(path);
        // Do not follow symlinks: Git compares their link text, not their target.
        // Reject known special files before open (opening a FIFO could block).
        // Check the opened handle is also regular if the path was replaced.
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => return Err(GitError::Other("File side is not a regular file".into())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error(error)),
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = match options.open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error(error)),
        };
        if !file.metadata().map_err(io_error)?.is_file() {
            return Err(GitError::Other("File side is not a regular file".into()));
        }
        Ok(Some(file))
    }

    fn file_blob_id(
        &self,
        source: FileReadSource<'_>,
        path: &Path,
    ) -> Result<Option<git2::Oid>, GitError> {
        if matches!(source, FileReadSource::ConflictStage { mode: 0o160000, .. }) {
            return Ok(None);
        }
        self.file_entry(source, path)
            .map(|entry| entry.and_then(|(oid, mode)| (mode != 0o160000).then_some(oid)))
    }

    fn file_entry(
        &self,
        source: FileReadSource<'_>,
        path: &Path,
    ) -> Result<Option<(git2::Oid, u32)>, GitError> {
        let commit = match source {
            FileReadSource::ConflictStage { oid, mode } => {
                return git2::Oid::from_str(oid)
                    .map(|oid| Some((oid, mode)))
                    .map_err(git_error);
            }
            FileReadSource::Commit(id) => self
                .repo
                .find_commit(git2::Oid::from_str(&id.0).map_err(git_error)?)
                .map_err(git_error)?,
            FileReadSource::Index | FileReadSource::IndexOrHead => {
                let index = self.repo.index().map_err(git_error)?;
                if let Some(entry) = index.get_path(path, 0) {
                    return Ok(Some((entry.id, entry.mode)));
                }
                if matches!(source, FileReadSource::Index) {
                    return Ok(None);
                }
                if let Some(base) = crate::staging::conflicted_base_entry(&index, path)? {
                    return Ok(base.map(|entry| (entry.id, entry.mode)));
                }
                return self.file_entry(FileReadSource::Head, path);
            }
            FileReadSource::Head => {
                let head = match self.repo.head() {
                    Ok(head) => head,
                    Err(error)
                        if matches!(
                            error.code(),
                            git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
                        ) =>
                    {
                        return Ok(None)
                    }
                    Err(error) => return Err(git_error(error)),
                };
                head.peel_to_commit().map_err(git_error)?
            }
            FileReadSource::Workdir => unreachable!("workdir has no blob OID"),
        };
        let tree = commit.tree().map_err(git_error)?;
        match tree.get_path(path) {
            Ok(entry) => Ok(Some((entry.id(), entry.filemode() as u32))),
            Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(error) => Err(git_error(error)),
        }
    }
}

/// Bounded blob read for live file sources and frozen conflict stages.
fn blob_bytes(
    repo: &git2::Repository,
    oid: git2::Oid,
    max_bytes: usize,
) -> Result<Vec<u8>, GitError> {
    let size = blob_size(repo, oid)?;
    if size > max_bytes {
        return Err(too_large(max_bytes));
    }
    // libgit2 may materialize/cache the entire object; only our copy is bounded.
    let blob = repo.find_blob(oid).map_err(git_error)?;
    if blob.content().len() > max_bytes {
        return Err(too_large(max_bytes));
    }
    Ok(blob.content().to_vec())
}

fn blob_size(repo: &git2::Repository, oid: git2::Oid) -> Result<usize, GitError> {
    let odb = repo.odb().map_err(git_error)?;
    let (size, kind) = odb.read_header(oid).map_err(git_error)?;
    if kind != git2::ObjectType::Blob {
        return Err(GitError::Other("File side is not a blob".into()));
    }
    Ok(size)
}

fn git_error(error: git2::Error) -> GitError {
    GitError::Other(error.message().to_string())
}
fn io_error(error: std::io::Error) -> GitError {
    GitError::Other(error.to_string())
}
fn too_large(max_bytes: usize) -> GitError {
    GitError::Other(format!("File exceeds the {max_bytes}-byte preview limit"))
}

fn read_bounded(
    reader: &mut impl Read,
    size_hint: usize,
    max_bytes: usize,
) -> Result<Vec<u8>, GitError> {
    if size_hint > max_bytes {
        return Err(too_large(max_bytes));
    }
    let mut bytes = Vec::with_capacity(size_hint);
    let mut chunk = [0; 8192];
    loop {
        // At the boundary probe one extra byte on the stack, not in the Vec.
        let count = chunk
            .len()
            .min(max_bytes.saturating_sub(bytes.len()).saturating_add(1));
        let read = match reader.read(&mut chunk[..count]) {
            Ok(0) => return Ok(bytes),
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_error(error)),
        };
        let required = bytes
            .len()
            .checked_add(read)
            .ok_or_else(|| too_large(max_bytes))?;
        if required > max_bytes {
            return Err(too_large(max_bytes));
        }
        if required > bytes.capacity() {
            let capacity = bytes
                .capacity()
                .saturating_mul(2)
                .max(required)
                .min(max_bytes);
            bytes.reserve_exact(capacity - bytes.len());
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitlink_identity_does_not_require_a_local_object() {
        let temp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(temp.path()).unwrap();
        drop(repo);
        let backend = Backend::open(temp.path()).unwrap();
        let oid = "1111111111111111111111111111111111111111";
        let source = FileReadSource::ConflictStage {
            oid,
            mode: 0o160000,
        };
        assert_eq!(
            backend.file_identity(source, Path::new("sub")).unwrap(),
            Some((Some(oid.into()), 0o160000))
        );
        assert_eq!(
            backend.file_bytes(source, Path::new("sub"), 8).unwrap(),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn workdir_symlink_reads_literal_target_with_a_bound() {
        let temp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(temp.path()).unwrap();
        drop(repo);
        let backend = Backend::open(temp.path()).unwrap();
        let target = "missing-target.pdf";
        std::os::unix::fs::symlink(target, temp.path().join("link.pdf")).unwrap();
        let path = Path::new("link.pdf");
        assert_eq!(
            backend
                .file_identity(FileReadSource::Workdir, path)
                .unwrap(),
            Some((None, 0o120000))
        );
        assert_eq!(
            backend
                .file_bytes(FileReadSource::Workdir, path, target.len())
                .unwrap(),
            Some(target.as_bytes().to_vec())
        );
        assert!(backend
            .file_bytes(FileReadSource::Workdir, path, target.len() - 1)
            .is_err());
        assert_eq!(
            backend
                .file_has_prefix(FileReadSource::Workdir, path, b"%PDF-")
                .unwrap(),
            Some(false)
        );
        let mut prefix = [0; 7];
        assert_eq!(
            backend
                .file_prefix(FileReadSource::Workdir, path, &mut prefix)
                .unwrap(),
            Some(7)
        );
        assert_eq!(&prefix, &target.as_bytes()[..7]);
        assert!(!temp.path().join(target).exists());
    }
    #[test]
    fn growth_after_metadata_is_bounded_and_exact_limit_is_accepted() {
        let limit = 10 * 1024 * 1024;
        let bytes = vec![b'x'; limit + 1];
        for hint in [0, 1, limit / 2, limit] {
            let exact = read_bounded(&mut &bytes[..limit], hint, limit).unwrap();
            assert_eq!(exact, bytes[..limit]);
            assert!(exact.capacity() <= limit);
            assert!(read_bounded(&mut bytes.as_slice(), hint, limit).is_err());
        }
        assert_eq!(read_bounded(&mut &b""[..], 0, 0).unwrap(), b"");
        assert!(read_bounded(&mut &b"x"[..], 0, 0).is_err());
    }

    #[test]
    fn rejected_metadata_does_not_start_the_reader() {
        struct Unread;
        impl Read for Unread {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                panic!("oversized metadata must be rejected before reading");
            }
        }
        assert!(read_bounded(&mut Unread, 11, 10).is_err());
    }
}
