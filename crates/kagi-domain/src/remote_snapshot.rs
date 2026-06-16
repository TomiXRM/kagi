//! Pure parsers that turn a remote repository's `git` output into the same
//! domain types the local (`git2`) snapshot produces (ADR-0089, Phase 2).
//!
//! No I/O, no `git2`. The I/O layer (`kagi::remote`) runs the `git` commands
//! over SSH with the format strings defined here, then calls these parsers and
//! assembles a `kagi::git::RepoSnapshot` from the results — so the existing
//! graph/diff views can render a *remote* repo unchanged (the snapshot is the
//! single thing the UI renders).
//!
//! ## Wire format
//!
//! Records and fields are separated by ASCII control characters that never
//! occur in commit metadata: **unit separator** `0x1F` between fields and
//! **record separator** `0x1E` between `git log` commits. The format constants
//! ([`LOG_FORMAT`], [`BRANCH_FORMAT`], …) embed these so the same delimiters are
//! used on the wire and in the parser — keeping the two in lock-step and the
//! parsers unit-testable from a literal string.

use std::path::PathBuf;

use crate::commit::{Commit, CommitId, Signature};
use crate::head::Head;
use crate::refs::{Branch, RemoteBranch, Stash, Tag, UpstreamInfo};
use crate::status::{ChangeKind, FileStatus, WorkingTreeStatus};

/// ASCII unit separator — between fields of one record.
pub const US: char = '\u{1f}';
/// ASCII record separator — between `git log` commit records.
pub const RS: char = '\u{1e}';

// ── git format strings (embed the separators above) ─────────────

/// `git log --pretty=format:` spec: hash, parents, author (name/email/time),
/// committer (name/email/time), raw body — `US`-separated, `RS`-terminated.
/// `%B` (raw body) is last so its embedded newlines can't be confused with a
/// field break.
pub const LOG_FORMAT: &str = "%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%cn%x1f%ce%x1f%ct%x1f%B%x1e";

/// `git for-each-ref refs/heads` spec: short name, tip oid, upstream short
/// name, upstream track (`[ahead N, behind M]`).
pub const BRANCH_FORMAT: &str =
    "%(refname:short)\u{1f}%(objectname)\u{1f}%(upstream:short)\u{1f}%(upstream:track)";

/// `git for-each-ref refs/remotes` spec: short name (`origin/main`) + tip oid.
pub const REMOTE_BRANCH_FORMAT: &str = "%(refname:short)\u{1f}%(objectname)";

/// `git for-each-ref refs/tags` spec: name, oid, peeled oid (`*objectname`, set
/// for annotated tags). The peeled oid is preferred so annotated tags resolve
/// to the commit they point at.
pub const TAG_FORMAT: &str = "%(refname:short)\u{1f}%(objectname)\u{1f}%(*objectname)";

/// `git stash list --format=` spec: selector (`stash@{0}`), stash commit oid,
/// its parents, subject.
pub const STASH_FORMAT: &str = "%gd\u{1f}%H\u{1f}%P\u{1f}%gs";

// ── HEAD ────────────────────────────────────────────────────────

/// Derive [`Head`] from the two cheap probes the I/O layer runs:
/// `git symbolic-ref -q --short HEAD` (the branch, if HEAD is attached) and
/// `git rev-parse -q --verify HEAD` (the commit, if one exists).
///
/// | branch | commit | result |
/// |--------|--------|--------|
/// | `Some` | `Some` | `Attached` |
/// | `Some` | `None` | `Unborn` (branch with no commits yet) |
/// | `None` | `Some` | `Detached` |
/// | `None` | `None` | `Unborn { "HEAD" }` (degenerate) |
pub fn head_from(branch: Option<&str>, commit: Option<&str>) -> Head {
    let branch = branch.map(str::trim).filter(|s| !s.is_empty());
    let commit = commit.map(str::trim).filter(|s| !s.is_empty());
    match (branch, commit) {
        (Some(b), Some(c)) => Head::Attached {
            branch: b.to_string(),
            target: c.to_string(),
        },
        (Some(b), None) => Head::Unborn {
            branch: b.to_string(),
        },
        (None, Some(c)) => Head::Detached {
            target: c.to_string(),
        },
        (None, None) => Head::Unborn {
            branch: "HEAD".to_string(),
        },
    }
}

// ── Commits ─────────────────────────────────────────────────────

/// Parse the [`LOG_FORMAT`] output of `git log` into commits (input order is
/// preserved — the caller passes `--topo-order`).
pub fn parse_commits(stdout: &str) -> Vec<Commit> {
    stdout.split(RS).filter_map(parse_commit_record).collect()
}

