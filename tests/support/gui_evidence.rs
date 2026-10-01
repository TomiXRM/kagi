//! Failure evidence for the GUI E2E runner (#516 slice 1).
//!
//! When a scenario panics, the runner leaves `target/gui-e2e/<scenario>/`
//! behind (under `$CARGO_TARGET_DIR` when set) and prints one
//! `[gui-e2e] FAIL <scenario>: evidence <dir>` line:
//!
//! - `panic.txt`: the panic message and location.
//! - `klog-tail.txt`: the last `klog::TAIL_LINES` `[kagi]` lines.
//! - `fixture.txt`: for each repository the scenario mounted,
//!   `git status --short` and `git log --oneline -5`.
//! - `window-<n>.png` for each live window, or `window.txt` saying why there
//!   is none.
//!
//! The text files are written from the panic hook, **before** unwinding,
//! because unwinding drops the scenario's fixture `TempDir`s. The windows
//! belong to the app context, not the scenario's stack, so they are captured
//! after `catch_unwind` returns. A panic caught inside the scenario (a
//! recovery boundary) also reaches the hook; the next panic overwrites it,
//! and a scenario that passes deletes it.
//!
//! Windows are captured where they are. They stay off-screen and are never
//! brought to the front: a failing run must not take over the developer's
//! screen. `screencapture -l` either gets an image of such a window or not
//! (off-screen, no Screen Recording permission); `window.txt` records which.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Default)]
struct Failure {
    scenario: &'static str,
    fixtures: Vec<PathBuf>,
    /// Set by the panic hook once the text evidence is written.
    dir: Option<PathBuf>,
}

thread_local! {
    static CURRENT: RefCell<Failure> = RefCell::new(Failure::default());
}

/// Where a scenario's evidence goes.
fn evidence_dir(scenario: &str) -> PathBuf {
    let base = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
    PathBuf::from(base).join("gui-e2e").join(scenario)
}

/// Keep the `[kagi]` tail and write the text evidence on any panic of this
/// thread. Chains to the hook that was installed before.
///
/// The hook runs for panics a recovery boundary catches as well (e.g. an
/// executor panic turned into a `GitError`), so every panic rewrites the
/// evidence: the last one before the scenario unwinds is the failure. A
/// scenario that passes removes what its caught panics wrote ([`passed`]).
pub(crate) fn install() {
    kagi_ui_core::klog::keep_tail();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        CURRENT.with(|current| {
            // A panic while the state is borrowed (inside this module) writes
            // nothing rather than panicking again.
            if let Ok(mut current) = current.try_borrow_mut() {
                if !current.scenario.is_empty() {
                    current.dir = write_text(&current, &info.to_string());
                }
            }
        });
        previous(info);
    }));
}

