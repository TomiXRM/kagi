//! Persisted Git replay inputs for a full approval, never per-file ownership.
use super::{digest_part, GitError};
use git2::Repository;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

// Only operands, not message/patch/subject payloads, can occupy this buffer.
const MAX_OPERAND_BYTES: usize = 65536;

pub(super) fn digest(repo: &Repository) -> Result<Vec<u8>, GitError> {
    let mut hash = Sha256::new();
    let mut buffer = [0; 8192];
    for (relative, directory) in [
        ("MERGE_HEAD", false),
        ("REBASE_HEAD", false),
        ("CHERRY_PICK_HEAD", false),
        ("REVERT_HEAD", false),
        ("ORIG_HEAD", false),
        ("MERGE_MSG", false),
        ("SQUASH_MSG", false),
        ("MERGE_MODE", false),
        ("MERGE_AUTOSTASH", false),
        ("sequencer", true),
        ("rebase-merge", true),
        ("rebase-apply", true),
        ("refs/rewritten", true),
    ] {
        digest_part(&mut hash, relative.as_bytes());
        let path = repo.path().join(relative);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                let valid = if directory {
                    metadata.is_dir()
                } else {
                    metadata.is_file()
                };
                if !valid {
                    return Err(invalid("operation metadata has an unexpected file type"));
                }
                hash_entry(repo, &path, &metadata, &mut hash, &mut buffer)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                digest_part(&mut hash, b"absent");
            }
            Err(error) => return Err(io_error(error)),
        }
    }
    Ok(hash.finalize().to_vec())
}

fn io_error(error: std::io::Error) -> GitError {
    GitError::Other(format!("operation metadata could not be read: {error}"))
}

fn invalid(message: &str) -> GitError {
    GitError::Other(message.to_owned())
}

/// Only fixed-size child digests are retained; no message, patch or whole
/// state-tree payload is materialized. Names/types/absence are framed inputs.
fn hash_entry(
    repo: &Repository,
    path: &Path,
    metadata: &std::fs::Metadata,
    hash: &mut Sha256,
    buffer: &mut [u8; 8192],
) -> Result<(), GitError> {
    if metadata.is_file() {
        hash_file(repo, path, metadata, hash, buffer)?;
    } else if metadata.is_dir() {
        digest_part(hash, b"directory");
        let mut children: Vec<[u8; 32]> = Vec::new();
        for entry in std::fs::read_dir(path).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let child_path = entry.path();
            let metadata = std::fs::symlink_metadata(&child_path).map_err(io_error)?;
            let mut child = Sha256::new();
            digest_part(&mut child, entry.file_name().as_encoded_bytes());
            hash_entry(repo, &child_path, &metadata, &mut child, buffer)?;
            children.push(child.finalize().into());
        }
        children.sort_unstable();
        hash.update((children.len() as u64).to_be_bytes());
        for child in children {
            digest_part(hash, &child);
        }
        let after = std::fs::symlink_metadata(path).map_err(io_error)?;
        if !after.is_dir()
            || metadata.modified().map_err(io_error)? != after.modified().map_err(io_error)?
        {
            return Err(invalid("operation metadata changed while reading"));
        }
    } else {
        return Err(invalid(
            "operation metadata must be a regular file or directory",
        ));
    }
    Ok(())
}

