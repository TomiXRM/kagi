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
    pub dirs: usize,
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
        dirs: worktree_dirs(&request.output)?,
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

/// Copy a pristine template without hardlinks or reflinks. Linked worktrees get
/// a sibling Git storage copy and relative gitdir/commondir links, so neither
/// the copy nor its Git metadata points back into the template.
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
    let source_repo = git2::Repository::open(&source)?;
    let linked_worktree = fs::symlink_metadata(source.join(".git"))
        .map_err(|error| HarnessError::io(&source, error))?
        .is_file();
    copy_tree(&source, destination)?;
    if linked_worktree {
        materialize_linked_worktree_storage(&source_repo, destination)?;
    }
    Ok(())
}

pub fn manifest_path(template: &Path) -> PathBuf {
    let mut path = template.as_os_str().to_os_string();
    path.push(".manifest.json");
    PathBuf::from(path)
}

fn materialize_linked_worktree_storage(
    source_repo: &git2::Repository,
    destination: &Path,
) -> Result<(), HarnessError> {
    let parent = destination
        .parent()
        .ok_or_else(|| HarnessError::new("linked worktree destination has no parent"))?;
    let name = destination
        .file_name()
        .ok_or_else(|| HarnessError::new("linked worktree destination has no file name"))?;
    let storage = parent.join(format!(".{}.kagi-git", name.to_string_lossy()));
    if storage.exists() {
        return Err(HarnessError::new(format!(
            "linked worktree storage already exists: {}",
            storage.display()
        )));
    }
    fs::create_dir(&storage).map_err(|error| HarnessError::io(&storage, error))?;
    let private = storage.join("private");
    let common = storage.join("common");
    copy_tree(source_repo.commondir(), &common)?;
    let copied_worktrees = common.join("worktrees");
    if copied_worktrees.exists() {
        fs::remove_dir_all(&copied_worktrees)
            .map_err(|error| HarnessError::io(&copied_worktrees, error))?;
    }
    copy_tree(source_repo.path(), &private)?;

    let source_private = source_repo.path();
    let source_worktree = source_repo
        .workdir()
        .ok_or_else(|| HarnessError::new("linked worktree has no workdir"))?;
    fs::write(private.join("commondir"), "../common\n")
        .map_err(|error| HarnessError::io(&private, error))?;
    set_copy_modified_time(
        &private.join("commondir"),
        &fs::symlink_metadata(source_private.join("commondir"))
            .map_err(|error| HarnessError::io(source_private, error))?,
    )?;
    let gitdir = format!("../../{}/.git\n", name.to_string_lossy());
    fs::write(private.join("gitdir"), gitdir).map_err(|error| HarnessError::io(&private, error))?;
    set_copy_modified_time(
        &private.join("gitdir"),
        &fs::symlink_metadata(source_private.join("gitdir"))
            .map_err(|error| HarnessError::io(source_private, error))?,
    )?;
    let git_entry = format!("gitdir: ../.{}.kagi-git/private\n", name.to_string_lossy());
    let destination_git_entry = destination.join(".git");
    fs::write(&destination_git_entry, git_entry)
        .map_err(|error| HarnessError::io(destination, error))?;
    set_copy_modified_time(
        &destination_git_entry,
        &fs::symlink_metadata(source_worktree.join(".git"))
            .map_err(|error| HarnessError::io(source_worktree, error))?,
    )
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

fn fixture_git_command(repo: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(repo)
        .args(["-c", "core.autocrlf=false", "-c", "core.eol=lf"])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", empty_global_config())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_CONFIG_COUNT");
    command
}

#[cfg(windows)]
fn empty_global_config() -> &'static str {
    "NUL"
}

#[cfg(not(windows))]
fn empty_global_config() -> &'static str {
    "/dev/null"
}

