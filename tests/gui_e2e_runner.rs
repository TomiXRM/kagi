//! ADR-0166 — GUI E2E main-thread runner (Lane 1, macOS native).
//!
//! This is the GREEN path the PoC (`#[ignore]`d unit test) could only gesture
//! at. It is a `harness = false` test target (see `Cargo.toml`): it owns
//! `fn main`, so it runs on the **process main thread** — where AppKit
//! `NSWindow` creation succeeds. libtest's default harness runs `#[test]` fns on
//! worker threads, where the same window creation SIGABRTs; that is the sole
//! reason the PoC stayed `#[ignore]`d.
//!
//! It links the `kagi` lib (which now hosts `ui`, ADR-0166) and drives the REAL
//! `KagiApp` root through `gpui::VisualTestAppContext` (unlocked by the
//! `test-support` **dev**-dependency feature — never in the release binary).
//! Every normal-dep detail (`gpui_component`, `gpui_platform`, `kagi_git`) is
//! hidden behind the `kagi::ui::e2e` seam, so this file touches only `gpui`
//! (dev-dep), `image`/`tempfile` (dev-deps), and `kagi`.
//!
//! Scenario (read-only, plan-or-before): mount → keyboard `cmd-j` → registered
//! `ToggleBottomPanel` action → deterministic settle → observable-state assert →
//! before/after screenshot → repo-unchanged assert. Exits 0 on success, non-zero
//! (panic → 101, or explicit) on failure.
//!
//! Run (opt-in — see the `KAGI_GUI_E2E` guard in `run`):
//!   KAGI_GUI_E2E=1 CARGO_TARGET_DIR=…/target \
//!     cargo test -p kagi --test gui_e2e_runner -- --nocapture
//!
//! Without `KAGI_GUI_E2E`, and on `cargo test --workspace`, it prints SKIP and
//! exits 0, so it never gates the normal suite (ADR-0166 §CI: evidence lane).
//!
//! PNGs would land in `$CARGO_TARGET_DIR/gui_e2e_poc/{before,after}.png` — but
//! the locked gpui rev does not implement `render_to_image` for the real Mac
//! window, so capture is best-effort and currently skipped (state assertions,
//! not the screenshot, are the pass/fail oracle — ADR-0166 §3).

#[cfg(not(target_os = "macos"))]
fn main() {
    // The visual driver is macOS-only (real Metal + AppKit). Elsewhere this
    // target is a no-op success so `cargo test --workspace` stays green.
    eprintln!("[gui-e2e] SKIP: VisualTestAppContext is macOS-only");
}

