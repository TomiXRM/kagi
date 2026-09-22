//! Issue #462 — a confirmation card must fit the window it is drawn in.
//!
//! The report: on a 600–700px-tall window the plan/confirm cards ran past the
//! bottom edge. The action row went with them, so the user could see neither
//! what the operation would touch nor the button that refuses it. `modal_shell`
//! answers with compact headings, spacing and bounded target lists below a
//! single height threshold, folding only supporting detail — never targets
//! or warnings. Cards keep an outer gutter and a pinned action row.
//!
//! This scenario is the oracle for that contract, and it asserts on laid-out
//! bounds from the real renderer: the card, its pinned footer, the footer's
//! buttons, the target panel and the individual target rows, all measured
//! through the zero-layout `e2e` canvas probes. Real `KagiApp`, real
//! `kagi_git` plans against an isolated fixture (a bare local "remote", never
//! the network), and nothing destructive is ever executed — the two-stage
//! confirm is armed and then cancelled, and the fixture's fingerprint is
//! re-checked at the end.
//!
//! One window per (viewport, locale): `MacWindow::resize` schedules work on the
//! native main queue that `VisualTestAppContext` never drains, so a resize is
//! not observable here — the same reason `layout.rs` mounts per viewport.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{point, px, size, AnyWindowHandle, Bounds, Entity, Pixels, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n, theme};

use crate::macos::{git, open_offscreen, repo_fingerprint, unmount};

/// GPUI snaps layout edges to device pixels.
const EPS: f32 = 1.;
/// Staged files an amend folds in, and tracked files a discard-all targets.
const TARGETS: usize = 120;
/// Untracked files: discard deletes them after an ODB backup, which is what
/// makes the discard plan produce its (safety-relevant) warning.
const UNTRACKED: usize = 6;
/// Commits the push preview lists.
const AHEAD: usize = 27;
/// `modal_shell`'s list floor: a target list never shrinks below three rows,
/// because a list squeezed to nothing hides what the operation acts on.
const LIST_FLOOR: usize = 3;

/// 600/700 are reported short windows; 750 guards the measured breakpoint gap.
/// 900 retains the normal layout.
const SIZES: [(f32, f32, bool); 4] = [
    (1200., 600., true),
    (1200., 700., true),
    (1200., 750., true),
    (1200., 900., false),
];

/// Everything the assertions need about the window under test.
struct Case {
    win: AnyWindowHandle,
    viewport: Bounds<Pixels>,
    /// Below the compact threshold.
    compact: bool,
    label: String,
}

impl Case {
    /// The list floor at rest: the reported 600px window may sit on
    /// `modal_shell`'s floor, but a 700px window has room for more than the
    /// bare minimum — that is the difference between "fits" and "usable".
    fn min_rows(&self) -> usize {
        if self.viewport.size.height <= px(600.) {
            LIST_FLOOR
        } else {
            LIST_FLOOR + 1
        }
    }

    /// A zoom that moves this window across the compact threshold, which is
    /// stated in zoom-normalised logical pixels: the short windows at 0.7x
    /// normalise above 850px; 900px at 1.5x normalises to 600px.
    fn flip_zoom(&self) -> f32 {
        if self.compact {
            0.7
        } else {
            1.5
        }
    }
}

// ── Fixture ──────────────────────────────────────────────────────────────

fn target_path(ix: usize) -> String {
    format!("crates/kagi-git/src/ops/module_{ix:03}/handler_{ix:03}.rs")
}

fn staged_path(ix: usize) -> String {
    format!("crates/kagi-ui-core/src/incoming_{ix:03}/patch_{ix:03}.rs")
}

/// Untracked, and therefore sorted after the tracked targets in the card's
/// list — the actual end of the discard list.
fn scratch_path(ix: usize) -> String {
    format!("scratch/note_{ix}.txt")
}