fn git(repo: &Path, args: &[&str]) -> Result<(), HarnessError> {
    let status = fixture_git_command(repo)
        .args(args)
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
    let output = fixture_git_command(repo)
        .args(args)
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
    let status = fixture_git_command(repo)
        .args(["commit", "-q", "-m", message])
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

fn worktree_dirs(root: &Path) -> Result<usize, HarnessError> {
    let mut total = 1_usize;
    visit(root, &mut |path, metadata| {
        if path.file_name().is_some_and(|name| name == ".git") {
            return Ok(false);
        }
        if metadata.is_dir() {
            total = total.saturating_add(1);
            return Ok(true);
        }
        Ok(false)
    })?;
    Ok(total)
}

fn max_directory_width(root: &Path) -> Result<usize, HarnessError> {
    let mut maximum = directory_width(root)?;
    visit(root, &mut |path, metadata| {
        if path.file_name().is_some_and(|name| name == ".git") {
            return Ok(false);
        }
        if metadata.is_dir() {
            maximum = maximum.max(directory_width(path)?);
            return Ok(true);
        }
        Ok(false)
    })?;
    Ok(maximum)
}

fn directory_width(path: &Path) -> Result<usize, HarnessError> {
    fs::read_dir(path)
        .map_err(|error| HarnessError::io(path, error))?
        .try_fold(0_usize, |width, entry| {
            let entry = entry.map_err(|error| HarnessError::io(path, error))?;
            Ok::<_, HarnessError>(width.saturating_add(usize::from(entry.file_name() != ".git")))
        })
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
        copy_symlink(source, destination, &metadata)?;
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
fn copy_symlink(
    source: &Path,
    destination: &Path,
    source_metadata: &fs::Metadata,
) -> Result<(), HarnessError> {
    use std::os::unix::fs::symlink;

    let target = fs::read_link(source).map_err(|error| HarnessError::io(source, error))?;
    symlink(target, destination).map_err(|error| HarnessError::io(destination, error))?;
    filetime::set_symlink_file_times(
        destination,
        filetime::FileTime::from_last_access_time(source_metadata),
        filetime::FileTime::from_last_modification_time(source_metadata),
    )
    .map_err(|error| HarnessError::io(destination, error))
}

#[cfg(windows)]
fn copy_symlink(
    source: &Path,
    destination: &Path,
    _source_metadata: &fs::Metadata,
) -> Result<(), HarnessError> {
    use std::os::windows::fs::{symlink_dir, symlink_file};

    let target = fs::read_link(source).map_err(|error| HarnessError::io(source, error))?;
    if fs::metadata(source)
        .map_err(|error| HarnessError::io(source, error))?
        .is_dir()
    {
        symlink_dir(target, destination).map_err(|error| HarnessError::io(destination, error))
    } else {
        symlink_file(target, destination).map_err(|error| HarnessError::io(destination, error))
    }
}

#[cfg(all(not(unix), not(windows)))]
fn copy_symlink(
    source: &Path,
    _destination: &Path,
    _source_metadata: &fs::Metadata,
) -> Result<(), HarnessError> {
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

/// Restore a copied entry's modified time from the template.
///
/// The handle this opens is platform-sensitive in two ways that a read-only
/// `File::open` hides. Windows refuses to set times through a handle that does
/// not carry write access, and will not open a directory as a plain `File` at
/// all; Unix's `futimens` accepts a read-only handle and opening a directory is
/// ordinary. Opening read-only therefore looked portable and was not — the
/// Windows probe failed here with `Access is denied (os error 5)` on
/// `.git/COMMIT_EDITMSG` (#627).
fn set_copy_modified_time(path: &Path, source: &fs::Metadata) -> Result<(), HarnessError> {
    let Ok(modified) = source.modified() else {
        return Ok(());
    };
    // Directory times cannot be restored on Windows through this API. Both
    // copies of a template skip them identically, so a fingerprint comparison
    // between copies stays valid; it just carries less on Windows.
    if cfg!(windows) && source.is_dir() {
        return Ok(());
    }
    let handle = if source.is_dir() {
        File::open(path)
    } else {
        File::options().write(true).open(path)
    };
    handle
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

    #[cfg(unix)]
    #[test]
    fn pristine_copies_preserve_symlink_fingerprint_metadata() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root = tempfile::tempdir().unwrap();
        let template = root.path().join("template");
        generate_fixture(&FixtureRequest {
            output: template.clone(),
            files: 1,
            commits: 1,
            depth: 1,
            seed: 627,
            scenario: Some("synthetic".into()),
        })
        .unwrap();
        let mode = fs::metadata(&template).unwrap().permissions().mode();
        fs::set_permissions(&template, fs::Permissions::from_mode(mode | 0o200)).unwrap();
        let link = template.join("tracked-link");
        symlink("d00-00/file-000000.bin", &link).unwrap();
        let fixture_time = filetime::FileTime::from_unix_time(1_704_067_200, 123_456_789);
        filetime::set_symlink_file_times(&link, fixture_time, fixture_time).unwrap();
        make_template_read_only(&template).unwrap();

        let first = root.path().join("first");
        let second = root.path().join("second");
        materialize_pristine(&template, &first).unwrap();
        materialize_pristine(&template, &second).unwrap();

        assert_eq!(
            fingerprint_repository(&first).unwrap(),
            fingerprint_repository(&second).unwrap(),
            "symlink metadata must not make pristine copies diverge"
        );
    }

    #[test]
    fn manifest_counts_root_directory_and_width() {
        let root = tempfile::tempdir().unwrap();
        let template = root.path().join("template");
        let manifest = generate_fixture(&FixtureRequest {
            output: template,
            files: 3,
            commits: 1,
            depth: 1,
            seed: 627,
            scenario: Some("synthetic".into()),
        })
        .unwrap();

        assert_eq!(manifest.dirs, 1);
        assert_eq!(manifest.max_directory_width, 3);
    }

    #[test]
    fn linked_worktree_copy_uses_independent_relative_git_storage() {
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("main");
        fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]).unwrap();
        git(&main, &["config", "user.name", "Fixture"]).unwrap();
        git(&main, &["config", "user.email", "fixture@example.test"]).unwrap();
        fs::write(main.join("tracked.txt"), b"base\n").unwrap();
        git(&main, &["add", "tracked.txt"]).unwrap();
        commit(&main, 0, "base").unwrap();

        let linked = root.path().join("linked");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "linked",
                linked.to_str().unwrap(),
            ],
        )
        .unwrap();
        let copy = root.path().join("copy");
        materialize_pristine(&linked, &copy).unwrap();

        let copy_gitfile = fs::read_to_string(copy.join(".git")).unwrap();
        assert!(copy_gitfile.starts_with("gitdir: ../.copy.kagi-git/private"));
        assert!(git_stdout(&copy, &["status", "--porcelain"])
            .unwrap()
            .is_empty());
        fs::write(copy.join("copy-only.txt"), b"copy\n").unwrap();
        git(&copy, &["add", "copy-only.txt"]).unwrap();
        assert!(git_stdout(&linked, &["status", "--porcelain"])
            .unwrap()
            .is_empty());
    }
}
