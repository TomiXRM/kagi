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
//! unusable answer is [`HostUnidentified`] and never a fallback to the alias
//! token: calling `gh` `github.com` because it usually is would address a
//! repository nobody named, and then read its answer as though it were about
//! the one that was promised (#701 final review 3).
//!
//! Two things this module deliberately does not do:
//!
//! - It does not **validate** a configuration. Every refusal here is about
//!   naming a host, never about `ControlMaster`, a `ProxyJump`, an agent-only
//!   identity or an unexpanded `known_hosts` token — ordinary settings that
//!   remotes are reached through every day, and none of which stop `ssh -G`
//!   from saying `hostname github.com` (#706 review 1).
//! - It does not decide whether a remote can be **reached**. When git reaches
//!   a remote through a replacement for `ssh` this module cannot name the host
//!   behind an alias, but git can still address it perfectly well — so that
//!   refusal is marked `SshReplaced` and kept apart from the
//!   one where nothing has an address at all (#706 review 2).

use std::time::Duration;

use kagi_domain::remote::RemoteHost;

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
    /// The host token, without URL userinfo.
    pub alias: String,
    /// English, and specific: what was asked, and what came back — carrying no
    /// credential, whoever produced the words.
    pub reason: String,
    kind: Unidentified,
}

/// Which half of the question failed — and therefore whether anything else
/// could still put it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unidentified {
    /// `ssh` was asked and no host came back: it failed, it was cut short, or
    /// what it named cannot be a host — and a target that is not a usable
    /// `[user@]host[:port]` in the first place belongs here too, since there
    /// was nothing to ask about. Nothing addresses this remote.
    Unresolved,
    /// Git reaches this remote through a replacement for `ssh`
    /// (`GIT_SSH_COMMAND`, `GIT_SSH`, a user-level `core.sshCommand`), so the
    /// configuration read by the `ssh` on `PATH` is not the mapping in force.
    /// The alias cannot be named here — but git addresses the remote through
    /// that very replacement, so the remote itself is not out of reach.
    SshReplaced,
}

impl HostUnidentified {
    /// Whether this remote has no address at all, as opposed to one this
    /// module may not name.
    ///
    /// Only the first may be reported as unobservable: a released write scope
    /// is permanent, and "kagi could not read your ssh config" is not the same
    /// claim as "there is no remote here to ask" (#706 review 2).
    pub(crate) fn unaddressable(&self) -> bool {
        matches!(self.kind, Unidentified::Unresolved)
    }
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
/// not be OpenSSH at all — so what `ssh -G` says an *alias* means is then an
/// answer about a different program.
///
/// It says nothing about a URL that spells its host out, which is why this is
/// consulted per candidate rather than used to refuse a repository wholesale
/// (see [`effective_host`]).
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
/// already answers exactly — so the system is asked, and only the one line
/// that answers it is read ([`reported_hostname`]).
///
/// A URL that spells its own host is not asked about at all when git reaches
/// remotes through a replacement for `ssh`: `github.com` is `github.com`
/// whatever transports it, and refusing it there cost the PR fetch and the
/// reconcile read a remote they had always identified (#706 review 2). Only an
/// alias-shaped target — one that cannot be a hostname on its own — is refused,
/// and it is refused as `SshReplaced`, never as an address that
/// does not exist.
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
    let seen = Redacted::new(target);
    let unidentified = |kind: Unidentified, reason: String| HostUnidentified {
        alias: seen.alias.to_string(),
        reason: seen.scrub(&reason),
        kind,
    };
    let unresolved = |reason: String| unidentified(Unidentified::Unresolved, reason);
    let host = RemoteHost::parse(target)
        .ok_or_else(|| unresolved("not a usable [user@]host[:port] target".to_string()))?;
    if let Some(replacement) = ssh_command_override(config) {
        if names_itself(&host.host) {
            return Ok(host.host);
        }
        return Err(unidentified(
            Unidentified::SshReplaced,
            format!(
                "git reaches this remote through {replacement}, so the ssh_config \
                 read here is not the one that maps this alias"
            ),
        ));
    }
    let destination = host.target();
    crate::cli::check_operand("ssh target", &destination)
        .map_err(|error| unresolved(error.to_string()))?;
    if destination.chars().any(unusable_in_a_target) {
        return Err(unresolved(
            "SSH target holds whitespace or a control character".to_string(),
        ));
    }
    let mut argv = vec!["-G".to_string()];
    argv.extend(host.connection_opts());
    argv.extend(["--".to_string(), destination]);