fn write_file(repo: &Path, rel: &str, body: &str) {
    let file = repo.join(rel);
    std::fs::create_dir_all(file.parent().expect("fixture path has a parent")).unwrap();
    std::fs::write(file, body).unwrap();
}

/// A repo that produces all three cards at once, each with enough content to
/// overflow a short window:
///
/// * 120 staged additions (+ one secret-named file, so the real checklist
///   produces an amend warning) → the amend card's folded-files list;
/// * 120 modified tracked files + 6 untracked ones → the discard-all card's
///   target list and its "untracked will be deleted" warning;
/// * 27 commits ahead of a **local bare** `origin` → the push card's preview.
///
/// HEAD is ahead of the upstream, so the amend plan is not blocked as "already
/// pushed"; the deep paths are the ones that made the card wrap in the report.
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("tempdir");
    let repo = root.path().join("repo");
    let bare = root.path().join("origin.git");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    for ix in 0..TARGETS {
        write_file(
            &repo,
            &target_path(ix),
            &format!("fn handler_{ix:03}() {{}}\n"),
        );
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "initial"]);

    let bare_str = bare.to_str().expect("utf-8 tempdir");
    git(&repo, &["init", "--bare", "-q", bare_str]);
    git(&repo, &["remote", "add", "origin", bare_str]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    for ix in 0..AHEAD {
        git(
            &repo,
            &[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("ahead {ix}"),
            ],
        );
    }

    for ix in 0..TARGETS {
        write_file(
            &repo,
            &target_path(ix),
            &format!("fn handler_{ix:03}() {{ todo!() }}\n"),
        );
        write_file(
            &repo,
            &staged_path(ix),
            &format!("pub fn patch_{ix:03}() {{}}\n"),
        );
    }
    write_file(&repo, ".env.local", "TOKEN=fixture\n");
    git(&repo, &["add", "crates/kagi-ui-core", ".env.local"]);
    for ix in 0..UNTRACKED {
        write_file(&repo, &scratch_path(ix), "scratch\n");
    }
    let canonical = repo.canonicalize().expect("canonical fixture path");
    (root, canonical)
}

/// Process-wide UI settings this scenario changes, restored on drop so the
/// rest of the suite is unaffected however an assertion exits.
struct Restore {
    zoom: f32,
    language_env: Option<std::ffi::OsString>,
    language: i18n::Lang,
}

impl Restore {
    fn capture() -> Self {
        Self {
            zoom: theme::zoom(),
            language_env: std::env::var_os("KAGI_LANG"),
            language: i18n::lang(),
        }
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        theme::set_zoom(self.zoom);
        std::env::set_var("KAGI_LANG", self.language.slug());
        i18n::init_lang();
        match self.language_env.take() {
            Some(value) => std::env::set_var("KAGI_LANG", value),
            None => std::env::remove_var("KAGI_LANG"),
        }
    }
}

// ── Measurement ──────────────────────────────────────────────────────────