fn parse_commit_record(record: &str) -> Option<Commit> {
    // `--pretty=format:` joins records with a newline; after splitting on `RS`
    // every record but the first carries that leading newline — strip it.
    let record = record.trim_start_matches(['\n', '\r']);
    if record.is_empty() {
        return None;
    }
    // `splitn(9)` so the body (field 9) keeps any stray separators/newlines.
    let mut f = record.splitn(9, US);
    let id = CommitId(f.next()?.trim().to_string());
    if id.0.is_empty() {
        return None;
    }
    let parents = f
        .next()?
        .split_whitespace()
        .map(|p| CommitId(p.to_string()))
        .collect();
    let author = Signature {
        name: f.next()?.to_string(),
        email: f.next()?.to_string(),
        time: f.next()?.trim().parse().unwrap_or(0),
    };
    let committer = Signature {
        name: f.next()?.to_string(),
        email: f.next()?.to_string(),
        time: f.next()?.trim().parse().unwrap_or(0),
    };
    let message = f.next().unwrap_or("").to_string();
    let summary = message.lines().next().unwrap_or("").trim_end().to_string();
    Some(Commit {
        id,
        parents,
        author,
        committer,
        summary,
        message,
    })
}

// ── Branches ────────────────────────────────────────────────────

/// Parse [`BRANCH_FORMAT`] output (`git for-each-ref refs/heads`) into local
/// branches with upstream ahead/behind.
pub fn parse_local_branches(stdout: &str) -> Vec<Branch> {
    let mut branches: Vec<Branch> = stdout.lines().filter_map(parse_branch_line).collect();
    branches.sort_by(|a, b| a.name.cmp(&b.name));
    branches
}

fn parse_branch_line(line: &str) -> Option<Branch> {
    if line.trim().is_empty() {
        return None;
    }
    let mut f = line.split(US);
    let name = f.next()?.to_string();
    let target = CommitId(f.next()?.trim().to_string());
    let upstream_short = f.next().unwrap_or("").trim();
    let track = f.next().unwrap_or("");
    if name.is_empty() || target.0.is_empty() {
        return None;
    }
    let upstream = if upstream_short.is_empty() {
        None
    } else {
        let (ahead, behind) = parse_track(track);
        Some(UpstreamInfo {
            remote_branch: upstream_short.to_string(),
            ahead,
            behind,
        })
    };
    Some(Branch {
        name,
        target,
        upstream,
    })
}

/// Parse `%(upstream:track)`, e.g. `"[ahead 2, behind 1]"`, `"[ahead 3]"`,
/// `"[gone]"`, or `""`, into `(ahead, behind)`.
fn parse_track(track: &str) -> (usize, usize) {
    let inner = track.trim().trim_start_matches('[').trim_end_matches(']');
    let mut ahead = 0;
    let mut behind = 0;
    for part in inner.split(',') {
        let mut it = part.split_whitespace();
        match (it.next(), it.next()) {
            (Some("ahead"), Some(n)) => ahead = n.parse().unwrap_or(0),
            (Some("behind"), Some(n)) => behind = n.parse().unwrap_or(0),
            _ => {}
        }
    }
    (ahead, behind)
}

// ── Remote branches ─────────────────────────────────────────────

/// Parse [`REMOTE_BRANCH_FORMAT`] output (`git for-each-ref refs/remotes`),
/// excluding the symbolic `*/HEAD` aliases, splitting `origin/main` into
/// `remote` + `name`.
pub fn parse_remote_branches(stdout: &str) -> Vec<RemoteBranch> {
    let mut out: Vec<RemoteBranch> = stdout
        .lines()
        .filter_map(parse_remote_branch_line)
        .collect();
    out.sort_by(|a, b| a.remote.cmp(&b.remote).then(a.name.cmp(&b.name)));
    out
}

fn parse_remote_branch_line(line: &str) -> Option<RemoteBranch> {
    if line.trim().is_empty() {
        return None;
    }
    let mut f = line.split(US);
    let full = f.next()?.trim();
    let target = CommitId(f.next()?.trim().to_string());
    if full.ends_with("/HEAD") || target.0.is_empty() {
        return None;
    }
    let (remote, name) = full.split_once('/')?;
    Some(RemoteBranch {
        remote: remote.to_string(),
        name: name.to_string(),
        target,
    })
}

// ── Tags ────────────────────────────────────────────────────────

/// Parse [`TAG_FORMAT`] output (`git for-each-ref refs/tags`), preferring the
/// peeled oid (annotated tags) over the tag-object oid.
pub fn parse_tags(stdout: &str) -> Vec<Tag> {
    let mut tags: Vec<Tag> = stdout.lines().filter_map(parse_tag_line).collect();
    tags.sort_by(|a, b| a.name.cmp(&b.name));
    tags
}

