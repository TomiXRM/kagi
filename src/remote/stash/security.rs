//! Frozen known-hosts snapshots used by the remote write boundary.

use super::{io_error, FrozenConnection, RemotePlanError};
use kagi_domain::remote::KnownHostsIdentity;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(super) struct FrozenFile {
    pub(super) path: PathBuf,
    pub(super) digest: String,
}

pub(super) fn snapshot_known_hosts(
    paths: &[String],
    dir: &Path,
    prefix: &str,
) -> Result<(Vec<KnownHostsIdentity>, Vec<String>, Vec<FrozenFile>), RemotePlanError> {
    let mut identities = Vec::new();
    let mut snapshots = Vec::new();
    let mut frozen = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        if path == "none" {
            continue;
        }
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(io_error(error)),
        };
        let digest = hex_digest(&bytes);
        let snapshot = dir.join(format!("{prefix}-{index}"));
        fs::write(&snapshot, &bytes).map_err(io_error)?;
        set_owner_only(&snapshot)?;
        identities.push(KnownHostsIdentity {
            path: path.clone(),
            digest: digest.clone(),
        });
        snapshots.push(snapshot.display().to_string());
        frozen.push(FrozenFile {
            path: snapshot,
            digest,
        });
    }
    if snapshots.is_empty() {
        return Err(RemotePlanError::UnsafeConfig(
            "no known_hosts file can be frozen".into(),
        ));
    }
    Ok((identities, snapshots, frozen))
}

pub(super) fn verify_known_hosts_snapshots(
    connection: &FrozenConnection,
) -> Result<(), RemotePlanError> {
    for snapshot in &connection.known_hosts_snapshots {
        verify_frozen_file(snapshot)?;
    }
    Ok(())
}

fn verify_frozen_file(snapshot: &FrozenFile) -> Result<(), RemotePlanError> {
    let metadata = fs::symlink_metadata(&snapshot.path).map_err(io_error)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(RemotePlanError::UnsafeConfig(format!(
            "known_hosts snapshot is missing or not a regular file: {}",
            snapshot.path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o777 != 0o600 {
            return Err(RemotePlanError::UnsafeConfig(format!(
                "known_hosts snapshot mode changed: {}",
                snapshot.path.display()
            )));
        }
    }
    let bytes = fs::read(&snapshot.path).map_err(io_error)?;
    if hex_digest(&bytes) != snapshot.digest {
        return Err(RemotePlanError::UnsafeConfig(format!(
            "known_hosts snapshot changed after plan: {}",
            snapshot.path.display()
        )));
    }
    Ok(())
}

#[doc(hidden)]
pub fn verify_known_hosts_snapshot_for_test(
    path: &Path,
    expected_bytes: &[u8],
) -> Result<(), RemotePlanError> {
    verify_frozen_file(&FrozenFile {
        path: path.to_path_buf(),
        digest: hex_digest(expected_bytes),
    })
}

#[cfg(unix)]
fn set_owner_only(path: &Path) -> Result<(), RemotePlanError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(io_error)
}

#[cfg(not(unix))]
fn set_owner_only(_path: &Path) -> Result<(), RemotePlanError> {
    Ok(())
}

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
