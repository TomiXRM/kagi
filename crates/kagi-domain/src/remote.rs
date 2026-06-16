//! Pure remote-host model for SSH-backed repositories (ADR-0089).
//!
//! No I/O, no `git2`, no `gpui`. This layer is the *pure* half of Kagi's
//! remote-over-SSH feature: it knows how to
//!
//! - parse a `[user@]host[:port]` connection string into a [`RemoteHost`];
//! - build the exact `ssh` argument vector (connection options + shell-quoted
//!   remote command) for a given remote command — fully unit-testable as a
//!   `Vec<String>`, with no process spawned here;
//! - parse the textual output of the read-only remote probes (`ls`,
//!   `git rev-parse`, `git log`) into typed values.
//!
//! The process spawning (running the system `ssh` binary) lives in
//! `src/remote/` and calls *this* module for argv construction and parsing,
//! exactly mirroring how `src/git/cli.rs` (network git) pairs with the system
//! `git` binary (ADR-0009). Everything here is reachable from a unit test
//! without a network, a host, or a key.

// ────────────────────────────────────────────────────────────
// Connection policy
// ────────────────────────────────────────────────────────────

/// Seconds passed to ssh's `ConnectTimeout` (the TCP/handshake phase). The
/// whole-command backstop timeout is enforced separately by the I/O layer.
pub const SSH_CONNECT_TIMEOUT_SECS: u32 = 10;

/// Non-interactive, non-hanging ssh options applied to *every* invocation.
///
/// `BatchMode=yes` mirrors `git/cli.rs`'s `GIT_TERMINAL_PROMPT=0`: Kagi never
/// blocks on an interactive auth/host prompt. Authentication is the OS's job
/// (keys, `ssh-agent`, `~/.ssh/config`) — Kagi just runs `ssh` (ADR-0009).
///
/// A consequence: an unknown host (not yet in `known_hosts`) and a host that
/// only offers password auth both *fail fast* rather than prompting. That is
/// the safe default — Kagi never silently accepts a new host key. The error is
/// surfaced to the user, who connects once out-of-band (terminal) to record the
/// key.
fn batch_opts() -> Vec<String> {
    vec![
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        format!("ConnectTimeout={SSH_CONNECT_TIMEOUT_SECS}"),
    ]
}

// ────────────────────────────────────────────────────────────
// RemoteHost
// ────────────────────────────────────────────────────────────

/// A parsed SSH connection target: `[user@]host[:port]`, plus an optional
/// identity (private key) file.
///
/// `host` may be a literal hostname/IP *or* a `~/.ssh/config` alias — both work
/// because the connection ultimately goes through the system `ssh` binary,
/// which resolves the alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteHost {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
}

