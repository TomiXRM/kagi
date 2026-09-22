//! Which host a remote URL really reaches, when only `ssh` can say.
//!
//! [`repo_identity`] reads the host straight out of the URL and must stay that
//! way: it runs while GitHub JSON is being parsed, where spawning a process per
//! pull request is not an option. That leaves exactly one shape it can only
//! take literally — an `ssh_config` alias, whose real host lives in the user's
//! config and not in the URL. `gh:acme/widgets.git` reads as the host `gh`,
//! matches no GitHub identity, and every operation addressed **by identity**
//! then finds no remote at all: the dead end #706 is about.
//!
//! This is the I/O-aware other half — the same identity, with an alias resolved
//! by asking `ssh -G`, the one authority on what the user's config means, under
//! the same deadline and the same process ownership as every other subprocess
//! kagi runs (#507).
//!
//! Asking is the *only* way an alias is mapped here. A failed, cut-short or
//! unparseable answer is [`HostUnidentified`] and never a fallback to the alias
//! token: calling `gh` `github.com` because it usually is would address a
//! repository nobody named, and then read its answer as though it were about
//! the one that was promised (#701 final review 3).

use std::time::Duration;

use kagi_domain::remote::{parse_effective_ssh_config, RemoteHost};

use super::remote_ref::{owner_repo, repo_identity};

/// The bound `ssh -G` runs under — the same 60 s every other kagi subprocess
/// gets (`GH_TIMEOUT`, `GIT_CLI_TIMEOUT_SECS`).
///
/// `-G` opens no connection, but it does evaluate the whole configuration,
/// including `Match exec` blocks that run a command of the user's choosing. So
/// it is bounded and owned like anything else that can hang, rather than
/// trusted to return.
const SSH_CONFIG_TIMEOUT: Duration = Duration::from_secs(60);

/// Why a remote URL's host could not be named.
///
/// Carried rather than flattened to a miss, because the two are opposite
/// answers: "this remote is not the repository" excludes a candidate, and
/// "this remote could not be identified" excludes nothing at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HostUnidentified {
    /// The `[user@]host[:port]` token exactly as the remote URL spells it.
    pub alias: String,
    /// English, and specific: what was asked, and what came back.
    pub reason: String,
}

impl std::fmt::Display for HostUnidentified {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cannot identify the host behind SSH target \"{}\": {}",
            self.alias, self.reason
        )
    }
}

/// `<host>/<owner>/<repo>` for a remote URL, with an `ssh_config` alias
/// resolved to the host it really means.
///
/// `Ok(None)` is the answer [`repo_identity`] already gives: this URL names no
/// `<owner>/<repo>` on a host — a local path, a `file://` URL. `Err` is the new
/// one and it is not absence: the URL *is* reached over SSH, and what it
/// reaches could not be established, so no identity may be claimed for it.
///
/// Only an SSH URL costs a process. `https://`, `git://`, `file://` and plain
/// paths name their own endpoint, so no configuration can rename it and the
/// pure read is already the whole answer.
pub(crate) fn resolve_repo_identity(
    url: &str,
    config: &git2::Config,
) -> Result<Option<String>, HostUnidentified> {
    let Some((target, path)) = ssh_target(url) else {
        return Ok(repo_identity(url));
    };
    Ok(owner_repo(&effective_host(target, config)?, path))
}

/// Why plain `ssh -G` is not authoritative for this repository.
///
/// Git does not have to reach a remote through the `ssh` on `PATH`:
/// `GIT_SSH_COMMAND`, `core.sshCommand` and `GIT_SSH` each replace it, and
/// [`repo_local_overrides`](crate::cli) deliberately leaves the user's own
/// `core.sshCommand` standing rather than disabling legitimate configuration.
/// A replacement may read a different config file, rewrite the destination, or
/// not be OpenSSH at all — so what `ssh -G` says a target means is then an
/// answer about a different program, and reporting it as this remote's host
/// would be the invention this module exists to refuse.
fn ssh_command_override(config: &git2::Config) -> Option<&'static str> {
    for key in ["GIT_SSH_COMMAND", "GIT_SSH"] {
        if std::env::var_os(key).is_some_and(|value| !value.is_empty()) {
            return Some(key);
        }
    }
    let entry = config.get_entry("core.sshCommand").ok()?;
    // The hardened Git runner replaces repository-chosen commands with `ssh`.
    if matches!(
        entry.level(),
        git2::ConfigLevel::Local | git2::ConfigLevel::Worktree
    ) {
        return None;
    }
    // Report the setting name, never a command that may embed credentials.
    if entry.value().is_ok_and(|value| value.trim().is_empty()) {
        None
    } else {
        Some("core.sshCommand")
    }
}