fn parse_tag_line(line: &str) -> Option<Tag> {
    if line.trim().is_empty() {
        return None;
    }
    let mut f = line.split(US);
    let name = f.next()?.to_string();
    let obj = f.next().unwrap_or("").trim();
    let peeled = f.next().unwrap_or("").trim();
    let target = if !peeled.is_empty() { peeled } else { obj };
    if name.is_empty() || target.is_empty() {
        return None;
    }
    Some(Tag {
        name,
        target: CommitId(target.to_string()),
    })
}

// ── Stashes ─────────────────────────────────────────────────────

/// Parse [`STASH_FORMAT`] output (`git stash list`). The stash commit's first
/// parent is its base (where it sprouted), for the graph (ADR-0088).
pub fn parse_stashes(stdout: &str) -> Vec<Stash> {
    stdout.lines().filter_map(parse_stash_line).collect()
}

fn parse_stash_line(line: &str) -> Option<Stash> {
    if line.trim().is_empty() {
        return None;
    }
    let mut f = line.split(US);
    let selector = f.next()?.trim();
    let oid = f.next()?.trim();
    let parents = f.next().unwrap_or("");
    let message = f.next().unwrap_or("").to_string();
    if oid.is_empty() {
        return None;
    }
    let index = stash_index(selector)?;
    let base = parents
        .split_whitespace()
        .next()
        .map(|p| CommitId(p.to_string()));
    Some(Stash {
        index,
        message,
        target: CommitId(oid.to_string()),
        base,
    })
}

/// `"stash@{3}"` → `Some(3)`.
fn stash_index(selector: &str) -> Option<usize> {
    let inner = selector.strip_prefix("stash@{")?.strip_suffix('}')?;
    inner.parse().ok()
}

// ── Working-tree status (`git status --porcelain`) ──────────────

/// Parse `git status --porcelain` (v1) into a [`WorkingTreeStatus`].
///
/// Each line is `XY PATH` (or `XY ORIG -> PATH` for renames); `X` is the index
/// (staged) state, `Y` the worktree (unstaged) state. `??` is untracked;
/// unmerged combinations (`U?`/`?U`/`DD`/`AA`) are conflicts. A file changed in
/// both index and worktree (e.g. `MM`) appears in *both* `staged` and
/// `unstaged`, matching the local backend.
pub fn parse_status_v1(text: &str) -> WorkingTreeStatus {
    let mut status = WorkingTreeStatus::default();
    for line in text.lines() {
        if line.len() < 3 {
            continue;
        }
        let bytes = line.as_bytes();
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        let rest = &line[3..];

        if x == '?' && y == '?' {
            status.untracked.push(PathBuf::from(rest));
            continue;
        }
        if is_conflict(x, y) {
            let path = rest.split_once(" -> ").map(|(_, p)| p).unwrap_or(rest);
            status.conflicted.push(PathBuf::from(path));
            continue;
        }
        // Rename arrow: `ORIG -> PATH`.
        let (orig, path) = match rest.split_once(" -> ") {
            Some((o, p)) => (Some(PathBuf::from(o)), p),
            None => (None, rest),
        };
        if let Some(change) = change_for(x, &orig) {
            status.staged.push(FileStatus {
                path: PathBuf::from(path),
                change,
            });
        }
        if let Some(change) = change_for(y, &orig) {
            status.unstaged.push(FileStatus {
                path: PathBuf::from(path),
                change,
            });
        }
    }
    status
}

fn is_conflict(x: char, y: char) -> bool {
    x == 'U' || y == 'U' || (x == 'D' && y == 'D') || (x == 'A' && y == 'A')
}