fn hash_file(
    repo: &Repository,
    path: &Path,
    metadata: &std::fs::Metadata,
    hash: &mut Sha256,
    buffer: &mut [u8; 8192],
) -> Result<(), GitError> {
    digest_part(hash, b"file");
    let file = std::fs::File::open(path).map_err(io_error)?;
    let opened = file.metadata().map_err(io_error)?;
    check_file_metadata(metadata, &opened)?;
    let relative = path.strip_prefix(repo.path()).ok().and_then(Path::to_str);
    let mut todo = match relative {
        Some("rebase-merge/git-rebase-todo" | "sequencer/todo") => Some(TodoInputs::new(repo)?),
        _ => None,
    };
    let scalar = matches!(
        relative,
        Some("rebase-merge/squash-onto" | "rebase-merge/autostash" | "rebase-apply/autostash")
    );
    let mut updates = (relative == Some("rebase-merge/update-refs")).then(|| RefUpdates::new(repo));
    let expected = opened.len();
    if scalar && expected > MAX_OPERAND_BYTES as u64 {
        return Err(invalid("operation commit operand is too long"));
    }
    let mut scalar_bytes = if scalar {
        Vec::with_capacity(expected as usize)
    } else {
        Vec::new()
    };
    hash.update(expected.to_be_bytes());
    // A concurrently growing file must not turn one observation into an
    // unbounded read. The extra byte detects growth rather than truncating it.
    let mut reader = file.take(expected.saturating_add(1));
    let mut read = 0_u64;
    loop {
        let count = reader.read(buffer).map_err(io_error)?;
        if count == 0 {
            break;
        }
        read += count as u64;
        hash.update(&buffer[..count]);
        if let Some(todo) = &mut todo {
            todo.feed(&buffer[..count])?;
        }
        if scalar {
            scalar_bytes.extend_from_slice(&buffer[..count]);
        }
        if let Some(updates) = &mut updates {
            updates.feed(&buffer[..count])?;
        }
    }
    if read != expected {
        return Err(invalid("operation metadata changed while reading"));
    }
    check_file_metadata(&opened, &reader.get_ref().metadata().map_err(io_error)?)?;
    check_file_metadata(&opened, &std::fs::symlink_metadata(path).map_err(io_error)?)?;
    if let Some(todo) = todo {
        digest_part(hash, b"todo-operands");
        digest_part(hash, &todo.finish()?);
    }
    if scalar {
        let name = scalar_bytes.trim_ascii();
        digest_part(hash, b"commit-operand");
        digest_part(hash, name);
        digest_part(hash, commit_oid(repo, name)?.as_bytes());
    }
    if relative == Some("MERGE_AUTOSTASH") {
        let reference = repo.find_reference("MERGE_AUTOSTASH").map_err(git_error)?;
        if reference.symbolic_target_bytes().is_some() {
            return Err(invalid("merge autostash must not be a symbolic reference"));
        }
        digest_part(hash, b"autostash-target");
        digest_part(hash, commit_oid(repo, b"MERGE_AUTOSTASH")?.as_bytes());
    }
    if let Some(updates) = updates {
        digest_part(hash, b"update-ref-targets");
        digest_part(hash, &updates.finish()?);
    }
    Ok(())
}

