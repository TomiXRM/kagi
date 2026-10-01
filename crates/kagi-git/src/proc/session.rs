//! Read-only count of the processes left in a terminal shell's session (#867).
//!
//! Kagi's PTY spawn (`portable-pty` 0.9) runs `setsid()` before `exec`, so the
//! shell leads a new session whose id is its PID. Its descendants stay in that
//! session unless they call `setsid` themselves, even after the shell exits:
//! `nohup`, `disown` and bash's plain `&` jobs all outlive `exit` (observed on
//! macOS with zsh and bash, ADR-0171 Follow-up 1). Counting the members of
//! that session finds them without looking at any process Kagi did not start.
//! Nothing here signals a process.

/// How many processes other than the leader `sid` belong to session `sid`.
#[cfg(target_os = "macos")]
pub fn session_members(sid: u32) -> std::io::Result<usize> {
    // SAFETY: a null buffer asks only for the current number of PIDs.
    let estimate = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if estimate <= 0 {
        return Err(std::io::Error::last_os_error());
    }
    // Headroom for processes started between the two calls.
    let mut pids = vec![0 as libc::pid_t; estimate as usize + 64];
    let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
    // SAFETY: the pointer and byte size describe `pids` exactly.
    let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    if count < 0 {
        return Err(std::io::Error::last_os_error());
    }
    pids.truncate(count as usize);
    Ok(pids
        .into_iter()
        .filter(|&pid| pid > 0 && pid as u32 != sid)
        // SAFETY: `getsid` only reads; a PID that has exited returns -1.
        .filter(|&pid| unsafe { libc::getsid(pid) } == sid as libc::pid_t)
        .count())
}

/// How many processes other than the leader `sid` belong to session `sid`.
#[cfg(target_os = "linux")]
pub fn session_members(sid: u32) -> std::io::Result<usize> {
    let mut count = 0;
    for entry in std::fs::read_dir("/proc")? {
        let Ok(entry) = entry else { continue };
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == sid {
            continue;
        }
        // An exited process vanishes between listing and reading: not a member.
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // `pid (comm) state ppid pgrp session …`; `comm` may contain spaces.
        let session = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().nth(3))
            .and_then(|s| s.parse::<u32>().ok());
        if session == Some(sid) {
            count += 1;
        }
    }
    Ok(count)
}

/// No session probe on this platform.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn session_members(_sid: u32) -> std::io::Result<usize> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::session_members;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    /// A process that leads its own session, with one background child that
    /// outlives it — the shape a `nohup` job leaves behind a Kagi shell.
    #[test]
    fn counts_what_an_exited_leader_left_in_its_session() {
        let mut leader = Command::new("/bin/sh");
        leader
            .args(["-c", "sleep 3091 </dev/null >/dev/null 2>&1 & echo $!"])
            .stdout(Stdio::piped());
        // SAFETY: `setsid` is async-signal-safe and touches only this child.
        unsafe {
            leader.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = leader.spawn().expect("spawn the leader");
        let sid = child.id();
        let out = child.wait_with_output().expect("the leader exits");
        let left: libc::pid_t = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();

        assert_eq!(
            session_members(sid).unwrap(),
            1,
            "the sleep is still in the session"
        );

        // SAFETY: `left` is the test's own child, by its PID.
        unsafe { libc::kill(left, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(5);
        while session_members(sid).unwrap() != 0 {
            assert!(
                Instant::now() < deadline,
                "the stopped sleep is no longer counted"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
