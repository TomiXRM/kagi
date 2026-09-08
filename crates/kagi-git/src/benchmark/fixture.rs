use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::HarnessError;

/// Inputs accepted by `backend_fixture` for deterministic synthetic fixtures.
#[derive(Clone, Debug)]
pub struct FixtureRequest {
    pub output: PathBuf,
    pub files: usize,
    pub commits: usize,
    pub depth: usize,
    pub seed: u64,
    pub scenario: Option<String>,
}

/// Sidecar document describing the generated pristine template.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FixtureManifest {
    pub schema_version: u32,
    pub seed: u64,
    pub scenario: Option<String>,
    pub tracked_files: usize,
    pub commits: usize,
    pub depth: usize,
    pub total_bytes: u64,
    pub max_directory_width: usize,
    pub head: String,
}

/// Create a deterministic, read-only Git template and write its manifest beside
/// it as `<fixture>.manifest.json`. The sidecar deliberately stays outside the
/// worktree so the template begins clean.
pub fn generate_fixture(request: &FixtureRequest) -> Result<FixtureManifest, HarnessError> {
    validate_request(request)?;
    if request.output.exists() {
        return Err(HarnessError::new(format!(
            "fixture output already exists: {}",
            request.output.display()
        )));
    }
    fs::create_dir_all(&request.output)
        .map_err(|error| HarnessError::io(&request.output, error))?;
    git(&request.output, &["init", "-q", "-b", "main"])?;
    git(&request.output, &["config", "user.name", "Kagi Fixture"])?;
    git(
        &request.output,
        &["config", "user.email", "fixture@example.test"],
    )?;
    git(&request.output, &["config", "commit.gpgSign", "false"])?;

    for index in 0..request.files {
        let path = fixture_file_path(request.depth, index);
        let bytes = deterministic_bytes(request.seed, index, file_size(request, index));
        write_file(&request.output.join(path), &bytes)?;
    }
    git(&request.output, &["add", "--all"])?;
    commit(&request.output, 0, "fixture base")?;

    for index in 1..request.commits {
        let file = fixture_file_path(request.depth, index % request.files);
        let bytes = deterministic_bytes(
            request.seed ^ index as u64,
            index,
            file_size(request, index),
        );
        write_file(&request.output.join(&file), &bytes)?;
        git(&request.output, &["add", "--", &file])?;
        commit(&request.output, index, &format!("fixture change {index}"))?;
    }

    let manifest = FixtureManifest {
        schema_version: 1,
        seed: request.seed,
        scenario: request.scenario.clone(),
        tracked_files: request.files,
        commits: request.commits,
        depth: request.depth,
        total_bytes: worktree_bytes(&request.output)?,
        max_directory_width: max_directory_width(&request.output)?,
        head: git_stdout(&request.output, &["rev-parse", "HEAD"])?,
    };
    let manifest_path = manifest_path(&request.output);
    let body = serde_json::to_vec_pretty(&manifest)?;
    fs::write(&manifest_path, body).map_err(|error| HarnessError::io(&manifest_path, error))?;
    make_template_read_only(&request.output)?;
    Ok(manifest)
}

/// Copy a pristine template without hardlinks or reflinks. The destination is
/// made owner-writable; this function is called before the timed interval.
pub fn materialize_pristine(template: &Path, destination: &Path) -> Result<(), HarnessError> {
    if destination.exists() {
        return Err(HarnessError::new(format!(
            "pristine destination already exists: {}",
            destination.display()
        )));
    }
    let source = template
        .canonicalize()
        .map_err(|error| HarnessError::io(template, error))?;
    copy_tree(&source, destination)?;
    Ok(())
}

pub fn manifest_path(template: &Path) -> PathBuf {
    let mut path = template.as_os_str().to_os_string();
    path.push(".manifest.json");
    PathBuf::from(path)
}

fn validate_request(request: &FixtureRequest) -> Result<(), HarnessError> {
    if request.files == 0 || request.commits == 0 || request.depth == 0 {
        return Err(HarnessError::new(
            "--files, --commits, and --depth must all be positive",
        ));
    }
    if let Some(scenario) = &request.scenario {
        if scenario != "synthetic" {
            return Err(HarnessError::new(format!(
                "scenario '{scenario}' is not registered; P0 provides only synthetic"
            )));
        }
    }
    Ok(())
}

fn file_size(request: &FixtureRequest, index: usize) -> usize {
    // The L fixture reserves 20 of its requested paths for the plan's 1 MiB blobs.
    if request.files >= 50_000 && request.commits >= 2_000 && index < 20 {
        1024 * 1024
    } else {
        256
    }
}

fn fixture_file_path(depth: usize, index: usize) -> String {
    let mut path = String::new();
    for level in 0..depth.saturating_sub(1) {
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(&format!("d{level:02}-{:02}", (index / (level + 1)) % 17));
    }
    if !path.is_empty() {
        path.push('/');
    }
    path.push_str(&format!("file-{index:06}.bin"));
    path
}

