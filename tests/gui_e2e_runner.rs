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
//! explicit `gui-e2e` feature; default builds leave `test-support` disabled).
//! Every normal-dep detail (`gpui_component`, `gpui_platform`, `kagi_git`) is
//! hidden behind the `kagi::ui::e2e` seam, so this file touches only `gpui`
//! (normal dependency), `image`/`tempfile` (dev-deps), and `kagi`.
//!
//! Each scenario mounts the real root, drives it (keystroke / registered action
//! / entity update), settles deterministically, and asserts observable state —
//! `KagiApp` fields, the clipboard, or repo refs (ADR-0166 §3: the assertions
//! are the oracle, screenshots are triage only). Coverage: bottom-panel toggle
//! (PoC), graph Cmd+C copy (ADR-0170), Create Snapshot (#335), command-palette
//! theme switch (#373), agent provenance (#337), WIP→HEAD connectors (#472),
//! and the linked-worktree commit panel's writes (#473 / #476 slices 1–3).
//! Exits 0 on success, non-zero (panic → 101) on failure.
//!
//! Run (opt-in — see the `KAGI_GUI_E2E` guard in `run`):
//!   KAGI_GUI_E2E=1 CARGO_TARGET_DIR=/Users/tomixrm/Dev/sandbox/git-client/target \
//!     cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
//!
//! Default workspace tests omit this target (including its recovery modules).
//! With `gui-e2e` but without `KAGI_GUI_E2E`, it still prints SKIP and exits 0.
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
#[path = "recovery/operations.rs"]
mod recovery_operations;

#[cfg(target_os = "macos")]
#[path = "recovery/app_remove.rs"]
mod app_remove;

#[cfg(target_os = "macos")]
#[path = "recovery/app_stash.rs"]
mod app_stash;

#[cfg(target_os = "macos")]
#[path = "recovery/app_writer_admission.rs"]
mod app_writer_admission;

#[cfg(target_os = "macos")]
#[path = "recovery/layout.rs"]
mod recovery_layout;

#[cfg(target_os = "macos")]
mod macos {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui::{px, size, AnyWindowHandle, Entity, VisualTestAppContext};
    use kagi::graph::{EdgeKind, GraphEdge};
    use kagi::ui::{
        commands::CreateSnapshot, commit_list, e2e, editor_tree_menu::EditorTreeAction, graph_wip,
        oplog_panel, settings::CopyTarget, theme, BottomTab, CopyDiffSelection, KagiApp,
        ToggleBottomPanel,
    };

    #[link(name = "objc")]
    extern "C" {
        fn objc_autoreleasePoolPush() -> *mut std::ffi::c_void;
        fn objc_autoreleasePoolPop(pool: *mut std::ffi::c_void);
    }

    /// The offscreen runner has no AppKit event loop to drain native windows.
    /// MacWindow::drop queues close/autorelease; drain while GPUI is still alive
    /// so native frame callbacks release their captured InputState entities.
    struct NativeAutoreleasePool(*mut std::ffi::c_void);

    impl NativeAutoreleasePool {
        fn new() -> Self {
            Self(unsafe { objc_autoreleasePoolPush() })
        }
    }

    impl Drop for NativeAutoreleasePool {
        fn drop(&mut self) {
            // Created and drained on this runner's macOS main thread.
            unsafe { objc_autoreleasePoolPop(self.0) };
        }
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFRunLoopDefaultMode: *const std::ffi::c_void;
        fn CFRunLoopRunInMode(
            mode: *const std::ffi::c_void,
            seconds: f64,
            return_after_source: u8,
        ) -> i32;
    }

