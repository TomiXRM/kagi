//! Read-only probe of a live process's current directory (#772 Phase 1,
//! ADR-0208 決定 3).
//!
//! The terminal auto-lock needs the shell's *real* cwd, not its launch cwd
//! or its title (contract A). This is the one OS-specific read; everything
//! that decides what to do with the answer is above it. A failed probe is an
//! answer too — [`CwdProbe::Unknown`] — and the caller keeps whatever it
//! knew rather than treating the process as gone (contract C).

use std::path::PathBuf;

/// Why the cwd could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CwdProbe {
    /// No probe exists for this platform (Windows). Never a success.
    Unsupported,
    /// The probe ran and failed: the process may have exited, be owned by
    /// another user, or the path may be unreadable. The message is the OS's.
    Unknown(String),
}

impl std::fmt::Display for CwdProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CwdProbe::Unsupported => write!(f, "cwd probe is not supported on this platform"),
            CwdProbe::Unknown(why) => write!(f, "cwd unknown: {why}"),
        }
    }
}

/// The current directory of process `pid`.
#[cfg(target_os = "macos")]
pub fn cwd_of_pid(pid: u32) -> Result<PathBuf, CwdProbe> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;

    // SAFETY: `proc_vnodepathinfo` is a plain C struct the kernel fills; an
    // all-zero value is a valid (empty) instance, and we pass its exact size.
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    // SAFETY: the buffer pointer and size describe `info` exactly.
    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&mut info as *mut libc::proc_vnodepathinfo).cast(),
            size,
        )
    };
    if written <= 0 {
        return Err(CwdProbe::Unknown(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    if written < size {
        return Err(CwdProbe::Unknown(format!(
            "proc_pidinfo returned {written} of {size} bytes"
        )));
    }
    // `vip_path` is declared as `[[c_char; 32]; 32]` (libc's workaround for
    // large arrays); it is one NUL-terminated MAXPATHLEN buffer in memory.
    let raw: &[libc::c_char; 1024] =
        // SAFETY: same size and layout, only the nesting differs.
        unsafe { &*(&info.pvi_cdir.vip_path as *const [[libc::c_char; 32]; 32]).cast() };
    let path = CStr::from_bytes_until_nul(
        // SAFETY: `c_char` and `u8` have the same size; the buffer is NUL-terminated by the kernel.
        unsafe { std::slice::from_raw_parts(raw.as_ptr().cast::<u8>(), raw.len()) },
    )
    .map_err(|_| CwdProbe::Unknown("cwd path is not NUL-terminated".to_string()))?;
    if path.to_bytes().is_empty() {
        return Err(CwdProbe::Unknown("cwd path is empty".to_string()));
    }
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(path.to_bytes())))
}

/// The current directory of process `pid`.
#[cfg(target_os = "linux")]
pub fn cwd_of_pid(pid: u32) -> Result<PathBuf, CwdProbe> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).map_err(|e| CwdProbe::Unknown(e.to_string()))
}

/// Contract A: no Windows probe yet, and no substitute that could pass for one.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn cwd_of_pid(_pid: u32) -> Result<PathBuf, CwdProbe> {
    Err(CwdProbe::Unsupported)
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;

    #[test]
    fn probes_our_own_cwd() {
        let expected = std::env::current_dir().unwrap().canonicalize().unwrap();
        let probed = cwd_of_pid(std::process::id())
            .expect("own cwd")
            .canonicalize()
            .unwrap();
        assert_eq!(probed, expected);
    }

    #[test]
    fn probes_a_child_launched_elsewhere() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .current_dir(dir.path())
            .spawn()
            .unwrap();
        let probed = cwd_of_pid(child.id()).expect("child cwd");
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(
            probed.canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn a_gone_process_is_unknown_not_a_path() {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(matches!(cwd_of_pid(pid), Err(CwdProbe::Unknown(_))));
    }
}