fn mount_at(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    dimensions: (f32, f32),
) -> (Entity<KagiApp>, AnyWindowHandle) {
    let state = e2e::app_state(repo).expect("fixture app state");
    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
    let out = captured.clone();
    let win = open_offscreen(
        cx,
        size(px(dimensions.0), px(dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &out),
    );
    let app = captured.borrow().clone().expect("captured KagiApp");
    cx.run_until_parked();
    (app, win.into())
}

/// Forget the named bounds, draw one real frame, and report what that frame
/// laid out. A name with no entry afterwards was not rendered at all.
fn measure(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    names: &[&str],
) -> Vec<Option<Bounds<Pixels>>> {
    let id = win.window_id();
    for name in names {
        e2e::clear_control_bounds(id, name);
    }
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw the modal frame");
    names
        .iter()
        .map(|name| e2e::control_bounds(id, name))
        .collect()
}

fn one(cx: &mut VisualTestAppContext, win: AnyWindowHandle, name: &str) -> Option<Bounds<Pixels>> {
    measure(cx, win, &[name])[0]
}

fn required(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    name: &str,
    label: &str,
) -> Bounds<Pixels> {
    let bounds = one(cx, win, name).unwrap_or_else(|| panic!("{label}: {name} was not laid out"));
    solid(bounds, &format!("{label}: {name}"));
    bounds
}

fn assert_body_content(cx: &mut VisualTestAppContext, case: &Case, name: &str) {
    let frame = measure(cx, case.win, &["modal-card", "modal-body", name]);
    let card = frame[0].expect("modal card");
    let body = frame[1].expect("modal body");
    let content = frame[2].unwrap_or_else(|| panic!("{}: missing {name}", case.label));
    solid(content, &format!("{}: {name}", case.label));
    contained(card, body, &format!("{}: modal body", case.label));
    contained(body, content, &format!("{}: {name}", case.label));
}

/// Finite, non-degenerate bounds: a zero-height card or button is not visible,
/// however well-contained it is.
fn solid(bounds: Bounds<Pixels>, label: &str) {
    for value in [
        bounds.origin.x,
        bounds.origin.y,
        bounds.size.width,
        bounds.size.height,
    ] {
        assert!(
            f32::from(value).is_finite(),
            "{label}: non-finite {bounds:?}"
        );
    }
    assert!(
        bounds.size.width > px(0.) && bounds.size.height > px(0.),
        "{label}: collapsed to {bounds:?}"
    );
}

fn contained(parent: Bounds<Pixels>, child: Bounds<Pixels>, label: &str) {
    assert!(
        child.left() + px(EPS) >= parent.left()
            && child.top() + px(EPS) >= parent.top()
            && child.right() <= parent.right() + px(EPS)
            && child.bottom() <= parent.bottom() + px(EPS),
        "{label}: {child:?} escapes {parent:?}"
    );
}

fn inside(parent: Bounds<Pixels>, child: Bounds<Pixels>) -> bool {
    child.top() + px(EPS) >= parent.top() && child.bottom() <= parent.bottom() + px(EPS)
}

fn scroll(cx: &mut VisualTestAppContext, win: AnyWindowHandle, at: gpui::Point<Pixels>, dy: f32) {
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: at,
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(dy))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cx.run_until_parked();
}

// ── Shared assertions ────────────────────────────────────────────────────

/// The card, the action row pinned to its bottom, and every button in that row
/// are laid out inside the window. This is issue #462 itself: on a short
/// window the card used to run past the bottom edge and take the buttons with
/// it.
fn assert_card_fits(
    cx: &mut VisualTestAppContext,
    case: &Case,
    buttons: &[&str],
) -> Bounds<Pixels> {
    let label = &case.label;
    let mut names: Vec<&str> = vec!["modal-card", "modal-footer"];
    names.extend_from_slice(buttons);
    // One frame: a card and a footer measured in different frames could
    // disagree about a layout that is still settling.
    let frame = measure(cx, case.win, &names);
    let card = frame[0].unwrap_or_else(|| panic!("{label}: modal-card was not laid out"));
    let footer = frame[1].unwrap_or_else(|| panic!("{label}: modal-footer was not laid out"));
    solid(card, &format!("{label}: card"));
    solid(footer, &format!("{label}: footer"));
    contained(case.viewport, card, &format!("{label}: card"));
    contained(card, footer, &format!("{label}: footer inside its card"));
    contained(case.viewport, footer, &format!("{label}: footer"));
    for (button, bounds) in buttons.iter().zip(&frame[2..]) {
        let bounds = bounds.unwrap_or_else(|| panic!("{label}: {button} was not laid out"));
        solid(bounds, &format!("{label}: {button}"));
        contained(footer, bounds, &format!("{label}: {button}"));
    }
    card
}