    fn drain_native_events() {
        // MacWindow closes on MacPlatform's native executor, not TestDispatcher.
        unsafe {
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.01, 0);
        }
    }

    /// `git` with a deterministic identity + no user-config bleed-through.
    pub(super) fn git(dir: &Path, args: &[&str]) {
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
    pub(super) fn build_fixture() -> tempfile::TempDir {
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

    /// A repo whose HEAD is an AI-agent commit (Claude Code, detected via the
    /// `Co-Authored-By: Claude <noreply@anthropic.com>` trailer — provenance
    /// Route 1) sitting on top of a plain human commit. Exercises issue #337's
    /// `CommitRow.provenance` classification through the real snapshot pipeline.
    fn build_agent_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        std::fs::write(p.join("f.txt"), "one\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "human commit"]);
        std::fs::write(p.join("f.txt"), "two\n").unwrap();
        git(p, &["add", "."]);
        git(
            p,
            &[
                "commit",
                "-q",
                "-m",
                "agent commit\n\nCo-Authored-By: Claude <noreply@anthropic.com>",
            ],
        );
        dir
    }

    /// Issue #472: a repo whose HEAD is buried — three commits stacked on a
    /// sibling branch put `main`'s HEAD at row 3 — with BOTH the main working
    /// tree and a linked worktree dirty, so the graph draws two WIP rows, each
    /// needing its own dashed connector down to its own HEAD.
    ///
    /// The worktree lives beside the repo, not inside it, so it does not show
    /// up as an untracked directory in the repo's own status.
    fn build_wip_connector_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().expect("tempdir");
        let repo = root.path().join("repo");
        let wt = root.path().join("wt");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("f.txt"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "base"]);
        // Three commits on top of HEAD, on another branch: the revwalk emits
        // children before parents, so `main`'s HEAD lands at row 3.
        git(&repo, &["checkout", "-q", "-b", "ahead"]);
        for i in 0..3 {
            std::fs::write(repo.join("f.txt"), format!("ahead {i}\n")).unwrap();
            git(&repo, &["commit", "-q", "-am", &format!("ahead {i}")]);
        }
        git(&repo, &["checkout", "-q", "main"]);
        git(
            &repo,
            &["worktree", "add", "-q", wt.to_str().unwrap(), "ahead"],
        );
        // Both working trees dirty → two WIP rows.
        std::fs::write(repo.join("dirty.txt"), "main\n").unwrap();
        std::fs::write(wt.join("dirty.txt"), "wt\n").unwrap();
        let repo = repo.canonicalize().unwrap();
        let wt = wt.canonicalize().unwrap();
        (root, repo, wt)
    }

    /// Issue #476 slice 2: `main` (dirty) plus **two** linked worktrees, both
    /// dirty and each on its own branch at its own commit — three WIP rows.
    ///
    /// Two worktrees, not one: committing from the first one's panel makes it
    /// clean, so its row leaves the list. With a second worktree BELOW it, a
    /// lane map keyed by row position hands that row its vanished neighbour's
    /// lane; keyed by target it keeps its own.
    fn build_two_worktree_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let root = tempfile::tempdir().expect("tempdir");
        let repo = root.path().join("repo");
        let wt_a = root.path().join("wt-a");
        let wt_b = root.path().join("wt-b");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("f.txt"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "base"]);
        // A distinct commit per branch, so each WIP row's connector has its own
        // HEAD row to land on (a shared HEAD would still work, but distinct
        // rows make a shifted lane unambiguous).
        for b in ["wt-a", "wt-b"] {
            git(&repo, &["checkout", "-q", "-b", b]);
            std::fs::write(repo.join("f.txt"), format!("{b}\n")).unwrap();
            git(&repo, &["commit", "-q", "-am", &format!("{b} commit")]);
            git(&repo, &["checkout", "-q", "main"]);
        }
        git(
            &repo,
            &["worktree", "add", "-q", wt_a.to_str().unwrap(), "wt-a"],
        );
        git(
            &repo,
            &["worktree", "add", "-q", wt_b.to_str().unwrap(), "wt-b"],
        );
        // All three working trees dirty → three WIP rows.
        std::fs::write(repo.join("dirty.txt"), "main\n").unwrap();
        std::fs::write(wt_a.join("dirty.txt"), "a\n").unwrap();
        std::fs::write(wt_b.join("dirty.txt"), "b\n").unwrap();
        (
            root,
            repo.canonicalize().unwrap(),
            wt_a.canonicalize().unwrap(),
            wt_b.canonicalize().unwrap(),
        )
    }

    /// `git rev-parse <rev>` for a working tree (#476 slice 3: `HEAD^`, to prove
    /// an amend replaced the tip instead of stacking on it).
    fn rev_parse(dir: &Path, rev: &str) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["rev-parse", rev])
            .output()
            .expect("rev-parse");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Did `git <args>` exit 0 in `dir`? (`cat-file -e <sha>` is an existence
    /// probe, so the exit code IS the answer.)
    fn git_ok(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .current_dir(dir)
            .args(args)
            .status()
            .expect("spawn git")
            .success()
    }

    /// `git rev-list --count HEAD` for a working tree.
    fn commit_count(dir: &Path) -> usize {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .expect("rev-list");
        String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
    }

    /// Overwrite `$KAGI_LOG_DIR/settings.json` (the isolated dir `run` sets) with
    /// a single flat string key — the on-disk shape `Settings::load` parses.
    fn write_setting(log_dir: &Path, key: &str, value: &str) {
        let json = format!("{{\n  \"{key}\": \"{value}\"\n}}\n");
        std::fs::write(log_dir.join("settings.json"), json).expect("write settings.json");
    }

    /// `git for-each-ref <pattern>` — the ref-existence probe for the snapshot
    /// scenario. Returns the raw stdout (one line per matching ref).
    fn for_each_ref(dir: &Path, pattern: &str) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["for-each-ref", pattern])
            .output()
            .expect("for-each-ref");
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// Mount the real `KagiApp` offscreen against `repo_path`, settle the first
    /// frame, and hand back the captured entity + window handle. Mirrors the
    /// PoC mount (ADR-0166) so every scenario builds the root identically.
    pub(super) fn mount(
        cx: &mut VisualTestAppContext,
        repo_path: &Path,
    ) -> (Entity<KagiApp>, AnyWindowHandle) {
        let app_state = e2e::app_state(repo_path).expect("build app_state");
        let cell: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::new(RefCell::new(None));
        let build_cell = cell.clone();
        let window = cx
            .open_offscreen_window(size(px(1440.0), px(900.0)), move |window, cx| {
                e2e::mount_root(app_state, window, cx, &build_cell)
            })
            .expect("open_offscreen_window");
        let kagi = cell.borrow().clone().expect("kagi entity captured");
        cx.run_until_parked();
        (kagi, window.into())
    }

    /// `git rev-parse HEAD` + porcelain status, for the no-mutation assertion.
    pub(super) fn repo_fingerprint(dir: &Path) -> (String, String) {
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

    /// The scenario suite. Returns a process exit code (0 = pass). Each scenario
    /// asserts observable `KagiApp` state / clipboard / repo refs (ADR-0166 §3:
    /// deterministic assertions are the oracle, screenshots are triage only) and
    /// prints a `[gui-e2e] PASS …` line. A failed assertion panics → exit 101.
    pub fn run() -> i32 {
        // Opt-in: real Metal + a main-thread window is an *evidence lane*, not a
        // required gate (ADR-0166 §CI). Unset → skip so `cargo test --workspace`
        // stays fast and non-flaky. Set `KAGI_GUI_E2E=1` to run it.
        if std::env::var_os("KAGI_GUI_E2E").is_none() {
            eprintln!("[gui-e2e] SKIP: set KAGI_GUI_E2E=1 to run the visual scenarios");
            return 0;
        }

        // Redirect settings.json to a throwaway dir so scenarios that touch
        // settings (graph_copy_target, theme via set_active) never read or clobber
        // the developer's real `~/.kagi/settings.json` (ADR-0091 flat-string file).
        let log_dir = tempfile::tempdir().expect("settings tempdir");
        std::env::set_var("KAGI_LOG_DIR", log_dir.path());

        // Shared context: real Mac platform + bundled assets, one-time app init
        // (fonts, gpui_component, theme sync, the cmd-j / cmd-c bindings).
        theme::init_active();
        let mut cx = VisualTestAppContext::with_asset_source(e2e::platform(), e2e::asset_source());
        let native_pool = NativeAutoreleasePool::new();
        cx.update(e2e::init_app);
        crate::recovery_operations::scenario_stash_drop_persists(&mut cx);
        crate::recovery_operations::scenario_history_persists(&mut cx);
        crate::recovery_operations::scenario_cleanup_stale_tab(&mut cx);
        crate::recovery_operations::scenario_preflight_presentation(&mut cx);
        crate::recovery_operations::scenario_cleanup_open_failure(&mut cx);
        crate::recovery_operations::scenario_cleanup_partial_presentation(&mut cx);
        crate::app_remove::scenario_remove_public_boundary(&mut cx);
        crate::app_writer_admission::scenario_editor_save_admission(&mut cx);
        crate::app_stash::scenario_stash_public_boundary(&mut cx);
        crate::app_stash::scenario_stash_conflict_followup(&mut cx);
        crate::app_stash::scenario_stash_replan_error(&mut cx);
        crate::app_stash::scenario_external_stash_conflict_has_no_drop_prompt(&mut cx);
        crate::recovery_layout::scenario_commit_row_layout(&mut cx);
        let history_fixture = build_fixture();
        let history_before = repo_fingerprint(history_fixture.path());
        crate::recovery_layout::scenario_editor_history_layout(&mut cx, history_fixture.path());
        assert_eq!(history_before, repo_fingerprint(history_fixture.path()));

        scenario_bottom_panel(&mut cx);
        scenario_graph_copy(&mut cx, log_dir.path());
        scenario_oplog_expand_copy(&mut cx);
        scenario_create_snapshot(&mut cx);
        scenario_theme_switch(&mut cx);
        scenario_agent_provenance(&mut cx);
        scenario_wip_head_connector(&mut cx);
        scenario_worktree_wip_inline(&mut cx);
        scenario_worktree_panel_commit(&mut cx);
        scenario_worktree_panel_amend_discard(&mut cx);
        cx.run_until_parked();
        drain_native_events();
        drop(native_pool);
        cx.update(|_| {});
        cx.run_until_parked();

        eprintln!("[gui-e2e] PASS all scenarios");
        0
    }

    /// PoC scenario (ADR-0166): cmd-j keystroke + `ToggleBottomPanel` action flip
    /// and restore `bottom_panel_open`; the repo is untouched (read-only proof).
    fn scenario_bottom_panel(cx: &mut VisualTestAppContext) {
        let fixture = build_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        let before_fp = repo_fingerprint(&repo_path);
        let (kagi, win) = mount(cx, &repo_path);

        let initial = cx.read(|app| kagi.read(app).bottom_panel_open);
        capture_screenshot_best_effort(cx, win, "before");

        cx.simulate_keystrokes(win, "cmd-j"); // keyboard → ToggleBottomPanel
        let after_key = cx.read(|app| kagi.read(app).bottom_panel_open);
        assert_eq!(
            after_key, !initial,
            "cmd-j keystroke should toggle bottom_panel_open ({initial} -> {})",
            !initial
        );

        cx.dispatch_action(win, ToggleBottomPanel); // registered action path
        let after_action = cx.read(|app| kagi.read(app).bottom_panel_open);
        assert_eq!(
            after_action, initial,
            "ToggleBottomPanel action should restore bottom_panel_open to {initial}"
        );

        capture_screenshot_best_effort(cx, win, "after");
        assert_eq!(
            before_fp,
            repo_fingerprint(&repo_path),
            "repo mutated during a read-only scenario"
        );
        eprintln!("[gui-e2e] PASS bottom_panel initial={initial}");
    }

    /// ADR-0170 graph Cmd+C: with a graph row selected and root focus, the
    /// `CopyDiffSelection` action (no diff selection → Graph gets first refusal)
    /// writes the row's full SHA (`graph_copy_target=hash`) or its local branch
    /// name (`…=branch`) to the clipboard. Asserts the clipboard text each way.
    fn scenario_graph_copy(cx: &mut VisualTestAppContext, log_dir: &Path) {
        let fixture = build_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        let (kagi, win) = mount(cx, &repo_path);

        // Select the HEAD row; capture its full SHA + the branch name the
        // production copy path would yield (via the real `graph_copy_value`, so
        // the badge-label decoration — `"main ✓"` etc. — is handled identically).
        let (full_sha, branch) = kagi.update(cx, |app, cx| {
            app.selected = Some(0);
            cx.notify();
            let row = &app.active_view.rows[0];
            let full_sha = row.id.0.clone();
            let branch = commit_list::graph_copy_value(&row.badges, &full_sha, CopyTarget::Branch);
            (full_sha, branch)
        });
        assert_ne!(
            branch, full_sha,
            "HEAD row should resolve to a local branch (not fall back to the SHA)"
        );
        cx.run_until_parked();

        // hash mode → clipboard == full SHA
        write_setting(log_dir, "graph_copy_target", "hash");
        cx.dispatch_action(win, CopyDiffSelection);
        let copied = cx.read_from_clipboard().and_then(|i| i.text());
        assert_eq!(
            copied.as_deref(),
            Some(full_sha.as_str()),
            "graph Cmd+C (hash) should copy the full SHA"
        );

        // branch mode → clipboard == local branch name
        write_setting(log_dir, "graph_copy_target", "branch");
        cx.dispatch_action(win, CopyDiffSelection);
        let copied = cx.read_from_clipboard().and_then(|i| i.text());
        assert_eq!(
            copied.as_deref(),
            Some(branch.as_str()),
            "graph Cmd+C (branch) should copy the local branch name"
        );
        eprintln!(
            "[gui-e2e] PASS graph_copy hash={} branch={branch}",
            &full_sha[..8]
        );
    }

    /// Issue #468: the Operation Log row list is variable-height
    /// (`gpui::list` + `ListState`), not `uniform_list`.
    ///
    /// Seeds two entries — row 0 carries a 200-char `error:` — opens the
    /// Operation Log tab, and reads the rows' real laid-out bounds back out of
    /// the panel's `ListState` (`bounds_for_item`, window coordinates).
    /// Expanding row 0 must (a) grow row 0 past its collapsed summary height
    /// and (b) push row 1's y-origin DOWN by that growth, leaving no overlap.
    /// Under the old `uniform_list` neither held: every row was laid out at the
    /// first row's height, so the detail block painted over the row below.
    /// Then drives `OpLogPanel::copy_entry` (the row copy button's handler) and
    /// asserts the clipboard carries the whole entry, 200-char error included —
    /// the part the truncated summary row never shows.
    fn scenario_oplog_expand_copy(cx: &mut VisualTestAppContext) {
        let fixture = build_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        let before_fp = repo_fingerprint(&repo_path);
        let (kagi, _win) = mount(cx, &repo_path);

        let long_error = "e".repeat(200);
        kagi.update(cx, |app, cx| {
            app.bottom_panel_open = true;
            app.bottom_tab = BottomTab::OperationLog;
            // Oldest first — `push` puts the newest at the front, so the
            // long-error entry ends up as row 0.
            e2e::push_failed_op(app, "fetch", "short".to_string(), cx);
            e2e::push_failed_op(app, "checkout", long_error.clone(), cx);
            cx.notify();
        });
        cx.run_until_parked();

        let panel = cx
            .read(|app| kagi.read(app).op_log.clone())
            .expect("op_log entity");
        let row_bounds = |cx: &VisualTestAppContext, ix: usize| {
            cx.read(|app| panel.read(app).scroll_handle().bounds_for_item(ix))
                .unwrap_or_else(|| {
                    panic!("op-log row {ix} was not laid out — is the Operation Log tab open?")
                })
        };
        let collapsed0 = row_bounds(cx, 0);
        let collapsed1 = row_bounds(cx, 1);
        assert!(
            collapsed1.origin.y >= collapsed0.origin.y + collapsed0.size.height,
            "collapsed rows already overlap: {collapsed0:?} / {collapsed1:?}"
        );

        // Expand row 0 (what a click on the row does).
        panel.update(cx, |p, cx| {
            p.toggle_expanded(0);
            cx.notify();
        });
        cx.run_until_parked();

        let expanded0 = row_bounds(cx, 0);
        let expanded1 = row_bounds(cx, 1);
        assert!(
            expanded0.size.height > collapsed0.size.height,
            "expanding row 0 must grow the row itself (was {:?}, now {:?}) — a \
             fixed-height list would keep it at the summary height",
            collapsed0.size.height,
            expanded0.size.height
        );
        assert!(
            expanded1.origin.y > collapsed1.origin.y,
            "expanding row 0 must push row 1 down (row 1 y {:?} -> {:?})",
            collapsed1.origin.y,
            expanded1.origin.y
        );
        assert!(
            expanded1.origin.y >= expanded0.origin.y + expanded0.size.height,
            "expanded row 0 overlaps row 1: {expanded0:?} / {expanded1:?}"
        );

        // The row copy button's handler: the whole entry, not the truncated line.
        let expected = cx.read(|app| {
            let p = panel.read(app);
            oplog_panel::entry_clipboard_text(&p.entries()[0])
        });
        panel.update(cx, |p, cx| p.copy_entry(0, cx));
        let copied = cx
            .read_from_clipboard()
            .and_then(|i| i.text())
            .expect("clipboard text");
        assert_eq!(
            copied, expected,
            "copy_entry should write the formatted entry"
        );
        // Independent of the formatter itself: the clipboard must carry the
        // header AND every detail line, 200-char error included — the tail the
        // truncated summary row never shows.
        assert!(copied.contains("checkout"), "no op name: {copied:?}");
        assert!(
            copied.contains(&format!("  error:   {long_error}")),
            "the copied entry must carry the full 200-char error detail line"
        );
        assert!(
            copied.lines().count() >= 4,
            "expected header + before/dirty/error lines, got {}: {copied:?}",
            copied.lines().count()
        );

        assert_eq!(
            before_fp,
            repo_fingerprint(&repo_path),
            "repo mutated during a read-only scenario"
        );
        eprintln!(
            "[gui-e2e] PASS oplog_expand_copy grew {:?} -> {:?}, row1 {:?} -> {:?}, copied {} chars",
            collapsed0.size.height,
            expanded0.size.height,
            collapsed1.origin.y,
            expanded1.origin.y,
            copied.chars().count()
        );
    }

    /// Issue #335: the `CreateSnapshot` command captures the working tree as a
    /// non-destructive savepoint under `refs/kagi/snapshots/`. Asserts no such
    /// ref exists before, exactly one after, and that HEAD/porcelain are
    /// unchanged (a snapshot only adds a ref — it never moves HEAD).
    fn scenario_create_snapshot(cx: &mut VisualTestAppContext) {
        let fixture = build_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        let before_fp = repo_fingerprint(&repo_path);
        let (_kagi, win) = mount(cx, &repo_path);

        assert!(
            for_each_ref(&repo_path, "refs/kagi/snapshots/")
                .trim()
                .is_empty(),
            "no snapshot ref should exist before CreateSnapshot"
        );

        cx.dispatch_action(win, CreateSnapshot);

        let refs = for_each_ref(&repo_path, "refs/kagi/snapshots/");
        let count = refs.lines().filter(|l| !l.trim().is_empty()).count();
        assert_eq!(
            count, 1,
            "CreateSnapshot should add exactly one refs/kagi/snapshots/ ref, got:\n{refs}"
        );
        assert_eq!(
            before_fp,
            repo_fingerprint(&repo_path),
            "CreateSnapshot must not move HEAD or dirty the working tree"
        );
        eprintln!("[gui-e2e] PASS create_snapshot ref_count={count}");
    }

    /// Issue #373: the command palette's SetTheme action switches the active
    /// theme (`KagiApp::set_theme`, the exact method the palette dispatches).
    /// Asserts the process-wide active-theme slug flips to the requested theme.
    fn scenario_theme_switch(cx: &mut VisualTestAppContext) {
        let fixture = build_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        let (kagi, _win) = mount(cx, &repo_path);

        let before = theme::theme().slug;
        let target = if before == "dracula" {
            "tokyo-night"
        } else {
            "dracula"
        };
        kagi.update(cx, |app, cx| app.set_theme(target, cx));
        cx.run_until_parked();

        let after = theme::theme().slug;
        assert_eq!(
            after, target,
            "SetTheme should make {target} the active theme"
        );
        assert_ne!(after, before, "active theme should have changed");
        eprintln!("[gui-e2e] PASS theme_switch {before} -> {after}");
    }

    /// Issue #337: `CommitRow.provenance` is computed through the real snapshot
    /// pipeline. Asserts the agent commit's row classifies as Claude Code and
    /// the plain human commit's row carries no provenance.
    fn scenario_agent_provenance(cx: &mut VisualTestAppContext) {
        let fixture = build_agent_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        let (kagi, _win) = mount(cx, &repo_path);

        let (head_agent, parent_agent) = cx.read(|app| {
            let rows = &kagi.read(app).active_view.rows;
            (
                rows[0]
                    .provenance
                    .as_ref()
                    .map(|p| p.agent.label().to_string()),
                rows[1]
                    .provenance
                    .as_ref()
                    .map(|p| p.agent.label().to_string()),
            )
        });
        assert_eq!(
            head_agent.as_deref(),
            Some("Claude Code"),
            "agent commit (HEAD) should classify as Claude Code"
        );
        assert_eq!(
            parent_agent, None,
            "plain human commit should carry no provenance"
        );
        eprintln!("[gui-e2e] PASS agent_provenance head=ClaudeCode human=None");
    }

    /// Issue #472: each WIP row draws a dashed connector down to its own
    /// worktree's HEAD, in that worktree's lane colour.
    ///
    /// Asserts, through the real snapshot → `build_tab_view` pipeline: two WIP
    /// rows each get a lane, the lanes differ, every row above the open repo's
    /// HEAD carries exactly one WIP-ghost `Pass` on that lane, the HEAD row
    /// carries the `IntoNode` landing on HEAD's own node, and each connector's
    /// colour index equals its worktree's index (main = 0, the linked worktree
    /// = 1) — which is what makes the two lines tellable apart on screen.
    fn scenario_wip_head_connector(cx: &mut VisualTestAppContext) {
        let (_fixture, repo_path, _wt_path) = build_wip_connector_fixture();
        let before_fp = repo_fingerprint(&repo_path);
        let (kagi, _win) = mount(cx, &repo_path);

        // One WIP-ghost edge of `row`, by kind and colour index.
        fn ghost(
            row: &commit_list::CommitRow,
            kind: EdgeKind,
            color_idx: usize,
        ) -> Vec<&GraphEdge> {
            row.edges
                .iter()
                .filter(|e| {
                    e.kind == kind && graph_wip::wip_color_index(e.color) == Some(color_idx)
                })
                .collect()
        }

        cx.read(|app| {
            let view = &kagi.read(app).active_view;

            // Two dirty working trees → two WIP rows, each with a lane.
            assert_eq!(
                view.wip_lanes.len(),
                2,
                "expected 2 WIP rows (open repo + linked worktree), got {:?}",
                view.wip_lanes
            );
            // #476 slice 2: the map is keyed by target, not by row position.
            let open_lane = graph_wip::wip_lane(&view.wip_lanes, graph_wip::WipTarget::Current)
                .expect("open repo's WIP row got no connector lane");
            let wt_lane = graph_wip::wip_lane(&view.wip_lanes, graph_wip::WipTarget::Worktree(1))
                .expect("linked worktree's WIP row got no lane");
            assert_ne!(open_lane, wt_lane, "two connectors must not share a column");

            // The open repo's HEAD is buried under the `ahead` branch.
            let head = view
                .rows
                .iter()
                .position(|r| r.is_head)
                .expect("no HEAD row");
            assert!(
                head >= 3,
                "fixture should bury HEAD at row >= 3, got row {head}"
            );

            // Every row above HEAD carries the connector; HEAD carries the curve.
            for (i, row) in view.rows[..head].iter().enumerate() {
                let passes = ghost(row, EdgeKind::Pass, 0);
                assert_eq!(
                    passes.len(),
                    1,
                    "row {i} above HEAD must carry exactly one WIP-ghost Pass"
                );
                assert_eq!(passes[0].from_lane, open_lane);
                assert_eq!(passes[0].to_lane, open_lane);
            }
            let into = ghost(&view.rows[head], EdgeKind::IntoNode, 0);
            assert_eq!(
                into.len(),
                1,
                "HEAD's row must carry the connector's IntoNode curve"
            );
            assert_eq!(into[0].from_lane, open_lane);
            assert_eq!(
                into[0].to_lane, view.rows[head].lane,
                "the curve must land on HEAD's own node"
            );

            // The linked worktree's connector reaches ITS head, in colour 1 —
            // the worktree's index in `worktrees`, i.e. its lane colour.
            let wt = &view.worktrees[1];
            assert!(!wt.is_current, "worktrees[1] should be the linked worktree");
            let wt_head_id = wt.head.clone().expect("linked worktree HEAD not read");
            let wt_head = view.commit_row_index[&wt_head_id];
            assert_ne!(
                wt_head, head,
                "the two worktrees should sit on different commits"
            );
            let wt_into = ghost(&view.rows[wt_head], EdgeKind::IntoNode, 1);
            assert_eq!(
                wt_into.len(),
                1,
                "the linked worktree's HEAD row must carry its own IntoNode, in colour index 1"
            );
            assert_eq!(wt_into[0].from_lane, wt_lane);
            assert_eq!(
                ghost(&view.rows[head], EdgeKind::IntoNode, 1).len(),
                0,
                "the worktree's connector must not land on the other worktree's HEAD"
            );

            eprintln!(
                "[gui-e2e] PASS wip_head_connector head_row={head} lanes={open_lane}/{wt_lane} \
                 wt_head_row={wt_head}"
            );
        });

        assert_eq!(
            before_fp,
            repo_fingerprint(&repo_path),
            "repo mutated during a read-only scenario"
        );
    }

    /// Issue #473 + #476 slice 1: clicking a LINKED WORKTREE's WIP row shows
    /// that worktree's changes in the commit panel **in place** — no new tab,
    /// no snapshot — and the four staging ops dispatched there write into
    /// **that worktree**, not the open tab's repository.
    ///
    /// Drives the row's own click handler (`open_commit_panel_for_worktree`)
    /// and asserts: `tabs.len()` unchanged; the panel points at the WORKTREE
    /// and lists ITS dirty file; `do_stage_all` / `do_unstage_all` /
    /// `do_stage_file` / `do_unstage_file` each move the WORKTREE's
    /// `git status --porcelain` while the OPEN repo's fingerprint is unchanged;
    /// the worktree's WIP row counts (`active_view.worktrees[i].wip`) follow;
    /// the still-tab-resolved ops (commit plan, commit, amend, discard-all) are
    /// still refused and leave both trees untouched; and the watcher's in-place
    /// refresh (`refresh_working_tree_external`) does not swap the panel back to
    /// the open repository.
    fn scenario_worktree_wip_inline(cx: &mut VisualTestAppContext) {
        let (_fixture, repo_path, wt_path) = build_wip_connector_fixture();
        let repo_fp = repo_fingerprint(&repo_path);
        let wt_fp = repo_fingerprint(&wt_path);
        let (kagi, _win) = mount(cx, &repo_path);

        let tabs_before = cx.read(|app| kagi.read(app).tabs.len());

        // What the linked worktree's WIP row click does (`render_wip.rs`),
        // minus the message `InputState`s — see `open_worktree_panel_no_inputs`
        // (#476 slice 2 gives a worktree panel real inputs, and the runner
        // cannot construct one without failing the run's leak detector). Both
        // paths go through the same `attach_commit_panel_at`, which is where
        // the panel's `repo_path` canonicalization and `foreign` marking live.
        let wt = wt_path.clone();
        kagi.update(cx, |app, cx| {
            e2e::open_worktree_panel_no_inputs(app, wt, "ahead", 1, cx)
        });
        cx.run_until_parked();

        cx.read(|app| {
            let app = kagi.read(app);
            assert_eq!(
                app.tabs.len(),
                tabs_before,
                "clicking a linked worktree's WIP row must NOT open a tab"
            );
            assert!(app.commit_panel_open, "the commit panel should be open");
        });
        let (panel_repo, files, foreign) = cx.read(|app| {
            let p = kagi.read(app).commit_panel.clone().expect("panel");
            let p = p.read(app);
            (
                p.repo_path.clone(),
                p.state
                    .unstaged
                    .iter()
                    .map(|f| f.path.display().to_string())
                    .collect::<Vec<_>>(),
                p.foreign.clone(),
            )
        });
        assert_eq!(
            panel_repo, wt_path,
            "the panel must point at the worktree, not the open repo"
        );
        assert!(
            files.iter().any(|f| f == "dirty.txt"),
            "the panel should list the WORKTREE's dirty file, got {files:?}"
        );
        assert!(
            foreign.is_some(),
            "a worktree panel must be marked foreign (read-only + header chip)"
        );
        assert!(
            cx.read(|app| kagi.read(app).commit_panel_is_foreign(app)),
            "commit_panel_is_foreign must hold for a worktree panel"
        );

        // ── #476 slice 1: the staging ops write into the WORKTREE ──────────
        // The worktree's WIP row counts, straight off `active_view` — they come
        // from the snapshot, so this is what proves the row followed the write.
        let wip_of = |cx: &mut VisualTestAppContext| {
            cx.read(|app| {
                kagi.read(app)
                    .active_view
                    .worktrees
                    .iter()
                    .find(|w| w.path == wt_path)
                    .unwrap_or_else(|| panic!("no worktree row for {}", wt_path.display()))
                    .wip
                    .expect("the dirty worktree must carry a WIP row")
            })
        };
        let wip_before = wip_of(cx);
        assert_eq!(
            (wip_before.staged, wip_before.untracked),
            (0, 1),
            "fixture: the worktree starts with one untracked file"
        );

        kagi.update(cx, |app, cx| app.do_stage_all(cx));
        cx.run_until_parked();
        assert_eq!(
            repo_fp,
            repo_fingerprint(&repo_path),
            "the OPEN repo was mutated by a stage dispatched at a worktree panel"
        );
        assert_eq!(
            repo_fingerprint(&wt_path).1,
            "A  dirty.txt\n",
            "stage-all from a worktree panel must stage in the WORKTREE"
        );
        let wip_staged = wip_of(cx);
        assert_eq!(
            (wip_staged.staged, wip_staged.unstaged, wip_staged.untracked),
            (1, 0, 0),
            "the worktree's WIP row counts must follow the stage"
        );

        kagi.update(cx, |app, cx| app.do_unstage_all(cx));
        cx.run_until_parked();
        assert_eq!(
            repo_fingerprint(&wt_path),
            wt_fp,
            "unstage-all from a worktree panel must unstage in the WORKTREE"
        );
        assert_eq!(
            repo_fp,
            repo_fingerprint(&repo_path),
            "the OPEN repo was mutated by an unstage dispatched at a worktree panel"
        );
        assert_eq!(
            wip_of(cx).untracked,
            1,
            "the WIP row must follow back to untracked"
        );

        // Per-file, by the same panel index the row's button carries.
        kagi.update(cx, |app, cx| app.do_stage_file(0, cx));
        cx.run_until_parked();
        assert_eq!(
            repo_fingerprint(&wt_path).1,
            "A  dirty.txt\n",
            "stage(0) from a worktree panel must stage in the WORKTREE"
        );
        kagi.update(cx, |app, cx| app.do_unstage_file(0, cx));
        cx.run_until_parked();
        assert_eq!(
            repo_fingerprint(&wt_path),
            wt_fp,
            "unstage(0) from a worktree panel must unstage in the WORKTREE"
        );
        assert_eq!(
            repo_fp,
            repo_fingerprint(&repo_path),
            "the OPEN repo was mutated by a per-file stage/unstage at a worktree panel"
        );

        // ── Planning amend / discard-all writes nothing ──────────────────────
        // #476 slice 3 converted both (execution lives in
        // `scenario_worktree_panel_amend_discard`). Opening their confirms is
        // still a pure read — `plan → confirm → …` means nothing has happened
        // until the second click, in either repository.
        kagi.update(cx, |app, cx| {
            app.commit_panel_amend(cx);
            app.open_discard_all_modal(cx);
        });
        cx.run_until_parked();
        assert_eq!(
            repo_fp,
            repo_fingerprint(&repo_path),
            "the OPEN repo was mutated by merely PLANNING a write at a worktree panel"
        );
        assert_eq!(
            wt_fp,
            repo_fingerprint(&wt_path),
            "the WORKTREE was mutated by merely PLANNING a write at a worktree panel"
        );

        // The watcher's in-place refresh must not swap the panel back.
        kagi.update(cx, |app, cx| app.refresh_working_tree_external(cx));
        cx.run_until_parked();
        let after_reload = cx.read(|app| {
            let p = kagi.read(app).commit_panel.clone().expect("panel survived");
            p.read(app).repo_path.clone()
        });
        assert_eq!(
            after_reload, wt_path,
            "a watcher refresh replaced the worktree panel with the open repo's"
        );
        assert_eq!(
            cx.read(|app| kagi.read(app).tabs.len()),
            tabs_before,
            "no tab may appear at any point in this scenario"
        );

        // POSITIVE CONTROL: the guard must not misfire on the tab's OWN panel.
        // Everything above proves writes are refused; this proves they are
        // refused *because the panel is foreign*, not because staging is broken
        // in the harness — and pins `is_foreign_panel`'s path-equality down.
        //
        // The panel is re-pointed at the tab's repo rather than opened through
        // `open_commit_panel`: that path builds the two `InputState`s, and
        // gpui_component's `InputState` retains handles that gpui's end-of-run
        // leak detector then reports (nothing in this suite drops them). What
        // is under test here is the guard + the staging path, and both run
        // identically either way.
        let panel = cx.read(|app| kagi.read(app).commit_panel.clone().expect("panel"));
        let own = repo_path.clone();
        panel.update(cx, |v, _| {
            v.repo_path = own.clone();
            v.foreign = None;
            v.state.reload_status(&own);
        });
        cx.run_until_parked();
        assert!(
            !cx.read(|app| kagi.read(app).commit_panel_is_foreign(app)),
            "the open repo's own panel must NOT be treated as foreign"
        );
        kagi.update(cx, |app, cx| app.do_stage_all(cx));
        cx.run_until_parked();
        let staged_fp = repo_fingerprint(&repo_path);
        assert_ne!(
            repo_fp, staged_fp,
            "staging from the OPEN repo's own panel must actually stage"
        );
        assert_eq!(
            staged_fp.1, "A  dirty.txt\n",
            "expected dirty.txt staged in the open repo, got {:?}",
            staged_fp.1
        );
        assert_eq!(
            wt_fp,
            repo_fingerprint(&wt_path),
            "staging in the open repo must not touch the worktree"
        );

        eprintln!(
            "[gui-e2e] PASS worktree_wip_inline tabs={tabs_before} panel={} files={files:?} \
             own-panel-staged={:?}",
            panel_repo.display(),
            staged_fp.1.trim()
        );
    }

    /// Issue #476 slice 2: a commit dispatched from a LINKED WORKTREE's panel
    /// lands in **that worktree**.
    ///
    /// Drives the real write path — `do_stage_all`, then
    /// `open_commit_plan_modal`, which plans and (no blockers) runs
    /// `start_commit` itself, the "smooth commit" the Commit button takes — and
    /// asserts: the worktree's HEAD advanced by exactly one commit and its
    /// working tree is clean; the OPEN repository's HEAD/porcelain and the
    /// second worktree are untouched; the newest oplog entry (in-memory panel
    /// AND the persisted log `Backend::run` writes) is `commit` against the
    /// WORKTREE's path; the open tab's graph gained the new commit (shared refs
    /// and the post-commit re-snapshot); the committed worktree's WIP row is gone
    /// while the surviving rows keep their own lanes (the `WipTarget` keying);
    /// and the tab's Undo does not point at the worktree's new commit.
    fn scenario_worktree_panel_commit(cx: &mut VisualTestAppContext) {
        let (_fixture, repo_path, wt_a, wt_b) = build_two_worktree_fixture();
        let repo_fp = repo_fingerprint(&repo_path);
        let wt_b_fp = repo_fingerprint(&wt_b);
        let wt_a_head_before = repo_fingerprint(&wt_a).0;
        let wt_a_commits_before = commit_count(&wt_a);
        let (kagi, _win) = mount(cx, &repo_path);

        // Where each worktree sits in the snapshot's list — its `WipTarget` key
        // and its lane colour index are both that position.
        let wt_index = |cx: &mut VisualTestAppContext, path: &Path| -> usize {
            cx.read(|app| {
                kagi.read(app)
                    .active_view
                    .worktrees
                    .iter()
                    .position(|w| w.path == path)
                    .unwrap_or_else(|| panic!("no worktree row for {}", path.display()))
            })
        };
        let idx_a = wt_index(cx, &wt_a);
        let idx_b = wt_index(cx, &wt_b);
        let (lanes_before, lane_open, lane_a, lane_b) = cx.read(|app| {
            let lanes = kagi.read(app).active_view.wip_lanes.clone();
            let get = |t| graph_wip::wip_lane(&lanes, t);
            (
                lanes.clone(),
                get(graph_wip::WipTarget::Current),
                get(graph_wip::WipTarget::Worktree(idx_a)),
                get(graph_wip::WipTarget::Worktree(idx_b)),
            )
        });
        assert_eq!(
            lanes_before.len(),
            3,
            "fixture: three dirty working trees → three WIP rows, got {lanes_before:?}"
        );
        assert!(
            lane_open.is_some() && lane_a.is_some() && lane_b.is_some(),
            "every WIP row should have drawn a connector: {lanes_before:?}"
        );
        assert_ne!(lane_a, lane_b, "two connectors must not share a column");

        // Open worktree A's panel, stage its file, give it a message.
        let path_a = wt_a.clone();
        kagi.update(cx, |app, cx| {
            e2e::open_worktree_panel_no_inputs(app, path_a, "wt-a", idx_a, cx)
        });
        cx.run_until_parked();
        assert!(
            cx.read(|app| kagi.read(app).commit_panel_is_foreign(app)),
            "the panel must be marked foreign for the write ops to resolve it"
        );
        kagi.update(cx, |app, cx| {
            app.do_stage_all(cx);
            e2e::set_commit_message(app, "wt-a: from the worktree panel", cx);
        });
        cx.run_until_parked();
        assert_eq!(
            repo_fingerprint(&wt_a).1,
            "A  dirty.txt\n",
            "stage-all should have staged in the WORKTREE"
        );

        // The Commit button's path: plan, then (no blockers) commit.
        kagi.update(cx, |app, cx| app.open_commit_plan_modal(cx));
        cx.run_until_parked();

        // ── The worktree moved; nothing else did ────────────────────────────
        let (wt_a_head_after, wt_a_status) = repo_fingerprint(&wt_a);
        assert_ne!(
            wt_a_head_after, wt_a_head_before,
            "the WORKTREE's HEAD must have advanced"
        );
        assert_eq!(
            commit_count(&wt_a),
            wt_a_commits_before + 1,
            "exactly one new commit in the worktree"
        );
        assert_eq!(
            wt_a_status, "",
            "the worktree should be clean after committing its only change"
        );
        assert_eq!(
            repo_fp,
            repo_fingerprint(&repo_path),
            "the OPEN repo was mutated by a commit dispatched at a worktree panel"
        );
        assert_eq!(
            wt_b_fp,
            repo_fingerprint(&wt_b),
            "the OTHER worktree was mutated by a commit at worktree A's panel"
        );

        // ── The oplog is attributed to the worktree ─────────────────────────
        let (op, repo) = cx.read(|app| {
            let panel = kagi.read(app).op_log.clone().expect("op_log entity");
            let panel = panel.read(app);
            e2e::entry_op_and_repo(panel.entries().front().expect("an op-log entry"))
        });
        assert_eq!(op, "commit", "newest op-log entry should be the commit");
        assert_eq!(
            PathBuf::from(&repo),
            wt_a,
            "the op-log entry's repo must be the WORKTREE's path, not the tab's"
        );
        let (pop, prepo) = e2e::latest_persisted_op().expect("a persisted oplog entry");
        assert_eq!(pop, "commit");
        assert_eq!(
            PathBuf::from(&prepo),
            wt_a,
            "the persisted oplog entry's repo must be the WORKTREE's path"
        );

        // ── The open tab's graph gained the commit (shared refs) ────────────
        let new_sha = wt_a_head_after.clone();
        assert!(
            cx.read(|app| kagi
                .read(app)
                .active_view
                .rows
                .iter()
                .any(|r| r.id.0 == new_sha)),
            "the open tab must re-snapshot and show the worktree's new commit"
        );

        // ── The WIP rows: A's is gone, the others keep their own lanes ──────
        let wip_a = cx.read(|app| {
            kagi.read(app)
                .active_view
                .worktrees
                .iter()
                .find(|w| w.path == wt_a)
                .and_then(|w| w.wip)
        });
        assert!(
            wip_a.is_none_or(|w| !w.is_dirty()),
            "worktree A is clean after the commit — its WIP row must be gone, got {wip_a:?}"
        );
        // The frame between the commit and the re-snapshot renders the NEW row
        // list against the OLD lane map — `refresh_worktree_wip_row` drops A's
        // row in place. Keyed by target, the survivors keep their own lanes;
        // by position, the open repo's row would hand its lane to A's former
        // neighbour and worktree B would inherit A's.
        let rows_now = [
            graph_wip::WipTarget::Current,
            graph_wip::WipTarget::Worktree(idx_b),
        ];
        assert_eq!(
            graph_wip::lanes_for_rows(&lanes_before, &rows_now),
            vec![lane_open, lane_b],
            "a committed worktree's row leaving the list must not shift the \
             lanes of the rows that remain (lanes were {lanes_before:?})"
        );
        // …and the freshly-built map agrees, with A's entry simply absent.
        cx.read(|app| {
            let lanes = &kagi.read(app).active_view.wip_lanes;
            assert_eq!(
                graph_wip::wip_lane(lanes, graph_wip::WipTarget::Worktree(idx_a)),
                None,
                "the clean worktree must have no connector: {lanes:?}"
            );
            assert_eq!(
                graph_wip::wip_lane(lanes, graph_wip::WipTarget::Current),
                lane_open,
                "the open repo's WIP row must keep its lane: {lanes:?}"
            );
        });

        // ── The commit recorded NO undo entry for the tab ───────────────────
        // #476 slice 3: `operation_history` is per tab, and `head_branch_and_sha`
        // reads the TAB's HEAD — so a recorded worktree commit would sit here as
        // the tab's branch pointed at the worktree's commit, and Cmd+Z would
        // move `main` onto it. (The stack is not compared wholesale: `reload`
        // legitimately seeds it from the tab's own reflog, ADR-0084.)
        let undo_head = cx.read(|app| e2e::undo_head(kagi.read(app)));
        assert!(
            undo_head
                .as_ref()
                .is_none_or(|(_, after, _)| after != &wt_a_head_after),
            "the tab's Undo must not point at the WORKTREE's commit: {undo_head:?}"
        );

        eprintln!(
            "[gui-e2e] PASS worktree_panel_commit wt-a {}..{} lanes open={lane_open:?} \
             b={lane_b:?} oplog={op}@{repo}",
            &wt_a_head_before[..8],
            &wt_a_head_after[..8],
        );
    }

    /// Issue #476 slice 3: **amend** and **discard** dispatched from a LINKED
    /// WORKTREE's panel act on that worktree.
    ///
    /// Both are the destructive pair the earlier slices kept refused, and both
    /// run their real two-stage confirm here: `commit_panel_amend` →
    /// `start_amend` (arm) → `start_amend` (fire), and `open_discard_all_modal`
    /// → `start_discard` (arm) → `start_discard` (fire).
    ///
    /// Asserts, in order: the amend rewrote worktree A's HEAD **in place** (new
    /// SHA, same parent, same commit count, clean tree) while the open
    /// repository and worktree B are untouched and the persisted oplog names A;
    /// the open tab re-snapshotted onto the rewritten commit; the discard
    /// emptied A's working tree with a backup blob that resolves in the shared
    /// ODB; and neither op pushed anything onto the TAB's undo stack.
    fn scenario_worktree_panel_amend_discard(cx: &mut VisualTestAppContext) {
        let (_fixture, repo_path, wt_a, wt_b) = build_two_worktree_fixture();
        let repo_fp = repo_fingerprint(&repo_path);
        let wt_b_fp = repo_fingerprint(&wt_b);
        let head_before = repo_fingerprint(&wt_a).0;
        let parent_before = rev_parse(&wt_a, "HEAD^");
        let commits_before = commit_count(&wt_a);
        let (kagi, win) = mount(cx, &repo_path);

        let idx_a = cx.read(|app| {
            kagi.read(app)
                .active_view
                .worktrees
                .iter()
                .position(|w| w.path == wt_a)
                .unwrap_or_else(|| panic!("no worktree row for {}", wt_a.display()))
        });
        // ── (a) amend ───────────────────────────────────────────────────────
        // Stage A's file, then amend: staged content, no new message
        // (`AmendMode::Staged`), so A's HEAD is replaced rather than extended.
        let path_a = wt_a.clone();
        kagi.update(cx, |app, cx| {
            e2e::open_worktree_panel_no_inputs(app, path_a, "wt-a", idx_a, cx)
        });
        cx.run_until_parked();
        assert!(
            cx.read(|app| kagi.read(app).commit_panel_is_foreign(app)),
            "the panel must be marked foreign for the write ops to resolve it"
        );
        kagi.update(cx, |app, cx| app.do_stage_all(cx));
        cx.run_until_parked();

        kagi.update(cx, |app, cx| app.commit_panel_amend(cx));
        cx.run_until_parked();
        assert_eq!(
            head_before,
            repo_fingerprint(&wt_a).0,
            "planning the amend must not have rewritten anything yet"
        );
        // Two-stage confirm: the first click only arms.
        kagi.update(cx, |app, cx| app.start_amend(cx));
        assert_eq!(
            head_before,
            repo_fingerprint(&wt_a).0,
            "the FIRST amend confirm must only arm — a history rewrite needs two"
        );
        kagi.update(cx, |app, cx| app.start_amend(cx));
        cx.run_until_parked();

        let (head_after_amend, status_after_amend) = repo_fingerprint(&wt_a);
        assert_ne!(
            head_after_amend, head_before,
            "the WORKTREE's HEAD must have been rewritten"
        );
        assert_eq!(
            rev_parse(&wt_a, "HEAD^"),
            parent_before,
            "an amend replaces the tip in place — the parent must not move"
        );
        assert_eq!(
            commit_count(&wt_a),
            commits_before,
            "an amend must not add a commit"
        );
        assert_eq!(
            status_after_amend, "",
            "the staged change went INTO the amended commit, so A is clean"
        );
        assert_eq!(
            repo_fp,
            repo_fingerprint(&repo_path),
            "the OPEN repo was mutated by an amend dispatched at a worktree panel"
        );
        assert_eq!(
            wt_b_fp,
            repo_fingerprint(&wt_b),
            "the OTHER worktree was mutated by an amend at worktree A's panel"
        );
        let (pop, prepo) = e2e::latest_persisted_op().expect("a persisted oplog entry");
        assert_eq!(
            pop, "amend",
            "newest persisted oplog entry should be the amend"
        );
        assert_eq!(
            PathBuf::from(&prepo),
            wt_a,
            "the persisted oplog entry's repo must be the WORKTREE's path, not the tab's"
        );
        // Shared refs + the post-amend re-snapshot: the open tab shows the new tip.
        assert!(
            cx.read(|app| kagi
                .read(app)
                .active_view
                .rows
                .iter()
                .any(|r| r.id.0 == head_after_amend)),
            "the open tab must re-snapshot and show the worktree's amended commit"
        );

        // ── (b) discard ─────────────────────────────────────────────────────
        // `reload` dropped the (now clean) panel, so dirty A again and re-open it.
        std::fs::write(wt_a.join("f.txt"), "dirtied for discard\n").unwrap();
        let path_a = wt_a.clone();
        kagi.update(cx, |app, cx| {
            e2e::open_worktree_panel_no_inputs(app, path_a, "wt-a", idx_a, cx)
        });
        cx.run_until_parked();
        assert_eq!(
            repo_fingerprint(&wt_a).1,
            " M f.txt\n",
            "fixture: worktree A must have exactly one unstaged modification to discard"
        );

        kagi.update(cx, |app, cx| app.open_discard_all_modal(cx));
        cx.run_until_parked();
        kagi.update(cx, |app, cx| app.start_discard(cx));
        assert_eq!(
            repo_fingerprint(&wt_a).1,
            " M f.txt\n",
            "the FIRST discard confirm must only arm — the destructive op needs two"
        );
        kagi.update(cx, |app, cx| app.start_discard(cx));
        cx.run_until_parked();

        assert_eq!(
            repo_fingerprint(&wt_a),
            (head_after_amend.clone(), String::new()),
            "discard-all from a worktree panel must clean the WORKTREE without moving its HEAD"
        );
        assert_eq!(
            repo_fp,
            repo_fingerprint(&repo_path),
            "the OPEN repo was mutated by a discard dispatched at a worktree panel"
        );
        assert_eq!(
            wt_b_fp,
            repo_fingerprint(&wt_b),
            "the OTHER worktree was mutated by a discard at worktree A's panel"
        );
        let (pop, prepo) = e2e::latest_persisted_op().expect("a persisted oplog entry");
        assert_eq!(
            pop, "discard",
            "newest persisted oplog entry should be the discard"
        );
        assert_eq!(
            PathBuf::from(&prepo),
            wt_a,
            "the persisted oplog entry's repo must be the WORKTREE's path, not the tab's"
        );
        // The recovery handle (ADR-0083): the pre-discard content lives in the
        // ODB as a blob, and the oplog entry carries its SHA. The worktree
        // shares the open repository's ODB, so the blob resolves from BOTH.
        //
        // Read from the IN-MEMORY entry deliberately: the persisted entry's
        // after-state is `plan.predicted` ("N file(s) discarded"), because
        // ADR-0149 moved the persisted write into `Backend::run`, which only
        // sees the plan. The blob list reaches `record_op` (this entry) and the
        // `[kagi] executed: discarded …; backup: <path>=<sha>` line. Unchanged
        // by #476 — the slice only redirects which repository is named.
        let (op, repo, dirty) = cx.read(|app| {
            let panel = kagi.read(app).op_log.clone().expect("op_log entity");
            let panel = panel.read(app);
            let entry = panel.entries().front().expect("an op-log entry");
            let (op, repo) = e2e::entry_op_and_repo(entry);
            (op, repo, e2e::entry_after_dirty(entry))
        });
        assert_eq!(op, "discard", "newest op-log entry should be the discard");
        assert_eq!(
            PathBuf::from(&repo),
            wt_a,
            "the op-log entry's repo must be the WORKTREE's path"
        );
        let dirty = dirty.expect("a successful discard records an after-state");
        let blob = dirty
            .rsplit_once('=')
            .map(|(_, sha)| sha.trim().to_string())
            .unwrap_or_else(|| panic!("no `path=blob` backup in the oplog entry: {dirty:?}"));
        assert_eq!(blob.len(), 40, "expected a 40-hex blob SHA, got {blob:?}");
        for odb in [&wt_a, &repo_path] {
            assert!(
                git_ok(odb, &["cat-file", "-e", &blob]),
                "the discard's backup blob {blob} must resolve in {}",
                odb.display()
            );
        }

        // ── (c) neither op touched the TAB's undo stack ─────────────────────
        // Both ran in another repository, so `undo_skipped_for_foreign` kept
        // them off `operation_history` (and said so on stderr:
        // `[kagi] undo: skipped — amend ran in another worktree …`).
        //
        // The oracle is the SHA, not stack equality: `reload` legitimately
        // seeds the stack from the TAB's own branch reflog whenever it is empty
        // (ADR-0084), and `head_branch_and_sha` reads the tab's HEAD — so a
        // leaked entry would appear as the tab's branch pointed at worktree A's
        // rewritten commit, and Cmd+Z would move `main` onto it.
        let undo_head = cx.read(|app| e2e::undo_head(kagi.read(app)));
        assert!(
            undo_head
                .as_ref()
                .is_none_or(|(_, after, _)| after != &head_after_amend),
            "the tab's Undo must not point at the WORKTREE's amended commit: {undo_head:?}"
        );
        // …and running it changes neither repository: the entry it holds (if
        // any) is the tab's own, and its plan is evaluated against the tab.
        let before_undo = (repo_fingerprint(&repo_path), repo_fingerprint(&wt_a));
        kagi.update(cx, |app, cx| {
            app.open_history_undo_modal();
            app.confirm_history(cx);
        });
        cx.run_until_parked();
        assert_eq!(
            (repo_fingerprint(&repo_path), repo_fingerprint(&wt_a)),
            before_undo,
            "the tab's Undo must not act on either repository after worktree ops"
        );

        // ── (d) the Editor Workspace tree discards in the TAB ───────────────
        // Slice 3 review: `open_discard_modal_for_path` is shared by the commit
        // panel's file menu and the editor tree, which mean DIFFERENT
        // repositories. With the same relative path dirty in both, resolving
        // the panel would destroy the worktree's copy on an editor-tree click.
        // `WriteOrigin` is what keeps them apart, and it rides on the modal so
        // preflight + execute land where the plan was built.
        std::fs::write(repo_path.join("f.txt"), "open repo edit\n").unwrap();
        std::fs::write(wt_a.join("f.txt"), "worktree edit\n").unwrap();
        let path_a = wt_a.clone();
        kagi.update(cx, |app, cx| {
            e2e::open_worktree_panel_no_inputs(app, path_a, "wt-a", idx_a, cx)
        });
        cx.run_until_parked();
        assert!(
            cx.read(|app| kagi.read(app).commit_panel_is_foreign(app)),
            "the worktree panel must be up — that is the trap the origin avoids"
        );

        // The real handler behind the tree's "Discard Changes…" menu item.
        let discard_f = EditorTreeAction::Discard(PathBuf::from("f.txt"));
        win.update(cx, |_, window, app_cx| {
            kagi.update(app_cx, |app, cx| {
                app.dispatch_editor_tree_action(discard_f, window, cx)
            })
        })
        .expect("dispatch editor-tree discard");
        cx.run_until_parked();
        kagi.update(cx, |app, cx| app.start_discard(cx)); // arms
        kagi.update(cx, |app, cx| app.start_discard(cx)); // fires
        cx.run_until_parked();

        assert_eq!(
            repo_fingerprint(&repo_path),
            repo_fp,
            "an editor-tree discard must restore the OPEN repo's f.txt (back to the \
             fixture state: only the untracked dirty.txt left)"
        );
        assert_eq!(
            repo_fingerprint(&wt_a).1,
            " M f.txt\n",
            "an editor-tree discard must NOT touch the worktree's copy of the same path"
        );

        // …and the panel's own discard still goes to the worktree.
        let path_a = wt_a.clone();
        kagi.update(cx, |app, cx| {
            e2e::open_worktree_panel_no_inputs(app, path_a, "wt-a", idx_a, cx)
        });
        cx.run_until_parked();
        kagi.update(cx, |app, cx| app.open_discard_all_modal(cx));
        cx.run_until_parked();
        kagi.update(cx, |app, cx| app.start_discard(cx)); // arms
        kagi.update(cx, |app, cx| app.start_discard(cx)); // fires
        cx.run_until_parked();
        assert_eq!(
            repo_fingerprint(&wt_a),
            (head_after_amend.clone(), String::new()),
            "the panel's own discard must clean the WORKTREE"
        );
        assert_eq!(
            repo_fingerprint(&repo_path),
            repo_fp,
            "the panel's discard must not touch the open repo"
        );

        eprintln!(
            "[gui-e2e] PASS worktree_panel_amend_discard wt-a {}..{} backup={} undo={undo_head:?}",
            &head_before[..8],
            &head_after_amend[..8],
            &blob[..8],
        );
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
