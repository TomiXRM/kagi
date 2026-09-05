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
//! Each scenario mounts the real root, drives it (keystroke / registered action
//! / entity update), settles deterministically, and asserts observable state —
//! `KagiApp` fields, the clipboard, or repo refs (ADR-0166 §3: the assertions
//! are the oracle, screenshots are triage only). Coverage: bottom-panel toggle
//! (PoC), graph Cmd+C copy (ADR-0170), Create Snapshot (#335), command-palette
//! theme switch (#373), agent provenance (#337), WIP→HEAD connectors (#472).
//! Exits 0 on success, non-zero (panic → 101) on failure.
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

    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui::{px, size, AnyWindowHandle, Entity, VisualTestAppContext};
    use kagi::graph::{EdgeKind, GraphEdge};
    use kagi::ui::{
        commands::CreateSnapshot, commit_list, e2e, graph_wip, oplog_panel, settings::CopyTarget,
        theme, BottomTab, CopyDiffSelection, KagiApp, ToggleBottomPanel,
    };

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
    fn mount(
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
        cx.update(e2e::init_app);

        scenario_bottom_panel(&mut cx);
        scenario_graph_copy(&mut cx, log_dir.path());
        scenario_oplog_expand_copy(&mut cx);
        scenario_create_snapshot(&mut cx);
        scenario_theme_switch(&mut cx);
        scenario_agent_provenance(&mut cx);
        scenario_wip_head_connector(&mut cx);

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
            let open_lane = view.wip_lanes[0].expect("open repo's WIP row got no connector lane");
            let wt_lane = view.wip_lanes[1].expect("linked worktree's WIP row got no lane");
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