/// The list of things the operation acts on: a panel inside the card, holding
/// real rows, with the end of the list reachable by scrolling.
///
/// `paths` are the fixture's own targets in the order the status reports them,
/// so the row probes are named without reading them back off the modal.
fn assert_targets(
    cx: &mut VisualTestAppContext,
    case: &Case,
    card: Bounds<Pixels>,
    paths: &[String],
) {
    let label = &case.label;
    let (panel, list, visible) = target_frame(cx, case, paths);
    contained(card, panel, &format!("{label}: target panel"));
    let floor = case.min_rows();
    assert!(
        visible >= floor,
        "{label}: {visible} target row(s) visible, expected at least {floor}. \
         A confirmation must always show what it acts on."
    );

    // Every target must be reachable, not merely the first screenful: the
    // regression behind #454/#462 was a list that clipped the rest away.
    scroll(cx, case.win, list.center(), -100_000.);
    let (panel, list, reached) = target_frame(cx, case, &paths[paths.len() - 1..]);
    contained(card, panel, &format!("{label}: target panel at the bottom"));
    assert_eq!(
        reached, 1,
        "{label}: scrolling to the bottom never exposed the final target"
    );
    scroll(cx, case.win, list.center(), 100_000.);
}

/// One frame: the target panel's bounds, and how many of `paths` that frame
/// laid out fully inside it.
fn target_frame(
    cx: &mut VisualTestAppContext,
    case: &Case,
    paths: &[String],
) -> (Bounds<Pixels>, Bounds<Pixels>, usize) {
    let label = &case.label;
    let names: Vec<String> = ["modal-target-panel", "modal-target-list", "modal-body"]
        .into_iter()
        .map(str::to_string)
        .chain(paths.iter().map(|path| format!("modal-file-{path}")))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let frame = measure(cx, case.win, &refs);
    let panel = frame[0].unwrap_or_else(|| panic!("{label}: modal-target-panel was not laid out"));
    solid(panel, &format!("{label}: target panel"));
    let list = frame[1].unwrap_or_else(|| panic!("{label}: target scroller was not laid out"));
    solid(list, &format!("{label}: target scroller"));
    contained(panel, list, &format!("{label}: target scroller"));
    contained(
        frame[2].expect("modal body"),
        panel,
        &format!("{label}: target panel"),
    );
    let mut visible = 0;
    for (name, row) in refs[3..].iter().zip(&frame[3..]) {
        let Some(row) = *row else { continue };
        solid(row, &format!("{label}: {name}"));
        if inside(list, row) {
            contained(case.viewport, row, &format!("{label}: {name}"));
            visible += 1;
        }
    }
    (panel, list, visible)
}

/// A collapsible section: its header is always drawn inside the card, and its
/// body is drawn exactly when the section is expanded. Card and section are
/// measured in the same frame, so a card that resized with the section is
/// compared against, not a stale one.
fn assert_section(cx: &mut VisualTestAppContext, case: &Case, id: &str, expect_open: bool) {
    let label = &case.label;
    let body_name = format!("{id}-body");
    let frame = measure(
        cx,
        case.win,
        &["modal-card", id, body_name.as_str(), "modal-body"],
    );
    let card = frame[0].unwrap_or_else(|| panic!("{label}: modal-card was not laid out"));
    let header = frame[1].unwrap_or_else(|| panic!("{label}: {id} header was not laid out"));
    solid(header, &format!("{label}: {id}"));
    contained(card, header, &format!("{label}: {id} header"));
    contained(
        frame[3].expect("modal body"),
        header,
        &format!("{label}: {id} header"),
    );
    let body = frame[2].is_some();
    assert_eq!(
        body, expect_open,
        "{label}: {id} body rendered={body}, expected={expect_open}"
    );
    if let Some(section) = frame[2] {
        contained(card, section, &format!("{label}: {id} section"));
        contained(
            frame[3].expect("modal body"),
            section,
            &format!("{label}: {id} section"),
        );
        let content_id = if id.ends_with("-recovery") {
            "modal-recovery-scroll"
        } else {
            "modal-warning-content"
        };
        let content = required(cx, case.win, content_id, label);
        solid(content, &format!("{label}: {id} content"));
        contained(section, content, &format!("{label}: {id} content"));
    }
}

