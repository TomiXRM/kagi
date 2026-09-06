//! Run one integration test in a child process with its own log directory.
//!
//! `KAGI_LOG_DIR` is process-global. Keeping it out of the parent harness
//! avoids races with sibling test threads while preserving normal parallelism.

use std::process::Command;

/// Returns `true` only in the isolated child that should execute the test body.
///
/// A parent test launches its exact test name with a fresh `KAGI_LOG_DIR`,
/// verifies the child succeeded, then returns `false` so its body can exit.
pub fn run_isolated() -> bool {
    let thread = std::thread::current();
    let test_name = thread.name().expect("test thread has a name");
    if std::env::var("KAGI_TEST_ISOLATED").ok().as_deref() == Some(test_name) {
        return true;
    }

    let log_dir = tempfile::tempdir().expect("isolated log directory");
    let output = Command::new(std::env::current_exe().expect("current test executable"))
        .args(["--exact", "--include-ignored", test_name])
        .env("KAGI_LOG_DIR", log_dir.path())
        .env("KAGI_TEST_ISOLATED", test_name)
        .output()
        .expect("start isolated test child");
    assert!(
        output.status.success(),
        "isolated test {test_name} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    false
}