/// Map a single porcelain status code to a [`ChangeKind`]; `' '`/`'?'` → `None`.
fn change_for(code: char, orig: &Option<PathBuf>) -> Option<ChangeKind> {
    match code {
        'M' => Some(ChangeKind::Modified),
        'A' => Some(ChangeKind::Added),
        'D' => Some(ChangeKind::Deleted),
        'T' => Some(ChangeKind::TypeChange),
        'C' => Some(ChangeKind::Modified),
        'R' => Some(ChangeKind::Renamed {
            from: orig.clone().unwrap_or_default(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_cases() {
        assert_eq!(
            head_from(Some("main"), Some("abc")),
            Head::Attached {
                branch: "main".into(),
                target: "abc".into()
            }
        );
        assert_eq!(
            head_from(Some("main"), None),
            Head::Unborn {
                branch: "main".into()
            }
        );
        assert_eq!(
            head_from(None, Some("abc")),
            Head::Detached {
                target: "abc".into()
            }
        );
        assert_eq!(
            head_from(Some(""), Some(" ")),
            Head::Unborn {
                branch: "HEAD".into()
            }
        );
    }

    #[test]
    fn commits_with_merge_and_multiline_body() {
        // Two records: a merge (two parents) then a root commit. Bodies contain
        // newlines; records are RS-separated, fields US-separated.
        let out = format!(
            "m\u{1f}p1 p2\u{1f}Alice\u{1f}a@x\u{1f}1000\u{1f}Alice\u{1f}a@x\u{1f}1001\u{1f}Merge branch\n\nlong body\u{1e}\nr\u{1f}\u{1f}Bob\u{1f}b@x\u{1f}900\u{1f}Bob\u{1f}b@x\u{1f}900\u{1f}root\u{1e}"
        );
        let commits = parse_commits(&out);
        assert_eq!(commits.len(), 2);

        assert_eq!(commits[0].id.0, "m");
        assert_eq!(
            commits[0].parents,
            vec![CommitId("p1".into()), CommitId("p2".into())]
        );
        assert_eq!(commits[0].author.name, "Alice");
        assert_eq!(commits[0].author.time, 1000);
        assert_eq!(commits[0].committer.time, 1001);
        assert_eq!(commits[0].summary, "Merge branch");
        assert!(commits[0].message.contains("long body"));

        assert_eq!(commits[1].id.0, "r");
        assert!(commits[1].parents.is_empty());
        assert_eq!(commits[1].summary, "root");
    }

    #[test]
    fn commits_empty() {
        assert!(parse_commits("").is_empty());
        assert!(parse_commits("\n").is_empty());
    }

    #[test]
    fn local_branches_with_and_without_upstream() {
        let out = "main\u{1f}aaa\u{1f}origin/main\u{1f}[ahead 2, behind 1]\n\
                   feat\u{1f}bbb\u{1f}\u{1f}\n\
                   gone\u{1f}ccc\u{1f}origin/gone\u{1f}[gone]";
        let b = parse_local_branches(out);
        assert_eq!(b.len(), 3);
        // sorted by name: feat, gone, main
        assert_eq!(b[0].name, "feat");
        assert_eq!(b[0].upstream, None);
        assert_eq!(b[2].name, "main");
        let up = b[2].upstream.as_ref().unwrap();
        assert_eq!(up.remote_branch, "origin/main");
        assert_eq!((up.ahead, up.behind), (2, 1));
        // gone: has upstream name but no ahead/behind
        assert_eq!(b[1].upstream.as_ref().unwrap().ahead, 0);
    }

    #[test]
    fn remote_branches_skip_head() {
        let out = "origin/HEAD\u{1f}xxx\norigin/main\u{1f}aaa\nupstream/dev\u{1f}bbb";
        let r = parse_remote_branches(out);
        assert_eq!(r.len(), 2);
        assert_eq!(
            (r[0].remote.as_str(), r[0].name.as_str()),
            ("origin", "main")
        );
        assert_eq!(
            (r[1].remote.as_str(), r[1].name.as_str()),
            ("upstream", "dev")
        );
    }

    #[test]
    fn tags_prefer_peeled() {
        // lightweight: peeled empty → use objectname; annotated: use peeled.
        let out = "v1.0\u{1f}light\u{1f}\nv2.0\u{1f}tagobj\u{1f}peeledcommit";
        let t = parse_tags(out);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].name, "v1.0");
        assert_eq!(t[0].target.0, "light");
        assert_eq!(t[1].target.0, "peeledcommit");
    }

    #[test]
    fn status_v1_parse() {
        let out = " M src/a.rs\n\
                   M  src/b.rs\n\
                   MM src/c.rs\n\
                   A  added.rs\n\
                   ?? new.txt\n\
                   UU conflict.rs\n\
                   R  old.rs -> renamed.rs";
        let s = parse_status_v1(out);
        // staged: b (M), c (M), added (A), renamed (R)
        assert_eq!(s.staged.len(), 4);
        // unstaged: a (M), c (M)
        assert_eq!(s.unstaged.len(), 2);
        assert_eq!(s.untracked, vec![PathBuf::from("new.txt")]);
        assert_eq!(s.conflicted, vec![PathBuf::from("conflict.rs")]);
        assert!(s.is_dirty());
        // rename carries the old path on the staged side
        let renamed = s
            .staged
            .iter()
            .find(|f| f.path == PathBuf::from("renamed.rs"))
            .unwrap();
        assert_eq!(
            renamed.change,
            ChangeKind::Renamed {
                from: PathBuf::from("old.rs")
            }
        );
    }

    #[test]
    fn stashes_parse_index_and_base() {
        let out = "stash@{0}\u{1f}deadbee\u{1f}base0 p2\u{1f}WIP on main: x\n\
                   stash@{1}\u{1f}feedfac\u{1f}base1\u{1f}On feat: y";
        let s = parse_stashes(out);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].index, 0);
        assert_eq!(s[0].target.0, "deadbee");
        assert_eq!(s[0].base, Some(CommitId("base0".into())));
        assert_eq!(s[0].message, "WIP on main: x");
        assert_eq!(s[1].index, 1);
    }
}