/// Click a section header on the real card.
fn toggle_section(cx: &mut VisualTestAppContext, case: &Case, id: &str) {
    let header = required(cx, case.win, id, &case.label);
    cx.simulate_click(case.win, header.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

// ── Cards ────────────────────────────────────────────────────────────────

fn amend_case(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    case: &Case,
    repo: &Path,
    clean: &(String, String),
    staged: &[String],
) {
    let label = case.label.clone();
    open_amend(cx, app, "compact amend");
    // Fixture preconditions: this card is only the subject of the scenario if
    // the real plan is confirmable and carries the checklist warning.
    cx.read(|cx| {
        let modal = app.read(cx).amend_modal().expect("the amend card is open");
        assert!(
            modal.plan.blockers.is_empty(),
            "{label}: fixture must produce a confirmable amend: {:?}",
            modal.plan.blockers
        );
        assert!(
            !modal.plan.warnings.is_empty(),
            "{label}: fixture must stage a file the checklist warns about"
        );
    });

    let card = assert_card_fits(cx, case, &["amend-cancel", "amend-confirm"]);
    assert_targets(cx, case, card, staged);
    // Safety detail stays visible even when the window is short; supporting
    // detail is what compact mode folds.
    assert_section(cx, case, "amend-warnings", true);
    assert_section(cx, case, "amend-recovery", !case.compact);

    // The user outranks the default: clicking the real header flips the
    // section, and the card still fits with it flipped.
    toggle_section(cx, case, "amend-recovery");
    assert_section(cx, case, "amend-recovery", case.compact);
    let card = assert_card_fits(cx, case, &["amend-cancel", "amend-confirm"]);
    assert_targets(cx, case, card, staged);

    // …but the flip must not ride along into the next confirmation: a fresh
    // card starts from the renderer's default again. No test-only reset here —
    // the production open path is what has to do it.
    open_amend(cx, app, "compact amend");
    assert_section(cx, case, "amend-recovery", !case.compact);

    // The responsive default is not allowed to re-decide a section the user
    // has already decided. Compact mode keys off the zoom-normalised window
    // height, and the native queue never settles a resize here, so the zoom is
    // what moves this window across the threshold — a real responsive change
    // through the real root, not an injected viewport.
    //
    // Two clicks: the second lands back on the value the current default
    // happens to have, which is the point — it must be stored as the user's
    // choice, not forgotten as "same as default", or the flipped default
    // silently overrides it.
    toggle_section(cx, case, "amend-recovery");
    assert_section(cx, case, "amend-recovery", case.compact);
    toggle_section(cx, case, "amend-recovery");
    assert_section(cx, case, "amend-recovery", !case.compact);
    if case.viewport.size.height == px(600.) {
        theme::set_zoom(0.8);
        assert_card_fits(cx, case, &["amend-cancel", "amend-confirm"]);
        assert_section(cx, case, "amend-recovery", false);
        assert_section(cx, case, "amend-warnings", true);
        theme::set_zoom(1.);
    }
    theme::set_zoom(case.flip_zoom());
    assert_card_fits(cx, case, &["amend-cancel", "amend-confirm"]);
    assert_section(cx, case, "amend-recovery", !case.compact);
    theme::set_zoom(1.);
    assert_section(cx, case, "amend-recovery", !case.compact);

    // Arming is the first of the two confirm clicks. The armed notice adds
    // prose to an already-full card: the buttons and the targets must survive
    // it, and nothing may be written.
    app.update(cx, |app, cx| app.start_amend(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app
            .read(cx)
            .amend_modal()
            .is_some_and(|modal| modal.confirm_armed)),
        "{label}: the first confirm must arm the card, not execute"
    );
    let card = assert_card_fits(cx, case, &["amend-cancel", "amend-confirm"]);
    assert_targets(cx, case, card, staged);
    assert_body_content(cx, case, "modal-armed-notice");
    assert_section(cx, case, "amend-warnings", true);
    assert_eq!(
        repo_fingerprint(repo),
        *clean,
        "{label}: arming a confirm must not touch the repository"
    );

    // A blocked plan offers no way to proceed — and still fits the window.
    open_amend(cx, app, "");
    assert!(
        cx.read(|cx| app
            .read(cx)
            .amend_modal()
            .is_some_and(|modal| !modal.plan.blockers.is_empty())),
        "{label}: an empty amend message must block the plan"
    );
    assert_card_fits(cx, case, &["amend-cancel"]);
    assert_body_content(cx, case, "modal-blocker-content");
    assert!(
        one(cx, case.win, "amend-confirm").is_none(),
        "{label}: a blocked plan must not render a confirm button"
    );
    app.update(cx, |app, _| app.cancel_amend_modal());
    cx.run_until_parked();
}

fn open_amend(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, message: &str) {
    app.update(cx, |app, cx| {
        let ctx: &gpui::App = cx;
        app.open_amend_modal_with_message(kagi_git::AmendMode::Both, message.to_string(), ctx);
    });
    cx.run_until_parked();
}

fn discard_case(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    case: &Case,
    repo: &Path,
    clean: &(String, String),
    owner: kagi::app::SessionId,
    targets: &[String],
) {
    let label = case.label.clone();
    app.update(cx, |app, cx| app.open_discard_all_modal(owner, cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .discard_modal()
            .expect("the discard card is open");
        assert!(
            modal.plan.blockers.is_empty(),
            "{label}: fixture must produce a confirmable discard: {:?}",
            modal.plan.blockers
        );
        assert!(
            !modal.plan.warnings.is_empty(),
            "{label}: untracked targets must warn that they are deleted"
        );
        assert!(
            modal.paths.len() >= TARGETS,
            "{label}: discard-all must target the whole dirty set, got {}",
            modal.paths.len()
        );
    });

    let card = assert_card_fits(cx, case, &["discard-cancel", "discard-confirm"]);
    assert_targets(cx, case, card, targets);
    assert_section(cx, case, "discard-warnings", true);
    assert_section(cx, case, "discard-recovery", !case.compact);

    app.update(cx, |app, cx| app.start_discard(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app
            .read(cx)
            .discard_modal()
            .is_some_and(|modal| modal.confirm_armed)),
        "{label}: the first Discard click must arm the card, not execute"
    );
    let card = assert_card_fits(cx, case, &["discard-cancel", "discard-confirm"]);
    assert_targets(cx, case, card, targets);
    assert_body_content(cx, case, "modal-armed-notice");
    assert_section(cx, case, "discard-warnings", true);
    assert_eq!(
        repo_fingerprint(repo),
        *clean,
        "{label}: arming discard must not touch the working tree"
    );
    app.update(cx, |app, _| app.cancel_discard_modal());
    cx.run_until_parked();
}

/// The shared plan card (push): same chrome contract, a different renderer —
/// the one every non-destructive confirmation goes through.
fn push_case(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, case: &Case) {
    let label = case.label.clone();
    app.update(cx, |app, cx| app.open_push_modal(cx));
    cx.run_until_parked();
    let final_preview = cx.read(|cx| {
        let modal = app.read(cx).push_modal().expect("the push card is open");
        assert!(
            modal.plan.blockers.is_empty(),
            "{label}: fixture must produce a confirmable push: {:?}",
            modal.plan.blockers
        );
        assert_eq!(
            modal.plan.preview_commits.len(),
            AHEAD,
            "{label}: fixture must preview every commit that would be pushed"
        );
        modal
            .plan
            .preview_commits
            .last()
            .expect("push target")
            .clone()
    });
    let card = assert_card_fits(cx, case, &["plan-cancel", "plan-confirm"]);
    let panel = required(cx, case.win, "modal-target-panel", &label);
    contained(card, panel, &format!("{label}: commits panel"));
    let list = required(cx, case.win, "modal-target-list", &label);
    contained(panel, list, &format!("{label}: commits scroller"));
    scroll(cx, case.win, list.center(), -100_000.);
    let last = required(
        cx,
        case.win,
        &format!("modal-commit-{final_preview}"),
        &label,
    );
    contained(list, last, &format!("{label}: final push target"));

    // Recovery on the shared card: plain prose at a normal height, folded into
    // a section only where the window is short. Measuring both tells the two
    // shapes apart instead of just "something was drawn".
    let recovery = measure(
        cx,
        case.win,
        &[
            "plan-recovery",
            "plan-recovery-body",
            "plan-recovery-scroll",
        ],
    );
    if case.compact {
        assert!(
            recovery[0].is_some() && recovery[1].is_none(),
            "{label}: a short window folds the push recovery into a closed section"
        );
    } else {
        assert!(
            recovery[0].is_none() && recovery[2].is_some(),
            "{label}: a roomy window keeps the push recovery as plain prose"
        );
    }
    app.update(cx, |app, _| app.clear_push_modal());
    cx.run_until_parked();
}

// ── Scenario ─────────────────────────────────────────────────────────────

/// Call from the macOS main-thread runner after `e2e::init_app`.
///
/// 4 viewports × 2 locales, each a fresh window: the amend, discard-all and
/// push cards, at rest, with a flipped disclosure, and armed.
pub fn scenario_modal_compact(cx: &mut VisualTestAppContext) {
    let restore = Restore::capture();
    theme::set_zoom(1.);
    let (_fixture, repo) = fixture();
    let clean = repo_fingerprint(&repo);
    // Probe names in the order the card lists them: git reports the staged and
    // modified sets sorted, and the commit panel appends untracked rows after
    // the modified ones — so the end of each slice is the end of each list.
    let staged: Vec<String> = std::iter::once(".env.local".to_string())
        .chain((0..TARGETS).map(staged_path))
        .collect();
    let targets: Vec<String> = (0..TARGETS)
        .map(target_path)
        .chain((0..UNTRACKED).map(scratch_path))
        .collect();

    for (width, height, compact) in SIZES {
        for lang in [i18n::Lang::En, i18n::Lang::Ja] {
            i18n::set_lang(lang);
            let (app, win) = mount_at(cx, &repo, (width, height));
            let viewport = cx
                .update_window(win, |_, window, _| window.viewport_size())
                .expect("viewport size");
            assert_eq!(
                viewport,
                size(px(width), px(height)),
                "the window did not open at its requested size"
            );
            let case = Case {
                win,
                viewport: Bounds::new(point(px(0.), px(0.)), viewport),
                compact,
                label: format!("{width:.0}x{height:.0} {}", lang.slug()),
            };

            app.update(cx, |app, cx| {
                e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
            });
            cx.run_until_parked();
            let owner = cx.read(|cx| {
                app.read(cx)
                    .ui()
                    .commit_panel
                    .as_ref()
                    .expect("the commit panel carries the discard-all owner")
                    .read(cx)
                    .owner
            });

            amend_case(cx, &app, &case, &repo, &clean, &staged);
            discard_case(cx, &app, &case, &repo, &clean, owner, &targets);
            push_case(cx, &app, &case);
            unmount(cx, app, win);
        }
    }

    assert_eq!(
        repo_fingerprint(&repo),
        clean,
        "the compact matrix must never write to its fixture"
    );
    drop(restore);
    eprintln!(
        "[gui-e2e] PASS modal_compact amend/discard/push cards at 600/700/750/900 \u{d7} en/ja: \
         card, footer, buttons and targets measured inside the window"
    );
}