fn deterministic_bytes(seed: u64, index: usize, len: usize) -> Vec<u8> {
    let mut state = seed ^ (index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut output = Vec::with_capacity(len);
    while output.len() < len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        output.extend_from_slice(&state.to_le_bytes());
    }
    output.truncate(len);
    output
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), HarnessError> {
    let parent = path
        .parent()
        .ok_or_else(|| HarnessError::new("fixture path has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| HarnessError::io(parent, error))?;
    fs::write(path, bytes).map_err(|error| HarnessError::io(path, error))
}

fn git(repo: &Path, args: &[&str]) -> Result<(), HarnessError> {
    let status = Command::new("git")
        .current_dir(repo)
        .args(["-c", "core.autocrlf=false", "-c", "core.eol=lf"])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .map_err(|error| HarnessError::io(repo, error))?;
    if status.success() {
        Ok(())
    } else {
        Err(HarnessError::new(format!(
            "git {:?} failed in {}",
            args,
            repo.display()
        )))
    }
}

fn git_stdout(repo: &Path, args: &[&str]) -> Result<String, HarnessError> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(["-c", "core.autocrlf=false", "-c", "core.eol=lf"])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .map_err(|error| HarnessError::io(repo, error))?;
    if !output.status.success() {
        return Err(HarnessError::new(format!(
            "git {:?} failed in {}",
            args,
            repo.display()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn commit(repo: &Path, index: usize, message: &str) -> Result<(), HarnessError> {
    let timestamp = 1_704_067_200_i64 + index as i64;
    let date = format!("{timestamp} +0000");
    let status = Command::new("git")
        .current_dir(repo)
        .args(["-c", "core.autocrlf=false", "-c", "core.eol=lf"])
        .args(["commit", "-q", "-m", message])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .status()
        .map_err(|error| HarnessError::io(repo, error))?;
    if status.success() {
        Ok(())
    } else {
        Err(HarnessError::new(format!(
            "git commit failed in {}",
            repo.display()
        )))
    }
}

fn worktree_bytes(root: &Path) -> Result<u64, HarnessError> {
    let mut total = 0_u64;
    visit(root, &mut |path, metadata| {
        if path.file_name().is_some_and(|name| name == ".git") {
            return Ok(false);
        }
        if metadata.is_file() {
            total = total.saturating_add(metadata.len());
        }
        Ok(metadata.is_dir())
    })?;
    Ok(total)
}

fn max_directory_width(root: &Path) -> Result<usize, HarnessError> {
    let mut maximum = 0_usize;
    visit(root, &mut |path, metadata| {
        if path.file_name().is_some_and(|name| name == ".git") {
            return Ok(false);
        }
        if metadata.is_dir() {
            let width = fs::read_dir(path)
                .map_err(|error| HarnessError::io(path, error))?
                .count();
            maximum = maximum.max(width);
            return Ok(true);
        }
        Ok(false)
    })?;
    Ok(maximum)
}

fn visit(
    root: &Path,
    visitor: &mut impl FnMut(&Path, &fs::Metadata) -> Result<bool, HarnessError>,
) -> Result<(), HarnessError> {
    for entry in fs::read_dir(root).map_err(|error| HarnessError::io(root, error))? {
        let entry = entry.map_err(|error| HarnessError::io(root, error))?;
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| HarnessError::io(&path, error))?;
        if visitor(&path, &metadata)? {
            visit(&path, visitor)?;
        }
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), HarnessError> {
    let metadata = fs::symlink_metadata(source).map_err(|error| HarnessError::io(source, error))?;
    if metadata.is_dir() {
        fs::create_dir(destination).map_err(|error| HarnessError::io(destination, error))?;
        for entry in fs::read_dir(source).map_err(|error| HarnessError::io(source, error))? {
            let entry = entry.map_err(|error| HarnessError::io(source, error))?;
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
        set_copy_permissions(destination, &metadata)?;
        set_copy_modified_time(destination, &metadata)?;
    } else if metadata.is_file() {
        copy_file(source, destination)?;
        set_copy_permissions(destination, &metadata)?;
        set_copy_modified_time(destination, &metadata)?;
    } else if metadata.file_type().is_symlink() {
        copy_symlink(source, destination)?;
    } else {
        return Err(HarnessError::new(format!(
            "unsupported fixture entry: {}",
            source.display()
        )));
    }
    Ok(())
}

fn copy_file(source: &Path, destination: &Path) -> Result<(), HarnessError> {
    let mut input = File::open(source).map_err(|error| HarnessError::io(source, error))?;
    let mut output =
        File::create(destination).map_err(|error| HarnessError::io(destination, error))?;
    io::copy(&mut input, &mut output).map_err(|error| HarnessError::io(destination, error))?;
    output
        .flush()
        .map_err(|error| HarnessError::io(destination, error))
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<(), HarnessError> {
    use std::os::unix::fs::symlink;
    let target = fs::read_link(source).map_err(|error| HarnessError::io(source, error))?;
    symlink(target, destination).map_err(|error| HarnessError::io(destination, error))
}

#[cfg(not(unix))]
fn copy_symlink(source: &Path, _destination: &Path) -> Result<(), HarnessError> {
    Err(HarnessError::new(format!(
        "copying symlink fixtures is unavailable on this platform: {}",
        source.display()
    )))
}

#[cfg(unix)]
fn set_copy_permissions(path: &Path, source: &fs::Metadata) -> Result<(), HarnessError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = source.permissions().mode() | 0o200;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| HarnessError::io(path, error))
}

#[cfg(not(unix))]
fn set_copy_permissions(path: &Path, _source: &fs::Metadata) -> Result<(), HarnessError> {
    let mut permissions = fs::metadata(path)
        .map_err(|error| HarnessError::io(path, error))?
        .permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).map_err(|error| HarnessError::io(path, error))
}

fn set_copy_modified_time(path: &Path, source: &fs::Metadata) -> Result<(), HarnessError> {
    let Ok(modified) = source.modified() else {
        return Ok(());
    };
    File::open(path)
        .map_err(|error| HarnessError::io(path, error))?
        .set_times(fs::FileTimes::new().set_modified(modified))
        .map_err(|error| HarnessError::io(path, error))
}

fn make_template_read_only(root: &Path) -> Result<(), HarnessError> {
    let mut paths = vec![root.to_path_buf()];
    collect_paths(root, &mut paths)?;
    for path in paths.into_iter().rev() {
        make_read_only(&path)?;
    }
    Ok(())
}

fn collect_paths(root: &Path, paths: &mut Vec<PathBuf>) -> Result<(), HarnessError> {
    for entry in fs::read_dir(root).map_err(|error| HarnessError::io(root, error))? {
        let entry = entry.map_err(|error| HarnessError::io(root, error))?;
        let path = entry.path();
        if fs::symlink_metadata(&path)
            .map_err(|error| HarnessError::io(&path, error))?
            .is_dir()
        {
            collect_paths(&path, paths)?;
        }
        paths.push(path);
    }
    Ok(())
}

#[cfg(unix)]
fn make_read_only(path: &Path) -> Result<(), HarnessError> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::symlink_metadata(path).map_err(|error| HarnessError::io(path, error))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    let mode = metadata.permissions().mode() & !0o222;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| HarnessError::io(path, error))
}

#[cfg(not(unix))]
fn make_read_only(path: &Path) -> Result<(), HarnessError> {
    let mut permissions = fs::metadata(path)
        .map_err(|error| HarnessError::io(path, error))?
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).map_err(|error| HarnessError::io(path, error))
}

#[cfg(test)]
mod tests {
    use crate::benchmark::fingerprint_repository;
    use std::fs;

    use super::*;

    #[test]
    fn pristine_copies_are_independent_after_one_is_mutated() {
        let root = tempfile::tempdir().unwrap();
        let template = root.path().join("template");
        let manifest = generate_fixture(&FixtureRequest {
            output: template.clone(),
            files: 3,
            commits: 2,
            depth: 2,
            seed: 627,
            scenario: Some("synthetic".into()),
        })
        .unwrap();
        assert_eq!(manifest.tracked_files, 3);
        assert!(manifest_path(&template).is_file());

        let first = root.path().join("first");
        let second = root.path().join("second");
        materialize_pristine(&template, &first).unwrap();
        materialize_pristine(&template, &second).unwrap();
        let first_before = fingerprint_repository(&first).unwrap();
        let second_before = fingerprint_repository(&second).unwrap();
        assert_eq!(
            first_before, second_before,
            "copies from one manifest must have identical fingerprints"
        );
        let source_before = fs::read(template.join("d00-00/file-000000.bin")).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let source = fs::metadata(template.join("d00-00/file-000000.bin")).unwrap();
            let first_copy = fs::metadata(first.join("d00-00/file-000000.bin")).unwrap();
            let second_copy = fs::metadata(second.join("d00-00/file-000000.bin")).unwrap();
            assert_ne!(
                source.ino(),
                first_copy.ino(),
                "copy must not hardlink template data"
            );
            assert_ne!(
                source.ino(),
                second_copy.ino(),
                "copy must not hardlink template data"
            );
        }

        fs::write(
            first.join("d00-00/file-000000.bin"),
            b"mutated only in first copy",
        )
        .unwrap();

        assert_eq!(fingerprint_repository(&second).unwrap(), second_before);
        assert_eq!(
            fs::read(template.join("d00-00/file-000000.bin")).unwrap(),
            source_before
        );
    }
}