#[cfg(target_os = "macos")]
fn main() {
    std::process::exit(macos::run());
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use gpui::{px, size, Entity, VisualTestAppContext};
    use kagi::ui::{e2e, KagiApp, ToggleBottomPanel};

    /// `git` with a deterministic identity + no user-config bleed-through.
    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "poc")
            .env("GIT_AUTHOR_EMAIL", "poc@example.com")
            .env("GIT_COMMITTER_NAME", "poc")
            .env("GIT_COMMITTER_EMAIL", "poc@example.com")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .expect("spawn git");
        assert!(status.success(), "git {args:?} failed");
    }

    /// A throwaway repo with two commits on `main`.
    fn build_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        std::fs::write(p.join("README.md"), "# fixture\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "initial commit"]);
        std::fs::write(p.join("README.md"), "# fixture\nsecond line\n").unwrap();
        git(p, &["commit", "-q", "-am", "second commit"]);
        dir
    }

    /// `git rev-parse HEAD` + porcelain status, for the no-mutation assertion.
    fn repo_fingerprint(dir: &Path) -> (String, String) {
        let head = Command::new("git")
            .current_dir(dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("rev-parse");
        let status = Command::new("git")
            .current_dir(dir)
            .args(["status", "--porcelain"])
            .output()
            .expect("status");
        (
            String::from_utf8_lossy(&head.stdout).trim().to_string(),
            String::from_utf8_lossy(&status.stdout).to_string(),
        )
    }

    fn out_dir() -> PathBuf {
        let base = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
        let dir = PathBuf::from(base).join("gui_e2e_poc");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The scenario. Returns a process exit code (0 = pass).
    pub fn run() -> i32 {
        // Opt-in: real Metal + a main-thread window is an *evidence lane*, not a
        // required gate (ADR-0166 §CI). Unset → skip so `cargo test --workspace`
        // stays fast and non-flaky. Set `KAGI_GUI_E2E=1` to run it.
        if std::env::var_os("KAGI_GUI_E2E").is_none() {
            eprintln!("[gui-e2e] SKIP: set KAGI_GUI_E2E=1 to run the visual scenario");
            return 0;
        }

        // ── 1. fixture repo → real Kagi snapshot (built inside the seam via
        //        kagi_git — no git2 token reaches this crate) ──
        let fixture = build_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        let before_fp = repo_fingerprint(&repo_path);

        // ── 2. VisualTestAppContext with the real Mac platform + bundled assets ──
        kagi::ui::theme::init_active(); // resolve theme tokens (Catppuccin Mocha default)
        let mut cx = VisualTestAppContext::with_asset_source(e2e::platform(), e2e::asset_source());

        // App-level one-time init, mirroring `run_app` (fonts, gpui_component,
        // theme sync, the cmd-j → ToggleBottomPanel binding this exercises).
        cx.update(e2e::init_app);

        // Build the real KagiApp state from the fixture snapshot.
        let app_state = e2e::app_state(&repo_path).expect("build app_state");

        // Capture the inner KagiApp entity so observable state is readable.
        let kagi_cell: std::rc::Rc<std::cell::RefCell<Option<Entity<KagiApp>>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));

        // ── 3. Off-screen window mounting the REAL root (same entity build as
        //        open_main_window, shared via e2e::build_kagi_entity) ──
        let build_cell = kagi_cell.clone();
        let window = cx
            .open_offscreen_window(size(px(1440.0), px(900.0)), move |window, cx| {
                e2e::mount_root(app_state, window, cx, &build_cell)
            })
            .expect("open_offscreen_window");

        eprintln!("[gui-e2e] checkpoint: window opened + first render done");
        let kagi = kagi_cell.borrow().clone().expect("kagi entity captured");
        let win = window.into();

        // Settle the first frame deterministically (no real-time sleep).
        cx.run_until_parked();
        eprintln!("[gui-e2e] checkpoint: run_until_parked done");

        // ── 5a. Observable state before any input ──
        let initial = cx.read(|app| kagi.read(app).bottom_panel_open);

        // ── 6a. Before screenshot — BEST EFFORT (ADR-0166 §3: screenshot is a
        //        triage signal, not the pass/fail oracle; the state asserts are).
        //        The locked gpui rev does not implement `render_to_image` for the
        //        real Mac window, so capture returns an error there; the scenario
        //        still runs GREEN on its deterministic assertions. ──
        capture_screenshot_best_effort(&mut cx, win, "before");

        // ── 3+4. Keyboard input → registered Action, settled on TestDispatcher ──
        cx.simulate_keystrokes(win, "cmd-j"); // keyboard: dispatches ToggleBottomPanel
        let after_key = cx.read(|app| kagi.read(app).bottom_panel_open);
        assert_eq!(
            after_key, !initial,
            "cmd-j keystroke should toggle bottom_panel_open ({initial} -> {})",
            !initial
        );

        // The *action* path (not a direct method call): dispatch the registered
        // ToggleBottomPanel action, which restores the original state.
        cx.dispatch_action(win, ToggleBottomPanel);
        let after_action = cx.read(|app| kagi.read(app).bottom_panel_open);
        assert_eq!(
            after_action, initial,
            "ToggleBottomPanel action should restore bottom_panel_open to {initial}"
        );

        // ── 6b. After screenshot — best effort (see above). ──
        capture_screenshot_best_effort(&mut cx, win, "after");

        // ── 7. Read-only proof: nothing touched the repository ──
        let after_fp = repo_fingerprint(&repo_path);
        assert_eq!(
            before_fp, after_fp,
            "repo mutated during a read-only scenario"
        );

        eprintln!("[gui-e2e] PASS initial_bottom_panel_open={initial}");
        0
    }

    /// Try to capture a PNG; tolerate the locked gpui rev's unimplemented
    /// `render_to_image` for the real Mac window. When capture works, assert the
    /// frame is non-blank and save it to `$CARGO_TARGET_DIR/gui_e2e_poc/<tag>.png`.
    fn capture_screenshot_best_effort(
        cx: &mut VisualTestAppContext,
        win: gpui::AnyWindowHandle,
        tag: &str,
    ) {
        match cx.capture_screenshot(win) {
            Ok(img) => {
                assert!(
                    img.width() > 0 && img.height() > 0,
                    "{tag} screenshot empty"
                );
                assert!(
                    img.pixels().any(|p| p.0 != [255, 255, 255, 255]),
                    "{tag} screenshot is a blank white frame"
                );
                let path = out_dir().join(format!("{tag}.png"));
                img.save(&path).expect("save png");
                eprintln!("[gui-e2e] screenshot {tag}: {}", path.display());
            }
            Err(e) => eprintln!(
                "[gui-e2e] screenshot {tag}: skipped (gpui render_to_image \
                 unavailable on this platform: {e})"
            ),
        }
    }
}