    let mut command = std::process::Command::new(ssh_program());
    command.args(&argv).env("LC_ALL", "C");
    let run = crate::proc::run_child(&mut command, SSH_CONFIG_TIMEOUT, None)
        .map_err(|error| unresolved(format!("ssh could not be started: {error}")))?;
    match &run.status {
        Ok(0) => {}
        Ok(code) => {
            return Err(unresolved(format!(
                "ssh -G exited {code}: {}",
                run.stderr_lossy().trim()
            )))
        }
        Err(stop) => return Err(unresolved(format!("ssh -G {stop}"))),
    }
    // A prefix of the effective configuration is not the effective
    // configuration: the `hostname` line may be the one that was cut off.
    if let Err(io) = &run.io {
        return Err(unresolved(format!("ssh -G {io}")));
    }
    let stdout = run.stdout_lossy();
    let Some(hostname) = reported_hostname(&stdout) else {
        return Err(unresolved("ssh -G reported no hostname".to_string()));
    };
    // An identity is built by joining this with the path, so a host holding a
    // separator, a leading dash or a control character would not survive that
    // as itself — and must not be reported as though it had.
    if hostname.is_empty()
        || crate::cli::is_flag_like(hostname)
        || hostname
            .chars()
            .any(|c| c == '/' || unusable_in_a_target(c))
    {
        return Err(unresolved(format!(
            "ssh -G named an unusable host: {hostname:?}"
        )));
    }
    Ok(hostname.to_string())
}

/// The `hostname` line of `ssh -G` output, and nothing else.
///
/// [`parse_effective_ssh_config`] reads the same text, and is deliberately not
/// reused: it is the **write-profile validator** for a remote stash, so it
/// refuses `ControlMaster`, `ProxyJump`/`ProxyCommand`, a `ControlPath`, and
/// paths still holding a `%` token — because a stashed write has to reproduce
/// one exact connection later. Naming a host needs none of that, and borrowing
/// those refusals turned a `Host *` with `ControlMaster auto` in it — very
/// ordinary, and no obstacle to `ssh` answering `hostname github.com` — into an
/// unidentifiable remote (#706 review 1).
///
/// `ssh -G` prints one `key value` per line, lower-cased, `hostname` among
/// them; the first is taken, since a later repeat would be one `ssh` itself
/// ignores.
///
/// [`parse_effective_ssh_config`]: kagi_domain::remote::parse_effective_ssh_config
fn reported_hostname(text: &str) -> Option<&str> {
    text.lines().find_map(|line| {
        let (key, value) = line.trim().split_once(char::is_whitespace)?;
        key.eq_ignore_ascii_case("hostname").then(|| value.trim())
    })
}

/// Whether a host token is already the host it means, rather than something
/// only an `ssh_config` could translate.
///
/// A dotted name and an address literal name an endpoint on their own; a bare
/// word is exactly the `Host gh` shape that has to be looked up. This is the
/// same line [`repo_identity`] draws by reading the URL literally, kept only
/// for the case where the lookup is unavailable — never used to *map* one
/// token onto another.
fn names_itself(host: &str) -> bool {
    host.contains('.') || host.starts_with('[')
}

fn unusable_in_a_target(c: char) -> bool {
    c.is_whitespace() || c.is_control()
}

/// Remove URL userinfo from both the target and diagnostics that echo it.
struct Redacted<'a> {
    alias: &'a str,
    userinfo: Option<&'a str>,
}

impl<'a> Redacted<'a> {
    fn new(target: &'a str) -> Self {
        let (userinfo, alias) = match target.rsplit_once('@') {
            Some((userinfo, host)) => (Some(userinfo), host),
            None => (None, target),
        };
        Self { alias, userinfo }
    }