/// The `[user@]host[:port]` and repository path of a URL git reaches over SSH,
/// in either shape it accepts: the explicit `ssh://` scheme, and the scp-like
/// `[user@]host:path` that has no scheme at all.
///
/// The scp shape is the ambiguous one — `/srv/git/acme:widgets.git` is a local
/// path with a colon in it — so it is only read as a remote when the part
/// before the colon could be a host and the part after names a path within one.
fn ssh_target(url: &str) -> Option<(&str, &str)> {
    if let Some(rest) = url.strip_prefix("ssh://") {
        let (authority, path) = rest.split_once('/')?;
        return (!authority.is_empty()).then_some((authority, path));
    }
    if url.contains("://") {
        return None;
    }
    let (authority, path) = url.split_once(':')?;
    (!authority.is_empty() && !authority.contains('/')).then_some((authority, path))
}

/// What the user's `ssh_config` makes of an SSH target, read from `ssh -G`.
///
/// `ssh` evaluates its own configuration: `Host`/`Match` precedence, `%`
/// expansion, included files. Re-implementing that to read `~/.ssh/config`
/// would be a second, differently-wrong answer to a question the system
/// already answers exactly — so the system is asked, and
/// [`parse_effective_ssh_config`] stays the only parser of its output.
///
/// The invocation reaches no host: `-G` prints the effective configuration and
/// exits. It carries the same non-interactive options as every other kagi ssh
/// ([`RemoteHost::connection_opts`]), and nothing is interpolated into a
/// shell. The destination is checked twice over before it is passed: it must
/// parse as a `[user@]host[:port]`, it must survive [`check_operand`] — the
/// same leading-dash reject every name read out of a repository gets (#291) —
/// and it still travels after `--`, so `ssh` could not read it as an option
/// even if one of those ever let something through.
///
/// [`check_operand`]: crate::cli::check_operand
fn effective_host(target: &str, config: &git2::Config) -> Result<String, HostUnidentified> {
    let unidentified = |reason: String| HostUnidentified {
        alias: target.to_string(),
        reason,
    };
    if let Some(replacement) = ssh_command_override(config) {
        return Err(unidentified(format!(
            "git reaches this remote through {replacement}, not the ssh whose \
             configuration could be read"
        )));
    }
    let host = RemoteHost::parse(target)
        .ok_or_else(|| unidentified("not a usable [user@]host[:port] target".to_string()))?;
    let destination = host.target();
    crate::cli::check_operand("ssh target", &destination)
        .map_err(|error| unidentified(error.to_string()))?;
    if destination.chars().any(unusable_in_a_target) {
        return Err(unidentified(
            "SSH target holds whitespace or a control character".to_string(),
        ));
    }
    let mut argv = vec!["-G".to_string()];
    argv.extend(host.connection_opts());
    argv.extend(["--".to_string(), destination]);

    let mut command = std::process::Command::new(ssh_program());
    command.args(&argv).env("LC_ALL", "C");
    let run = crate::proc::run_child(&mut command, SSH_CONFIG_TIMEOUT, None)
        .map_err(|error| unidentified(format!("ssh could not be started: {error}")))?;
    match &run.status {
        Ok(0) => {}
        Ok(code) => {
            return Err(unidentified(format!(
                "ssh -G exited {code}: {}",
                run.stderr_lossy().trim()
            )))
        }
        Err(stop) => return Err(unidentified(format!("ssh -G {stop}"))),
    }
    // A prefix of the effective configuration is not the effective
    // configuration: the `hostname` line may be the one that was cut off.
    if let Err(io) = &run.io {
        return Err(unidentified(format!("ssh -G {io}")));
    }
    let hostname = parse_effective_ssh_config(&run.stdout_lossy())
        .map_err(|error| unidentified(error.to_string()))?
        .hostname;
    // An identity is built by joining this with the path, so a host holding a
    // separator, a leading dash or a control character would not survive that
    // as itself — and must not be reported as though it had.
    if hostname.is_empty()
        || crate::cli::is_flag_like(&hostname)
        || hostname
            .chars()
            .any(|c| c == '/' || unusable_in_a_target(c))
    {
        return Err(unidentified(format!(
            "ssh -G named an unusable host: {hostname:?}"
        )));
    }
    Ok(hostname)
}