impl RemoteHost {
    /// Parse `"[user@]host[:port]"`.
    ///
    /// Returns `None` for an empty host, a host beginning with `-` (which `ssh`
    /// would misread as an option), or a non-numeric / zero port. IPv6 literals
    /// are not handled in this MVP (a `host` with a bracketed `[..]` form is
    /// passed through verbatim as the host and any `:port` split is skipped).
    pub fn parse(s: &str) -> Option<RemoteHost> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }

        let (user, rest) = match s.split_once('@') {
            Some((u, r)) if !u.is_empty() => (Some(u.to_string()), r),
            Some(_) => return None, // empty user: "@host"
            None => (None, s),
        };

        // Bracketed IPv6 (`[::1]` / `[::1]:22`) — pass the bracket form through
        // as the host so ssh handles it; only split a trailing `:port`.
        let (host, port) = if let Some(close) = rest.strip_prefix('[').and_then(|_| rest.find(']'))
        {
            let host = rest[..=close].to_string();
            let after = &rest[close + 1..];
            let port = match after.strip_prefix(':') {
                Some(p) => Some(parse_port(p)?),
                None if after.is_empty() => None,
                None => return None,
            };
            (host, port)
        } else {
            match rest.rsplit_once(':') {
                Some((h, p)) => (h.to_string(), Some(parse_port(p)?)),
                None => (rest.to_string(), None),
            }
        };

        if host.is_empty() || host.starts_with('-') {
            return None;
        }

        Some(RemoteHost {
            user,
            host,
            port,
            identity_file: None,
        })
    }

    /// The `[user@]host` destination passed to `ssh`.
    pub fn target(&self) -> String {
        match &self.user {
            Some(u) => format!("{u}@{}", self.host),
            None => self.host.clone(),
        }
    }

    /// A human-readable label for the UI: `user@host:port` (omitting absent
    /// parts).
    pub fn label(&self) -> String {
        let mut s = self.target();
        if let Some(port) = self.port {
            s.push(':');
            s.push_str(&port.to_string());
        }
        s
    }

    /// The ssh connection options (everything *before* the destination):
    /// `BatchMode`/`ConnectTimeout`, plus `-p <port>` and `-i <identity>` when
    /// set. Does not include the destination or the remote command.
    pub fn connection_opts(&self) -> Vec<String> {
        let mut opts = batch_opts();
        if let Some(port) = self.port {
            opts.push("-p".to_string());
            opts.push(port.to_string());
        }
        if let Some(id) = &self.identity_file {
            opts.push("-i".to_string());
            opts.push(id.clone());
        }
        opts
    }

    /// Build the full argument vector passed to the `ssh` binary (everything
    /// *after* the program name) to run `remote_tokens` on the host.
    ///
    /// Layout: `[connection_opts..., destination, remote_command]`, where
    /// `remote_command` is the single shell-quoted string `ssh` forwards to the
    /// remote shell. Connection options precede the destination, so `ssh` stops
    /// option parsing at the destination and treats the trailing element purely
    /// as the command — there is no option-injection path from `remote_tokens`.
    pub fn ssh_invocation(&self, remote_tokens: &[&str]) -> Vec<String> {
        let mut argv = self.connection_opts();
        argv.push(self.target());
        argv.push(join_remote_command(remote_tokens));
        argv
    }
}

fn parse_port(s: &str) -> Option<u16> {
    match s.parse::<u16>() {
        Ok(0) | Err(_) => None,
        Ok(p) => Some(p),
    }
}

// ────────────────────────────────────────────────────────────
// Shell quoting for the remote command
// ────────────────────────────────────────────────────────────

/// POSIX single-quote a string so the *remote* shell receives it as one literal
/// argument. `ssh host cmd` joins the command words with spaces and hands the
/// result to the remote login shell, so every token Kagi sends must be quoted
/// to survive paths with spaces, `$`, `;`, `*`, etc.
///
/// Wraps in single quotes and renders embedded single quotes as `'\''`. Empty
/// input becomes `''`.
pub fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Join remote command tokens into a single shell-safe string (each token
/// [`shell_quote`]d, space-separated).
pub fn join_remote_command(tokens: &[&str]) -> String {
    tokens
        .iter()
        .map(|t| shell_quote(t))
        .collect::<Vec<_>>()
        .join(" ")
}

// ────────────────────────────────────────────────────────────
// Remote path helpers (POSIX, pure)
// ────────────────────────────────────────────────────────────

/// Join a POSIX `base` directory and a child `name` with a single `/`.
pub fn join_path(base: &str, name: &str) -> String {
    if base.is_empty() || base == "/" {
        format!("/{}", name.trim_start_matches('/'))
    } else {
        format!(
            "{}/{}",
            base.trim_end_matches('/'),
            name.trim_start_matches('/')
        )
    }
}

/// The parent of a POSIX absolute path, or `None` at the filesystem root.
pub fn parent_dir(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return None; // already root ("/" or "")
    }
    match trimmed.rsplit_once('/') {
        Some(("", _)) => Some("/".to_string()), // parent of "/foo" is "/"
        Some((parent, _)) => Some(parent.to_string()),
        None => None, // relative leaf with no slash — no known parent
    }
}

// ────────────────────────────────────────────────────────────
// Directory listing (`ls -1ApL`)
// ────────────────────────────────────────────────────────────

/// The kind of a remote directory entry, as far as the directory picker cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirEntryKind {
    Dir,
    File,
}

/// One entry in a remote directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteDirEntry {
    pub name: String,
    pub kind: DirEntryKind,
}

impl RemoteDirEntry {
    pub fn is_dir(&self) -> bool {
        self.kind == DirEntryKind::Dir
    }
}

