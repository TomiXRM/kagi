//! ADR-0166 — in-process GUI E2E PoC (Lane 1, macOS native).
//!
//! Proves that gpui's `VisualTestAppContext` (unlocked by the `test-support`
//! feature — declared **test-only** in the root `Cargo.toml`'s
//! `[dev-dependencies]`, so it never reaches the release binary) can drive the
//! REAL `KagiApp` root view against a fixture repo:
//!
//!   1. build a temp git repo with two commits (shelled `git`, read-only WRT
//!      the working tree once created — no git2 token, honouring the src/ui gate);
//!   2. mount the real `KagiApp` (same `cx.new` closure as `open_main_window`)
//!      in an off-screen window rendered by real Metal;
//!   3. dispatch a keyboard input (`cmd-j`) AND a registered `Action`
//!      (`ToggleBottomPanel`) through the context — not direct method calls;
//!   4. settle on the deterministic `TestDispatcher` (`run_until_parked`, no
//!      real-time sleep);
//!   5. assert observable Kagi state (`bottom_panel_open`) flips then restores;
//!   6. capture before/after PNGs (bundled fonts via `KagiAssets`);
//!   7. assert the scenario is read-only — repo HEAD + `git status` unchanged.
//!
//! macOS-only and `#[ignore]`d: `VisualTestAppContext` needs the macOS main
//! thread (AppKit window creation SIGABRTs on libtest worker threads), so run
//! it explicitly:
//!
//!   CARGO_TARGET_DIR=… cargo test -p kagi poc_toggle_bottom_panel_visual \
//!       -- --ignored --test-threads=1 --nocapture
//!
//! PNGs land in `$CARGO_TARGET_DIR/gui_e2e_poc/{before,after}.png`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use gpui::{px, size, AppContext, Entity, KeyBinding, Styled, VisualTestAppContext};

use super::assets::KagiAssets;
use super::{fonts, oplog_panel, theme, toast_stack, KagiApp, ToggleBottomPanel};

/// `git` with a deterministic identity + no user config bleed-through.
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