    fn scrub(&self, text: &str) -> String {
        let Some(userinfo) = self.userinfo.filter(|value| !value.is_empty()) else {
            return text.to_string();
        };
        let scrubbed = text.replace(userinfo, "***");
        match userinfo.split_once(':') {
            Some((_, password)) if !password.is_empty() => scrubbed.replace(password, "***"),
            _ => scrubbed,
        }
    }
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

    /// Whether the *process* environment already replaces `ssh`.
    ///
    /// `GIT_SSH_COMMAND` / `GIT_SSH` are read from the environment, which a
    /// test cannot pin without racing every sibling thread, so the tests that
    /// are about resolving a target say so and stand down rather than assert
    /// against a shell nobody controls. The override path has its own coverage
    /// (`a_replaced_ssh_command_keeps_a_literal_host_and_refuses_only_an_alias`),
    /// which drives it through config instead.
    fn ssh_is_not_replaced() -> bool {
        for key in ["GIT_SSH_COMMAND", "GIT_SSH"] {
            if std::env::var_os(key).is_some_and(|value| !value.is_empty()) {
                eprintln!("skipping: {key} is set in this environment");
                return false;
            }
        }
        true
    }

    /// A stand-in `ssh` answering from a fixed script: no host is contacted,
    /// and the developer's own `ssh_config` is never read.
    #[cfg(unix)]
    fn fake_ssh(dir: &std::path::Path, body: &str) {
        use std::os::unix::fs::PermissionsExt as _;
        let path = dir.join("ssh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        set_ssh_program_for_test(&path);
    }

    /// A URL that names its own endpoint is answered without asking anything:
    /// the `ssh` it would have to run does not exist here, so a spawn would be
    /// an error rather than a silent cost.
    #[test]
    fn a_url_that_names_its_own_host_is_identified_without_ssh() {
        if !ssh_is_not_replaced() {
            return;
        }
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
        for url in ["git@gh:acme/widgets.git", "git@gh:widgets.git"] {
            let error = resolve_repo_identity(url, &config).unwrap_err();
            assert!(
                error.unaddressable(),
                "an ssh that cannot be run leaves the remote with no address: {error}"
            );
        }
    }

    /// The identity read is not a configuration review: an `ssh_config` with
    /// `ControlMaster auto`, a `ProxyJump` and an agent-only identity in it is
    /// what a great many developers have, and the host it names is still
    /// `github.com` (#706 review 1).
    ///
    /// Real `ssh`, real parsing, no network: `-G` prints the effective
    /// configuration and exits, and `-F` points it at a fixture rather than the
    /// developer's own file. A `Host` block the fixture does not define is the
    /// other half of the boundary — `ssh` answers with the token itself and
    /// exits 0, so nothing here can call it unmappable, and only the attempt to
    /// reach it can fail (#706 review 3).
    #[cfg(unix)]
    #[test]
    fn an_ordinary_ssh_config_is_read_for_its_host_not_validated() {
        if !ssh_is_not_replaced() {
            return;
        }
        if !std::process::Command::new("ssh")
            .arg("-V")
            .output()
            .is_ok_and(|out| out.status.success())
        {
            eprintln!("skipping: no system ssh to read a configuration with");
            return;
        }
        let (dir, config) = fixture_config();
        let ssh_config = dir.path().join("ssh_config");
        std::fs::write(
            &ssh_config,
            "Host *\n  ControlMaster auto\n  ControlPath /tmp/kagi-%r@%h:%p\n  \
             ProxyJump bastion.example\n  IdentityFile none\n  \
             UserKnownHostsFile ~/.ssh/kagi_known_hosts\n",
        )
        .unwrap();
        // The system `ssh`, reading the fixture instead of `~/.ssh/config`.
        fake_ssh(
            dir.path(),
            &format!("exec ssh -F {} \"$@\"", ssh_config.display()),
        );

        // The fixture is live, and it is the very shape the write-profile
        // validator rejects: without this, the test could pass on an `ssh` that
        // ignored the file.
        let probe = std::process::Command::new(ssh_program())
            .args(["-G", "--", "git@github.com"])
            .output()
            .expect("ssh -G");
        let effective = String::from_utf8_lossy(&probe.stdout);
        for setting in ["controlmaster auto", "proxyjump bastion.example"] {
            assert!(
                effective.contains(setting),
                "fixture is inert: {setting} is not in effect:\n{effective}"
            );
        }
        assert!(
            kagi_domain::remote::parse_effective_ssh_config(&effective).is_err(),
            "the write-profile validator is what must keep refusing this, not identity"
        );

        assert_eq!(
            resolve_repo_identity("git@github.com:acme/widgets.git", &config)
                .unwrap()
                .as_deref(),
            Some("github.com/acme/widgets"),
            "a literal host stays identified through ordinary ssh settings"
        );
        assert_eq!(
            resolve_repo_identity("kagi-undefined-alias-706:acme/widgets.git", &config)
                .unwrap()
                .as_deref(),
            Some("kagi-undefined-alias-706/acme/widgets"),
            "ssh answers an undefined alias with the token itself, so this read \
             cannot tell it apart from a host — only reaching it can"
        );
    }

    /// A replacement for `ssh` means the configuration read here is not the one
    /// git uses — which is only ever about an *alias*. A URL that spells its
    /// host out is identified exactly as it was before this module existed
    /// (#706 review 2), and nothing about the replacement is quoted.
    #[test]
    fn a_replaced_ssh_command_keeps_a_literal_host_and_refuses_only_an_alias() {
        // An environment override would shadow the configured one this test is
        // about, and would name itself in the refusal instead.
        if !ssh_is_not_replaced() {
            return;
        }
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

        for (url, identity) in [
            ("git@github.com:acme/widgets.git", "github.com/acme/widgets"),
            (
                "ssh://git@ghe.example:2222/acme/widgets.git",
                "ghe.example/acme/widgets",
            ),
        ] {
            assert_eq!(
                resolve_repo_identity(url, &config).unwrap().as_deref(),
                Some(identity),
                "{url} names its own host, whatever transports it"
            );
        }

        let error = resolve_repo_identity("git@gh:acme/widgets.git", &config).unwrap_err();
        assert!(error.reason.contains("core.sshCommand"), "{}", error.reason);
        assert!(
            !error.unaddressable(),
            "git reaches this remote through the replacement; only the alias is unreadable"
        );
        assert!(
            !error.reason.contains("sentinel-secret"),
            "command credentials must not enter notices or the audit log"
        );
    }

    /// A password in a remote URL is a credential, and every refusal is built
    /// from text that has just been handed that URL: the argv check echoes the
    /// operand it rejected, `ssh` echoes the destination it was given, and a
    /// malformed target is quoted back as itself (#706 review 5).
    #[cfg(unix)]
    #[test]
    fn a_password_in_the_url_never_reaches_a_refusal() {
        if !ssh_is_not_replaced() {
            return;
        }
        let (dir, config) = fixture_config();
        // Echoes the destination back, the way a real ssh reports one it could
        // not use.
        fake_ssh(
            dir.path(),
            "echo \"ssh: could not resolve $*\" >&2\nexit 255",
        );

        for url in [
            // Asked, and refused by ssh itself.
            "ssh://user:sentinel-token@gh/acme/widgets.git",
            // Userinfo can itself be a token without a password separator.
            "ssh://sentinel-token@gh/acme/widgets.git",
            // Refused by the argv check before any process starts.
            "ssh://-evil:sentinel-token@gh/acme/widgets.git",
            // Malformed: no host at all behind the userinfo.
            "ssh://user:sentinel-token@:2222/acme/widgets.git",
        ] {
            let error = resolve_repo_identity(url, &config).unwrap_err();
            let shown = error.to_string();
            assert!(
                !shown.contains("sentinel-token"),
                "{url} leaked its password: {shown}"
            );
            assert!(error.unaddressable(), "{url}");
        }
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