/// Parse the stdout of `ls -1ApL` into typed entries.
///
/// `-1` one-per-line, `-A` all but `.`/`..`, `-p` append `/` to directories,
/// `-L` classify through symlinks (so a symlink to a directory is navigable).
/// A trailing `/` marks a directory; it is stripped from the stored name.
/// Blank lines are skipped.
pub fn parse_ls(stdout: &str) -> Vec<RemoteDirEntry> {
    stdout
        .lines()
        .filter(|l| !l.trim_end_matches('/').is_empty())
        .map(|line| {
            if let Some(name) = line.strip_suffix('/') {
                RemoteDirEntry {
                    name: name.to_string(),
                    kind: DirEntryKind::Dir,
                }
            } else {
                RemoteDirEntry {
                    name: line.to_string(),
                    kind: DirEntryKind::File,
                }
            }
        })
        .collect()
}

// ────────────────────────────────────────────────────────────
// Repository probe (`git rev-parse --is-inside-work-tree --show-toplevel`)
// ────────────────────────────────────────────────────────────

/// Whether a remote path is inside a Git work tree, and its toplevel if so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoProbe {
    pub is_repo: bool,
    pub toplevel: Option<String>,
}

impl RepoProbe {
    /// The "not a repository" result (also what the I/O layer returns when
    /// `git rev-parse` exits non-zero).
    pub fn not_a_repo() -> RepoProbe {
        RepoProbe {
            is_repo: false,
            toplevel: None,
        }
    }
}

/// Parse the stdout of a successful (`exit 0`) run of
/// `git -C <path> rev-parse --is-inside-work-tree --show-toplevel`.
///
/// Line 1 is `true`/`false`; line 2 (when present) is the absolute toplevel.
pub fn parse_repo_probe(stdout: &str) -> RepoProbe {
    let mut lines = stdout.lines();
    let is_repo = matches!(lines.next().map(str::trim), Some("true"));
    let toplevel = lines
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    RepoProbe {
        is_repo,
        toplevel: if is_repo { toplevel } else { None },
    }
}

// ────────────────────────────────────────────────────────────
// Repository summary (`git log -1 --format=%h%x1f%s%x1f%D`)
// ────────────────────────────────────────────────────────────

/// A one-line read-only summary of a remote repository's HEAD, for the detail
/// panel ("show the remote repo's internals"). Produced by
/// `git -C <path> log -1 --format=%h%x1f%s%x1f%D`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRepoSummary {
    /// Current branch (from `%D`'s `HEAD -> <branch>`), or `None` when detached.
    pub branch: Option<String>,
    /// Abbreviated HEAD commit hash (`%h`).
    pub head_short: String,
    /// HEAD commit subject (`%s`).
    pub summary: String,
}

/// Parse the single `%h\x1f%s\x1f%D` line emitted by the summary command.
///
/// Returns `None` for empty output (an unborn/empty repository has no HEAD
/// commit, so `git log` prints nothing).
pub fn parse_repo_summary(stdout: &str) -> Option<RemoteRepoSummary> {
    let line = stdout.lines().next()?;
    if line.trim().is_empty() {
        return None;
    }
    let mut fields = line.split('\u{1f}');
    let head_short = fields.next()?.trim().to_string();
    let summary = fields.next().unwrap_or("").to_string();
    let refs = fields.next().unwrap_or("");
    let branch = branch_from_refnames(refs);
    Some(RemoteRepoSummary {
        branch,
        head_short,
        summary,
    })
}