fn check_file_metadata(
    before: &std::fs::Metadata,
    after: &std::fs::Metadata,
) -> Result<(), GitError> {
    if !after.is_file()
        || before.len() != after.len()
        || before.modified().map_err(io_error)? != after.modified().map_err(io_error)?
    {
        return Err(invalid("operation metadata changed while reading"));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Position {
    Command,
    Ignore,
    End,
    Commit,
    Fixup,
    MergeOption,
    MergeCommit,
    MergeLabels,
    Label,
    Reset,
    NewRoot,
    UpdateRef,
    Exec,
}

/// A streaming operand collector, not a sequencer: Git still resolves commits
/// and executes every instruction. Subjects/exec bodies are never accumulated.
/// Earlier pending labels are produced by the frozen todo, not external refs.
struct TodoInputs<'repo> {
    repo: &'repo Repository,
    hash: Sha256,
    produced: HashSet<[u8; 32]>,
    position: Position,
    token: Vec<u8>,
    comment: Vec<u8>,
    comment_matched: usize,
    checking_comment: bool,
    merge_label_seen: bool,
}

impl<'repo> TodoInputs<'repo> {
    fn new(repo: &'repo Repository) -> Result<Self, GitError> {
        let config = repo.config().map_err(git_error)?;
        let mut comment = b"#".to_vec();
        for name in ["core.commentString", "core.commentChar"] {
            match config.get_string(name) {
                Ok(value) => {
                    if value.is_empty() || value.contains(['\n', '\r']) {
                        return Err(invalid("operation todo comment marker is invalid"));
                    }
                    comment = if value == "auto" {
                        b"#".to_vec()
                    } else {
                        value.into_bytes()
                    };
                    break;
                }
                Err(error) if error.code() == git2::ErrorCode::NotFound => {}
                Err(error) => return Err(git_error(error)),
            }
        }
        let mut hash = Sha256::new();
        digest_part(&mut hash, &comment);
        Ok(Self {
            repo,
            hash,
            produced: HashSet::new(),
            position: Position::Command,
            token: Vec::new(),
            comment,
            comment_matched: 0,
            checking_comment: true,
            merge_label_seen: false,
        })
    }

    fn feed(&mut self, bytes: &[u8]) -> Result<(), GitError> {
        for &byte in bytes {
            if self.checking_comment {
                if self.comment_matched == 0 && matches!(byte, b' ' | b'\t') {
                    continue;
                }
                if self.comment.get(self.comment_matched) == Some(&byte) {
                    self.comment_matched += 1;
                    if self.comment_matched == self.comment.len() {
                        self.position = Position::Ignore;
                        self.checking_comment = false;
                        self.comment_matched = 0;
                    }
                    continue;
                }
                self.checking_comment = false;
                let comment = std::mem::take(&mut self.comment);
                for &matched in &comment[..self.comment_matched] {
                    self.lex(matched)?;
                }
                self.comment = comment;
                self.comment_matched = 0;
            }
            self.lex(byte)?;
        }
        Ok(())
    }

    fn lex(&mut self, byte: u8) -> Result<(), GitError> {
        if byte.is_ascii_whitespace() {
            if matches!(self.position, Position::NewRoot) && self.token.is_empty() {
                return Err(invalid("operation todo reset target is invalid"));
            }
            let new_root = matches!(self.position, Position::Reset) && self.token == b"[new";
            self.end_token()?;
            if new_root && byte != b' ' {
                return Err(invalid("operation todo reset target is invalid"));
            }
            if byte == b'\n' {
                self.end_line()?;
            }
        } else if !matches!(self.position, Position::Ignore) {
            // Git writes 40-byte OIDs and filesystem-sized names here. Refuse
            // an unbounded operand instead of allocating an arbitrary subject.
            if self.token.len() == MAX_OPERAND_BYTES {
                return Err(invalid("operation todo operand is too long"));
            }
            self.token.push(byte);
        }
        Ok(())
    }

    fn end_token(&mut self) -> Result<(), GitError> {
        if self.token.is_empty() {
            return Ok(());
        }
        let mut token = std::mem::take(&mut self.token);
        self.operand(&token)?;
        token.clear();
        self.token = token;
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), GitError> {
        if !matches!(
            self.position,
            Position::Command | Position::Ignore | Position::End | Position::MergeLabels
        ) {
            return Err(invalid("operation todo operand is missing"));
        }
        if matches!(self.position, Position::MergeLabels) && !self.merge_label_seen {
            return Err(invalid("operation todo merge target is missing"));
        }
        self.position = Position::Command;
        self.checking_comment = true;
        self.comment_matched = 0;
        self.merge_label_seen = false;
        Ok(())
    }

    fn operand(&mut self, token: &[u8]) -> Result<(), GitError> {
        match self.position {
            Position::Command => {
                self.position = match token {
                    b"pick" | b"p" | b"revert" | b"edit" | b"e" | b"reword" | b"r" | b"squash"
                    | b"s" | b"drop" | b"d" => Position::Commit,
                    b"fixup" | b"f" => Position::Fixup,
                    b"merge" | b"m" => Position::MergeOption,
                    b"label" | b"l" => Position::Label,
                    b"reset" | b"t" => Position::Reset,
                    b"update-ref" | b"u" => Position::UpdateRef,
                    b"exec" | b"x" => Position::Exec,
                    b"noop" | b"break" | b"b" => Position::End,
                    _ => return Err(invalid("operation todo command is unknown")),
                }
            }
            Position::Commit | Position::MergeCommit => {
                self.bind_commit(token)?;
                self.position = if matches!(self.position, Position::MergeCommit) {
                    Position::MergeLabels
                } else {
                    Position::Ignore
                };
            }
            Position::Fixup | Position::MergeOption => self.option_or_operand(token)?,
            Position::MergeLabels => {
                if token == b"#" {
                    if !self.merge_label_seen {
                        return Err(invalid("operation todo merge target is missing"));
                    }
                    self.position = Position::Ignore;
                } else {
                    self.bind_label(token)?;
                    self.merge_label_seen = true;
                }
            }
            Position::Label => {
                let name = operand_text(token)?;
                if !git2::Reference::is_valid_name(&format!("refs/rewritten/{name}")) {
                    return Err(invalid("operation todo label is invalid"));
                }
                self.produced.insert(Sha256::digest(token).into());
                self.position = Position::End;
            }
            Position::Reset => {
                if token == b"[new" {
                    self.position = Position::NewRoot;
                } else {
                    self.bind_label(token)?;
                    self.position = Position::Ignore;
                }
            }
            Position::NewRoot => {
                if !token.starts_with(b"root]") {
                    return Err(invalid("operation todo reset target is invalid"));
                }
                self.position = Position::Ignore;
            }
            Position::UpdateRef => {
                hash_ref(self.repo, &mut self.hash, operand_text(token)?)?;
                self.position = Position::End;
            }
            Position::Exec => self.position = Position::Ignore,
            Position::End => return Err(invalid("operation todo has unexpected operands")),
            Position::Ignore => {}
        }
        Ok(())
    }

    fn option_or_operand(&mut self, token: &[u8]) -> Result<(), GitError> {
        let merge = matches!(self.position, Position::MergeOption);
        if token.starts_with(b"-C") || token.starts_with(b"-c") {
            if token.len() == 2 {
                self.position = if merge {
                    Position::MergeCommit
                } else {
                    Position::Commit
                };
            } else {
                self.bind_commit(&token[2..])?;
                self.position = if merge {
                    Position::MergeLabels
                } else {
                    Position::Ignore
                };
            }
        } else if merge {
            if token == b"#" {
                return Err(invalid("operation todo merge target is missing"));
            }
            self.bind_label(token)?;
            self.merge_label_seen = true;
            self.position = Position::MergeLabels;
        } else {
            self.bind_commit(token)?;
            self.position = Position::Ignore;
        }
        Ok(())
    }

    fn bind_commit(&mut self, token: &[u8]) -> Result<(), GitError> {
        let oid = commit_oid(self.repo, token)?;
        digest_part(&mut self.hash, b"commit");
        digest_part(&mut self.hash, token);
        digest_part(&mut self.hash, oid.as_bytes());
        Ok(())
    }

    fn bind_label(&mut self, token: &[u8]) -> Result<(), GitError> {
        digest_part(&mut self.hash, b"label");
        digest_part(&mut self.hash, token);
        if self
            .produced
            .contains(&<[u8; 32]>::from(Sha256::digest(token)))
        {
            digest_part(&mut self.hash, b"produced-by-todo");
            return Ok(());
        }
        let rewritten = format!("refs/rewritten/{}", operand_text(token)?);
        if !git2::Reference::is_valid_name(&rewritten)
            || !hash_ref(self.repo, &mut self.hash, &rewritten)?
        {
            self.bind_commit(token)?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<[u8; 32], GitError> {
        // Flush a partial comment prefix or a final line without a newline.
        self.feed(b"\n")?;
        Ok(self.hash.finalize().into())
    }
}

fn commit_oid(repo: &Repository, token: &[u8]) -> Result<git2::Oid, GitError> {
    let name = operand_text(token)?;
    if let Ok(oid) = git2::Oid::from_str(name) {
        if repo
            .odb()
            .map_err(git_error)?
            .read_header(oid)
            .is_ok_and(|(_, kind)| kind == git2::ObjectType::Commit)
        {
            return Ok(oid);
        }
    }
    Ok(crate::revision::resolve_commit_object(repo, name)?.id())
}

fn hash_ref(repo: &Repository, hash: &mut Sha256, name: &str) -> Result<bool, GitError> {
    digest_part(hash, name.as_bytes());
    let reference = match repo.find_reference(name) {
        Ok(reference) => reference,
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            digest_part(hash, b"absent");
            return Ok(false);
        }
        Err(error) => return Err(git_error(error)),
    };
    if let Some(target) = reference.symbolic_target_bytes() {
        digest_part(hash, b"symbolic");
        digest_part(hash, target);
    } else {
        digest_part(hash, b"direct");
    }
    let resolved = reference.resolve().map_err(git_error)?;
    let oid = resolved
        .target()
        .ok_or_else(|| invalid("operation reference is unresolved"))?;
    digest_part(hash, resolved.name_bytes());
    digest_part(hash, oid.as_bytes());
    Ok(true)
}

struct RefUpdates<'repo> {
    repo: &'repo Repository,
    hash: Sha256,
    row: u8,
    line: Vec<u8>,
}

impl<'repo> RefUpdates<'repo> {
    fn new(repo: &'repo Repository) -> Self {
        Self {
            repo,
            hash: Sha256::new(),
            row: 0,
            line: Vec::new(),
        }
    }

    fn feed(&mut self, bytes: &[u8]) -> Result<(), GitError> {
        for &byte in bytes {
            if byte == b'\n' {
                self.end_line()?;
            } else {
                if self.line.len() == MAX_OPERAND_BYTES {
                    return Err(invalid("operation update-ref operand is too long"));
                }
                self.line.push(byte);
            }
        }
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), GitError> {
        let name = operand_text(&self.line)?;
        if self.row == 0 {
            if !git2::Reference::is_valid_name(name) {
                return Err(invalid("operation update-ref name is invalid"));
            }
            hash_ref(self.repo, &mut self.hash, name)?;
        } else {
            git2::Oid::from_str(name).map_err(git_error)?;
        }
        self.line.clear();
        self.row = (self.row + 1) % 3;
        Ok(())
    }

    fn finish(mut self) -> Result<[u8; 32], GitError> {
        if !self.line.is_empty() {
            self.end_line()?;
        }
        if self.row != 0 {
            return Err(invalid("operation update-ref state is incomplete"));
        }
        Ok(self.hash.finalize().into())
    }
}

fn operand_text(token: &[u8]) -> Result<&str, GitError> {
    std::str::from_utf8(token).map_err(|_| invalid("operation todo operand is not UTF-8"))
}

fn git_error(error: git2::Error) -> GitError {
    GitError::Other(error.message().to_owned())
}
