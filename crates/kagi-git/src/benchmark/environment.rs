use std::fs;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::{FixtureManifest, HarnessError};

/// Environment fields required beside every #627 measurement. Unknown values
/// remain explicit `null`; callers must not turn them into invented defaults.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EnvironmentMeta {
    pub os: String,
    pub os_version: Option<String>,
    pub arch: String,
    pub git_version: Option<String>,
    pub git2_crate_version: String,
    pub filesystem: Option<String>,
    pub fsmonitor: Option<String>,
    pub untracked_cache: Option<String>,
    pub index_version: Option<u32>,
    pub fixture_manifest: Option<FixtureManifest>,
    pub other_process_load: Option<String>,
}

/// Gather stable environment context without mutating the target repository.
type RepositorySettings = (Option<String>, Option<String>, Option<u32>);

pub fn collect_environment(
    repo_path: Option<&Path>,
    fixture_manifest: Option<FixtureManifest>,
) -> Result<EnvironmentMeta, HarnessError> {
    let (fsmonitor, untracked_cache, index_version) = match repo_path {
        Some(path) => repository_settings(path)?,
        None => (None, None, None),
    };
    Ok(EnvironmentMeta {
        os: std::env::consts::OS.to_owned(),
        os_version: os_version(),
        arch: std::env::consts::ARCH.to_owned(),
        git_version: command_stdout("git", &["--version"], None),
        // This is the direct dependency in crates/kagi-git/Cargo.toml. Keeping
        // it visible in each artifact is more useful than libgit2's ABI version.
        git2_crate_version: "0.21".to_owned(),
        filesystem: repo_path.and_then(filesystem_kind),
        fsmonitor,
        untracked_cache,
        index_version,
        fixture_manifest,
        other_process_load: None,
    })
}

fn repository_settings(path: &Path) -> Result<RepositorySettings, HarnessError> {
    let repo = git2::Repository::open(path)?;
    let config = repo.config()?;
    let fsmonitor = config.get_string("core.fsmonitor").ok();
    let untracked_cache = config.get_string("core.untrackedCache").ok();
    let index_version = repo
        .index()?
        .path()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| {
            bytes
                .get(4..8)
                .and_then(|version| version.try_into().ok())
                .map(u32::from_be_bytes)
        });
    Ok((fsmonitor, untracked_cache, index_version))
}

fn os_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        return command_stdout("sw_vers", &["-productVersion"], None);
    }
    #[cfg(target_os = "linux")]
    {
        return fs::read_to_string("/proc/sys/kernel/osrelease")
            .ok()
            .map(|value| value.trim().to_owned());
    }
    #[cfg(target_os = "windows")]
    {
        return command_stdout("cmd", &["/C", "ver"], None);
    }
    #[allow(unreachable_code)]
    None
}

fn filesystem_kind(path: &Path) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        return macos_filesystem_kind(path);
    }
    #[cfg(target_os = "linux")]
    {
        return command_stdout("stat", &["-f", "-c", "%T", "."], Some(path));
    }
    #[cfg(target_os = "windows")]
    {
        let root = path
            .components()
            .next()?
            .as_os_str()
            .to_string_lossy()
            .to_string();
        return command_stdout("fsutil", &["fsinfo", "volumeinfo", &root], None);
    }
    #[allow(unreachable_code)]
    None
}

#[cfg(target_os = "macos")]
fn macos_filesystem_kind(path: &Path) -> Option<String> {
    let canonical = path.canonicalize().ok()?;
    let mounts = command_stdout("mount", &[], None)?;
    mounts
        .lines()
        .filter_map(|line| {
            let (_, rest) = line.split_once(" on ")?;
            let (mountpoint, options) = rest.split_once(" (")?;
            let kind = options.split(',').next()?.trim();
            let mountpoint = Path::new(mountpoint);
            canonical
                .starts_with(mountpoint)
                .then(|| (mountpoint.components().count(), kind))
        })
        .max_by_key(|(depth, _)| *depth)
        .map(|(_, kind)| kind.to_owned())
}

fn command_stdout(program: &str, args: &[&str], cwd: Option<&Path>) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
}