/// Extract the checked-out branch from a `%D` ref-name list, e.g.
/// `"HEAD -> main, origin/main"` → `Some("main")`. Detached HEAD (`"HEAD"`,
/// no arrow) → `None`.
fn branch_from_refnames(refs: &str) -> Option<String> {
    refs.split(',')
        .map(str::trim)
        .find_map(|r| r.strip_prefix("HEAD -> "))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_host_variants() {
        assert_eq!(
            RemoteHost::parse("alice@server:2222"),
            Some(RemoteHost {
                user: Some("alice".into()),
                host: "server".into(),
                port: Some(2222),
                identity_file: None,
            })
        );
        let h = RemoteHost::parse("example.com").unwrap();
        assert_eq!(h.user, None);
        assert_eq!(h.host, "example.com");
        assert_eq!(h.port, None);
        // ~/.ssh/config alias passes through as the host.
        assert_eq!(RemoteHost::parse("devbox").unwrap().host, "devbox");
    }

    #[test]
    fn parse_host_rejects_bad_input() {
        assert_eq!(RemoteHost::parse(""), None);
        assert_eq!(RemoteHost::parse("   "), None);
        assert_eq!(RemoteHost::parse("@host"), None);
        assert_eq!(RemoteHost::parse("host:0"), None);
        assert_eq!(RemoteHost::parse("host:notaport"), None);
        assert_eq!(RemoteHost::parse("-oProxyCommand=evil"), None);
    }

    #[test]
    fn host_label_and_target() {
        let h = RemoteHost::parse("bob@10.0.0.1:22").unwrap();
        assert_eq!(h.target(), "bob@10.0.0.1");
        assert_eq!(h.label(), "bob@10.0.0.1:22");
        let h2 = RemoteHost::parse("host").unwrap();
        assert_eq!(h2.target(), "host");
        assert_eq!(h2.label(), "host");
    }

    #[test]
    fn ssh_invocation_layout() {
        let h = RemoteHost {
            user: Some("u".into()),
            host: "h".into(),
            port: Some(2200),
            identity_file: Some("/keys/id_ed25519".into()),
        };
        let argv = h.ssh_invocation(&["git", "-C", "/srv/my repo", "rev-parse"]);
        assert_eq!(
            argv,
            vec![
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "-p",
                "2200",
                "-i",
                "/keys/id_ed25519",
                "u@h",
                // remote command is one shell-quoted element
                "'git' '-C' '/srv/my repo' 'rev-parse'",
            ]
        );
    }

    #[test]
    fn shell_quote_escapes() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
        assert_eq!(shell_quote("$(rm -rf /)"), "'$(rm -rf /)'");
    }

    #[test]
    fn path_helpers() {
        assert_eq!(join_path("/home/u", "proj"), "/home/u/proj");
        assert_eq!(join_path("/home/u/", "proj"), "/home/u/proj");
        assert_eq!(join_path("/", "proj"), "/proj");
        assert_eq!(parent_dir("/home/u/proj"), Some("/home/u".into()));
        assert_eq!(parent_dir("/home"), Some("/".into()));
        assert_eq!(parent_dir("/"), None);
        assert_eq!(parent_dir(""), None);
    }

    #[test]
    fn parse_ls_classifies() {
        let out = "Documents/\n.config/\nnotes.txt\n.bashrc\nproj/\n\n";
        let entries = parse_ls(out);
        assert_eq!(
            entries,
            vec![
                RemoteDirEntry {
                    name: "Documents".into(),
                    kind: DirEntryKind::Dir
                },
                RemoteDirEntry {
                    name: ".config".into(),
                    kind: DirEntryKind::Dir
                },
                RemoteDirEntry {
                    name: "notes.txt".into(),
                    kind: DirEntryKind::File
                },
                RemoteDirEntry {
                    name: ".bashrc".into(),
                    kind: DirEntryKind::File
                },
                RemoteDirEntry {
                    name: "proj".into(),
                    kind: DirEntryKind::Dir
                },
            ]
        );
        assert!(entries[0].is_dir());
        assert!(!entries[2].is_dir());
    }

    #[test]
    fn parse_repo_probe_cases() {
        let p = parse_repo_probe("true\n/home/u/proj\n");
        assert!(p.is_repo);
        assert_eq!(p.toplevel.as_deref(), Some("/home/u/proj"));

        let p = parse_repo_probe("false\n");
        assert!(!p.is_repo);
        assert_eq!(p.toplevel, None);

        assert_eq!(
            RepoProbe::not_a_repo(),
            RepoProbe {
                is_repo: false,
                toplevel: None
            }
        );
    }

    #[test]
    fn parse_repo_summary_cases() {
        let s = parse_repo_summary("a1b2c3d\u{1f}Fix the bug\u{1f}HEAD -> main, origin/main\n")
            .unwrap();
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.head_short, "a1b2c3d");
        assert_eq!(s.summary, "Fix the bug");

        // Detached HEAD: no "HEAD -> X".
        let s = parse_repo_summary("deadbee\u{1f}Some commit\u{1f}HEAD, tag: v1.0").unwrap();
        assert_eq!(s.branch, None);
        assert_eq!(s.head_short, "deadbee");

        // Empty / unborn repo: no output.
        assert_eq!(parse_repo_summary(""), None);
        assert_eq!(parse_repo_summary("\n"), None);
    }
}