#[test]
#[ignore = "macOS main thread + real Metal: run with --ignored --test-threads=1"]
fn poc_toggle_bottom_panel_visual() {
    // ── 1. fixture repo → real Kagi snapshot (via kagi_git, no git2 token) ──
    let fixture = build_fixture();
    let repo_path = fixture.path().canonicalize().unwrap();
    let before_fp = repo_fingerprint(&repo_path);

    let info = kagi_git::open_repository(&repo_path).expect("open_repository");
    let mut backend = kagi_git::Backend::open(&repo_path).expect("Backend::open");
    let snap = backend.snapshot(10_000).expect("snapshot");
    assert!(
        snap.commits.len() >= 2,
        "fixture should have 2 commits, got {}",
        snap.commits.len()
    );

    // ── 2. VisualTestAppContext with the real Mac platform + bundled assets ──
    theme::init_active(); // resolve theme tokens (defaults to Catppuccin Mocha)
    let platform = gpui_platform::current_platform(false);
    let mut cx = VisualTestAppContext::with_asset_source(platform, Arc::new(KagiAssets));
    assert!(
        cx.read(|_| true),
        "app context constructed with test-support"
    );

    // App-level one-time init, mirroring `run_app` (the parts a render needs).
    cx.update(|cx| {
        fonts::load_bundled_fonts(cx);
        gpui_component::init(cx);
        theme::sync_gpui_component_theme(cx);
        // The one binding this PoC exercises: cmd-j → ToggleBottomPanel.
        cx.bind_keys([KeyBinding::new("secondary-j", ToggleBottomPanel, None)]);
    });

    // Build the real KagiApp state from the fixture snapshot (as main.rs does).
    let mut app_state = KagiApp::from_snapshot(&info.name, &snap);
    app_state.repo_path = Some(repo_path.clone());
    app_state.tabs.push(super::tabs::RepoTab {
        path: repo_path.clone(),
        name: info.name.clone(),
        remote: None,
        is_worktree: info.is_worktree,
        wt_color_idx: None,
    });
    app_state.active_tab = 0;

    // Capture the inner KagiApp entity so we can read observable state later.
    let kagi_cell: std::rc::Rc<std::cell::RefCell<Option<Entity<KagiApp>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));

    // ── 3. Off-screen window mounting the REAL root (same closure shape as
    //        open_main_window: KagiApp entity wrapped in gpui_component::Root) ──
    let build_cell = kagi_cell.clone();
    let window = cx
        .open_offscreen_window(size(px(1440.0), px(900.0)), move |window, cx| {
            let kagi: Entity<KagiApp> = cx.new(|cx| {
                app_state.root_focus = Some(cx.focus_handle());
                app_state.toast_stack = Some(cx.new(|_| toast_stack::ToastStack::new()));
                let seed = std::mem::take(&mut app_state.op_log_seed);
                app_state.op_log = Some(cx.new(|_| oplog_panel::OpLogPanel::from_entries(seed)));
                app_state
            });
            if let Some(fh) = kagi.read(cx).root_focus.clone() {
                window.focus(&fh, cx);
            }
            *build_cell.borrow_mut() = Some(kagi.clone());
            cx.new(|cx| gpui_component::Root::new(kagi, window, cx).font(theme::ui_font()))
        })
        .expect("open_offscreen_window");

    eprintln!("[poc] checkpoint: window opened + first render done");
    let kagi = kagi_cell.borrow().clone().expect("kagi entity captured");
    let win = window.into();

    // Settle the first frame deterministically (no real-time sleep).
    cx.run_until_parked();
    eprintln!("[poc] checkpoint: run_until_parked done");

    // ── 5a. Observable state before any input ──
    let initial = cx.read(|app| kagi.read(app).bottom_panel_open);

    // ── 6a. Before screenshot (bundled fonts render through KagiAssets) ──
    eprintln!("[poc] checkpoint: about to capture_screenshot (before)");
    let before = cx.capture_screenshot(win).expect("capture before");
    eprintln!("[poc] checkpoint: capture_screenshot (before) returned");
    assert!(
        before.width() > 0 && before.height() > 0,
        "before screenshot is empty"
    );
    let before_nonwhite = before.pixels().any(|p| p.0 != [255, 255, 255, 255]);
    assert!(before_nonwhite, "before screenshot is a blank white frame");
    let before_path = out_dir().join("before.png");
    before.save(&before_path).expect("save before.png");

    // ── 3+4. Keyboard input → registered Action, settled on TestDispatcher ──
    cx.simulate_keystrokes(win, "cmd-j"); // keyboard: dispatches ToggleBottomPanel
    let after_key = cx.read(|app| kagi.read(app).bottom_panel_open);
    assert_eq!(
        after_key, !initial,
        "cmd-j keystroke should toggle bottom_panel_open ({initial} -> {})",
        !initial
    );

    // Now the *action* path (not a direct method call): dispatch the registered
    // ToggleBottomPanel action, which restores the original state.
    cx.dispatch_action(win, ToggleBottomPanel);
    let after_action = cx.read(|app| kagi.read(app).bottom_panel_open);
    assert_eq!(
        after_action, initial,
        "ToggleBottomPanel action should restore bottom_panel_open to {initial}"
    );

    // ── 6b. After screenshot ──
    let after = cx.capture_screenshot(win).expect("capture after");
    let after_path = out_dir().join("after.png");
    after.save(&after_path).expect("save after.png");

    // ── 7. Read-only proof: nothing touched the repository ──
    let after_fp = repo_fingerprint(&repo_path);
    assert_eq!(
        before_fp, after_fp,
        "repo mutated during a read-only scenario"
    );

    eprintln!(
        "[poc] before={} after={} initial_bottom_panel_open={initial}",
        before_path.display(),
        after_path.display()
    );
}