fn unusable_in_a_target(c: char) -> bool {
    c.is_whitespace() || c.is_control()
}

/// The `ssh` binary this process asks: the system one, found on `PATH`.
#[cfg(not(test))]
fn ssh_program() -> &'static str {
    "ssh"
}

// In a test it is a per-thread stand-in, so a fixture that must never reach a
// real host also cannot leak into a sibling test running beside it — which
// `PATH`, being process-wide, cannot promise.
#[cfg(test)]
thread_local! {
    static SSH_PROGRAM: std::cell::RefCell<std::path::PathBuf> =
        std::cell::RefCell::new(std::path::PathBuf::from("ssh"));
}

#[cfg(test)]
fn ssh_program() -> std::path::PathBuf {
    SSH_PROGRAM.with(|program| program.borrow().clone())
}

#[cfg(test)]
pub(crate) fn set_ssh_program_for_test(path: &std::path::Path) {
    SSH_PROGRAM.with(|program| *program.borrow_mut() = path.to_path_buf());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real, writable configuration. `core.sshCommand` is pinned empty at
    /// repository level, so the developer's own global setting cannot decide
    /// what these tests are about.
    fn fixture_config() -> (tempfile::TempDir, git2::Config) {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("core.sshCommand", "").unwrap();
        (dir, config)
    }

    /// A URL that names its own endpoint is answered without asking anything:
    /// the `ssh` it would have to run does not exist here, so a spawn would be
    /// an error rather than a silent cost.
    #[test]
    fn a_url_that_names_its_own_host_is_identified_without_ssh() {
        set_ssh_program_for_test(std::path::Path::new(
            "/nonexistent/kagi-must-not-run-this-ssh",
        ));
        let (_dir, config) = fixture_config();
        for (url, identity) in [
            (
                "https://github.com/acme/widgets.git",
                Some("github.com/acme/widgets"),
            ),
            (
                "https://ghe.example/acme/widgets.git",
                Some("ghe.example/acme/widgets"),
            ),
            (
                "git://github.com/acme/widgets.git",
                Some("github.com/acme/widgets"),
            ),
            ("file:///srv/git/acme/widgets.git", None),
            ("/srv/git/acme/widgets.git", None),
        ] {
            assert_eq!(
                resolve_repo_identity(url, &config).unwrap().as_deref(),
                identity,
                "{url}"
            );
        }
        // The same stand-in proves the SSH branch does spawn: it is the one
        // shape whose host is not in the URL.
        assert!(resolve_repo_identity("git@gh:acme/widgets.git", &config).is_err());
        assert!(resolve_repo_identity("git@gh:widgets.git", &config).is_err());
    }

    /// `core.sshCommand` means git does not reach the remote through the `ssh`
    /// whose configuration was read, so no host may be claimed from it.
    #[test]
    fn a_replaced_ssh_command_refuses_rather_than_reporting_another_program() {
        set_ssh_program_for_test(std::path::Path::new(
            "/nonexistent/kagi-must-not-run-this-ssh",
        ));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("global-config");
        std::fs::write(
            &path,
            "[core]\nsshCommand = sshpass -p sentinel-secret ssh\n",
        )
        .unwrap();
        let mut config = git2::Config::new().unwrap();
        config
            .add_file(&path, git2::ConfigLevel::Global, true)
            .unwrap();
        let error = resolve_repo_identity("git@gh:acme/widgets.git", &config).unwrap_err();
        assert_eq!(error.alias, "git@gh");
        assert!(error.reason.contains("core.sshCommand"), "{}", error.reason);
        assert!(
            !error.reason.contains("sentinel-secret"),
            "command credentials must not enter notices or the audit log"
        );
    }

    /// The scp-like shape is only a remote when it could be one.
    #[test]
    fn ssh_targets_are_told_apart_from_paths_and_other_schemes() {
        assert_eq!(
            ssh_target("git@gh:acme/widgets.git"),
            Some(("git@gh", "acme/widgets.git"))
        );
        assert_eq!(
            ssh_target("ssh://git@gh:2222/acme/widgets.git"),
            Some(("git@gh:2222", "acme/widgets.git"))
        );
        for url in [
            "https://github.com/acme/widgets.git",
            "file:///srv/git/acme/widgets.git",
            "/srv/git/acme:widgets.git",
            "/srv/git/widgets.git",
            "widgets.git",
        ] {
            assert_eq!(ssh_target(url), None, "{url}");
        }
    }
}