/// The scenario returned normally: panics it caught are not a failure, so
/// drop the evidence they wrote.
pub(crate) fn passed() {
    let written = CURRENT.with(|current| current.borrow_mut().dir.take());
    if let Some(dir) = written {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// A scenario starts: forget the last one's fixtures, and drop evidence an
/// earlier failed run of this scenario left, so a directory that exists
/// always belongs to the latest run.
pub(crate) fn begin(scenario: &'static str) {
    let _ = std::fs::remove_dir_all(evidence_dir(scenario));
    CURRENT.with(|current| {
        *current.borrow_mut() = Failure {
            scenario,
            ..Failure::default()
        }
    });
}

/// The scenario mounted a window on the repository at `path`.
pub(crate) fn fixture(path: &Path) {
    CURRENT.with(|current| current.borrow_mut().fixtures.push(path.to_path_buf()));
}

fn write_text(failure: &Failure, panic: &str) -> Option<PathBuf> {
    let dir = evidence_dir(failure.scenario);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    let _ = std::fs::write(dir.join("panic.txt"), format!("{panic}\n"));
    let mut tail = kagi_ui_core::klog::tail().join("\n");
    tail.push('\n');
    let _ = std::fs::write(dir.join("klog-tail.txt"), tail);
    let _ = std::fs::write(dir.join("fixture.txt"), fixture_report(&failure.fixtures));
    Some(dir)
}

fn fixture_report(fixtures: &[PathBuf]) -> String {
    if fixtures.is_empty() {
        return "no repository was mounted through `mount` / `mount_state`\n".to_string();
    }
    let mut report = String::new();
    for path in fixtures {
        let _ = writeln!(report, "== {}", path.display());
        for args in [&["status", "--short"][..], &["log", "--oneline", "-5"][..]] {
            let _ = writeln!(report, "$ git {}", args.join(" "));
            match Command::new("git").current_dir(path).args(args).output() {
                Ok(out) => {
                    report.push_str(&String::from_utf8_lossy(&out.stdout));
                    report.push_str(&String::from_utf8_lossy(&out.stderr));
                }
                Err(error) => {
                    let _ = writeln!(report, "(git did not run: {error})");
                }
            }
        }
    }
    report
}

/// After the unwind: capture the live windows into the directory the hook
/// wrote, and print where it all is.
pub(crate) fn finish() {
    let (scenario, dir) = CURRENT.with(|current| {
        let current = current.borrow();
        (current.scenario, current.dir.clone())
    });
    let Some(dir) = dir else {
        eprintln!("[gui-e2e] FAIL {scenario}: no evidence written");
        return;
    };
    let notes = capture_windows(&dir);
    if !notes.is_empty() {
        let _ = std::fs::write(dir.join("window.txt"), notes);
    }
    let shown = std::path::absolute(&dir).unwrap_or(dir);
    eprintln!("[gui-e2e] FAIL {scenario}: evidence {}", shown.display());
}

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    fn sel_registerName(name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    fn objc_msgSend();
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {}

type Id = *mut std::ffi::c_void;

/// `[receiver selector]` for a selector returning an object or integer.
///
/// # Safety
/// `receiver` must be a live Objective-C object answering `selector` with no
/// arguments and a pointer-sized result.
unsafe fn send(receiver: Id, selector: &std::ffi::CStr) -> usize {
    let call: unsafe extern "C" fn(Id, Id) -> usize =
        std::mem::transmute(objc_msgSend as *const ());
    call(receiver, sel_registerName(selector.as_ptr()))
}

/// `[receiver selector index]`.
///
/// # Safety
/// As [`send`], for a selector taking one `NSUInteger`.
unsafe fn send_index(receiver: Id, selector: &std::ffi::CStr, index: usize) -> usize {
    let call: unsafe extern "C" fn(Id, Id, usize) -> usize =
        std::mem::transmute(objc_msgSend as *const ());
    call(receiver, sel_registerName(selector.as_ptr()), index)
}

/// `[receiver selector]` for a selector returning `BOOL` (one byte: `bool` on
/// arm64, `signed char` on x86_64).
///
/// # Safety
/// `receiver` must be a live Objective-C object answering `selector` with no
/// arguments and a `BOOL` result.
unsafe fn send_bool(receiver: Id, selector: &std::ffi::CStr) -> bool {
    let call: unsafe extern "C" fn(Id, Id) -> std::ffi::c_schar =
        std::mem::transmute(objc_msgSend as *const ());
    call(receiver, sel_registerName(selector.as_ptr())) != 0
}

/// `[receiver selector]` for a selector returning `NSInteger`.
///
/// # Safety
/// As [`send`], for an `NSInteger` result.
unsafe fn send_integer(receiver: Id, selector: &std::ffi::CStr) -> isize {
    let call: unsafe extern "C" fn(Id, Id) -> isize =
        std::mem::transmute(objc_msgSend as *const ());
    call(receiver, sel_registerName(selector.as_ptr()))
}

/// The window numbers of this process's windows, and whether each is on screen.
fn app_windows() -> Vec<(isize, bool)> {
    // SAFETY: AppKit classes and selectors with the documented signatures,
    // called on the runner's main thread.
    unsafe {
        let app = send(
            objc_getClass(c"NSApplication".as_ptr()),
            c"sharedApplication",
        ) as Id;
        if app.is_null() {
            return Vec::new();
        }
        let windows = send(app, c"windows") as Id;
        let count = send(windows, c"count");
        (0..count)
            .map(|index| {
                let window = send_index(windows, c"objectAtIndex:", index) as Id;
                let number = send_integer(window, c"windowNumber");
                let visible = send_bool(window, c"isVisible");
                (number, visible)
            })
            .collect()
    }
}

/// `screencapture -l` each window as it is; the notes say what could not be
/// captured and why.
fn capture_windows(dir: &Path) -> String {
    let windows = app_windows();
    if windows.is_empty() {
        return "no live window\n".to_string();
    }
    let mut notes = String::new();
    for (index, (number, visible)) in windows.into_iter().enumerate() {
        let state = if visible {
            "on screen"
        } else {
            "off-screen (hidden)"
        };
        if number <= 0 {
            let _ = writeln!(notes, "window {index}: {state}, no window number");
            continue;
        }
        let png = dir.join(format!("window-{index}.png"));
        let output = Command::new("screencapture")
            .args(["-x", "-o", &format!("-l{number}")])
            .arg(&png)
            .output();
        let size = std::fs::metadata(&png).map(|m| m.len()).unwrap_or(0);
        match output {
            Ok(out) if out.status.success() && size > 0 => {}
            Ok(out) => {
                let _ = std::fs::remove_file(&png);
                let _ = writeln!(
                    notes,
                    "window {index} (#{number}): {state}; screencapture {} gave no image: {} \
                     (an off-screen window, or no Screen Recording permission)",
                    out.status,
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            Err(error) => {
                let _ = writeln!(
                    notes,
                    "window {index} (#{number}): screencapture did not run: {error}"
                );
            }
        }
    }
    notes
}
