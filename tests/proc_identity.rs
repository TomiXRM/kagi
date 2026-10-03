//! The process identity the GUI E2E runner's KEEP_GOING cleanup kills by
//! (`support/proc_identity.rs`, #967 review): a pid is the recorded process
//! only if it started at the same microsecond.
#![cfg(target_os = "macos")]

#[path = "support/proc_identity.rs"]
mod proc_identity;

use proc_identity::{identity, sigkill, started, Identity, Started};
use std::process::{Child, Command};

fn sleeper() -> Child {
    Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .expect("spawn sleep")
}

#[test]
fn a_start_time_one_microsecond_off_is_another_process() {
    let mut child = sleeper();
    let pid = child.id();
    let at = started(pid)
        .unwrap()
        .expect("a live child has a start time");
    assert_eq!(identity(pid, at), Identity::Same);

    // Same pid, same second: what `ps -o lstart` cannot tell apart.
    let usec = if at.usec == 0 { 1 } else { at.usec - 1 };
    assert_eq!(identity(pid, Started { usec, ..at }), Identity::Gone);

    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn processes_started_back_to_back_have_distinct_start_times() {
    let mut a = sleeper();
    let mut b = sleeper();
    let at_a = started(a.id()).unwrap().expect("a");
    let at_b = started(b.id()).unwrap().expect("b");
    assert_ne!(
        at_a, at_b,
        "the kernel's start time does not resolve below a second"
    );
    for child in [&mut a, &mut b] {
        child.kill().unwrap();
        child.wait().unwrap();
    }
}

#[test]
fn an_exited_process_is_gone_reaped_or_not() {
    let mut child = sleeper();
    let pid = child.id();
    let at = started(pid).unwrap().expect("live");
    sigkill(pid as i32).unwrap();
    // Dead but unreaped (a zombie) is not the recorded process any more.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while identity(pid, at) == Identity::Same {
        assert!(
            std::time::Instant::now() < deadline,
            "SIGKILL did not end it"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(identity(pid, at), Identity::Gone);
    child.wait().unwrap();
    assert_eq!(identity(pid, at), Identity::Gone);
    // Signalling what is no longer there is not an error.
    assert_eq!(sigkill(pid as i32), Ok(()));
}
