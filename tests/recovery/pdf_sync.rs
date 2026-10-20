//! Historical PDF sync uses real immutable multi-page documents and native input.
use crate::commit_pdf::{fixture, open, probe, viewer, wait_pdf, wheel};
use crate::diff_highlight::settle;
use crate::keyboard_nav::keys;
use crate::macos::{mount, repo_fingerprint, unmount};
use crate::native_ax;
use crate::pr_viewed::paint;
use gpui::{
    point, px, AnyWindowHandle, Entity, Focusable as _, Modifiers, MouseButton,
    VisualTestAppContext,
};
use kagi::ui::{main_diff_pane::MainDiffPane, KagiApp};
use kagi_ui_core::i18n::{self, Lang, Msg};
use kagi_ui_editor::{PdfProbe, PdfScrollChanged, PdfScrollPosition, PdfViewer};

const DUAL: &str = "dual.pdf";
const CONTROL: &str = "main-diff-pdf-sync";

fn mode(cx: &mut VisualTestAppContext, pane: &Entity<MainDiffPane>) -> bool {
    cx.read(|cx| pane.read(cx).pdf_scroll_synchronized_for_e2e(cx))
}

fn control(
    cx: &mut VisualTestAppContext,
    pane: &Entity<MainDiffPane>,
) -> gpui::Bounds<gpui::Pixels> {
    cx.read(|cx| {
        let fit = &pane.read(cx).fit;
        let control = fit
            .control_bounds(CONTROL)
            .expect("visible PDF scrolling mode");
        assert!(fit.row_bounds().unwrap().contains(&control.origin));
        assert!(fit.row_bounds().unwrap().contains(&control.bottom_right()));
        assert!(control.size.width > px(0.) && control.size.height > px(0.));
        control
    })
}

fn scroll_wheel(
    cx: &mut VisualTestAppContext,
    viewer: &Entity<PdfViewer>,
    window: AnyWindowHandle,
    delta: f32,
) {
    let pdf = probe(cx, viewer);
    wheel(cx, window, &pdf, delta);
}

fn tab_to_mode(cx: &mut VisualTestAppContext, window: AnyWindowHandle, synchronized: bool) {
    let label = if synchronized {
        Msg::DiffPdfScrollSynchronized.t()
    } else {
        Msg::DiffPdfScrollIndependent.t()
    };
    for _ in 0..200 {
        keys(cx, window, "tab");
        paint(cx, window);
        let tree = native_ax::tree(cx, window);
        if let Some(id) = tree["gpui_focus"].as_str() {
            let node = &tree["nodes"][id];
            if node["aria"]["role"] == "Button" && node["aria"]["label"] == label {
                return;
            }
        }
    }
    panic!("actual Tab traversal did not reach {label}");
}

fn focus_body(cx: &mut VisualTestAppContext, viewer: &Entity<PdfViewer>, window: AnyWindowHandle) {
    let pdf = wait_pdf(cx, viewer, window);
    cx.simulate_click(window, pdf.scroll.bounds().center(), Modifiers::none());
    paint(cx, window);
    assert_body_focus(cx, viewer, window);
}

fn assert_body_focus(
    cx: &mut VisualTestAppContext,
    viewer: &Entity<PdfViewer>,
    window: AnyWindowHandle,
) {
    assert!(
        cx.update_window(window, |_, window, cx| {
            viewer.read(cx).focus_handle(cx).is_focused(window)
        })
        .unwrap(),
        "synchronizing a peer must never steal the leader's body focus"
    );
}

fn assert_bounds(pdf: &PdfProbe) {
    let offset = pdf.scroll.offset();
    let max = pdf.scroll.max_offset();
    assert!(offset.x >= -max.x && offset.x <= px(0.) && offset.y >= -max.y && offset.y <= px(0.));
    assert!(pdf.cached_bytes <= 64_000_000 && pdf.raster_jobs <= 2);
    assert!(pdf.page < pdf.page_count && pdf.image.is_some() && pdf.error.is_none());
}

// Independent oracle: use actual page geometry and scroll offsets, not the
// viewer's public position getter or synchronization conversion implementation.
fn assert_alignment(leader: &PdfProbe, peer: &PdfProbe) {
    assert_bounds(leader);
    assert_bounds(peer);
    let y = -f32::from(leader.scroll.offset().y);
    let page = leader
        .page_tops
        .iter()
        .rposition(|top| *top <= y)
        .unwrap_or(0);
    let fraction =
        ((y - leader.page_tops[page]) / (leader.page_sizes[page].1 * leader.zoom)).clamp(0., 1.);
    let target = page.min(peer.page_count - 1);
    let expected = (peer.page_tops[target] + fraction * peer.page_sizes[target].1 * peer.zoom)
        .min(f32::from(peer.scroll.max_offset().y));
    assert!((-f32::from(peer.scroll.offset().y) - expected).abs() < 1.5,
        "page/fraction alignment, not raw pixels: leader page {page} fraction {fraction}, expected {expected}, actual {:?}", peer.scroll.offset());
    let extent = f32::from(leader.scroll.max_offset().x);
    let fraction = if extent > 0. {
        -f32::from(leader.scroll.offset().x) / extent
    } else {
        0.
    };
    let expected = fraction * f32::from(peer.scroll.max_offset().x);
    assert!((-f32::from(peer.scroll.offset().x) - expected).abs() < 1.5);
}

fn aligned(
    cx: &mut VisualTestAppContext,
    lead: &Entity<PdfViewer>,
    peer: &Entity<PdfViewer>,
    window: AnyWindowHandle,
) {
    let leader = wait_pdf(cx, lead, window);
    let other = wait_pdf(cx, peer, window);
    assert_alignment(&leader, &other);
    let offsets = (leader.scroll.offset(), other.scroll.offset());
    for _ in 0..6 {
        paint(cx, window);
    }
    assert_eq!(
        (
            probe(cx, lead).scroll.offset(),
            probe(cx, peer).scroll.offset()
        ),
        offsets,
        "a clamped peer must not echo its position back and move the leader"
    );
}

fn mount_pair(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
) -> (
    Entity<KagiApp>,
    AnyWindowHandle,
    Entity<MainDiffPane>,
    Entity<PdfViewer>,
    Entity<PdfViewer>,
) {
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    settle(cx, window);
    let pane = open(cx, &app, window, DUAL);
    let old = viewer(cx, &pane, true).unwrap();
    let new = viewer(cx, &pane, false).unwrap();
    wait_pdf(cx, &old, window);
    wait_pdf(cx, &new, window);
    native_ax::activate(cx);
    (app, window, pane, old, new)
}

pub fn scenario_commit_pdf_sync_scrolling(cx: &mut VisualTestAppContext) {
    let _presentation = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let fingerprint = repo_fingerprint(&repo);
    let (app, window, pane, old, new) = mount_pair(cx, &repo);
    cx.update(|cx| {
        gpui_component::Theme::global_mut(cx).scrollbar_show =
            gpui_component::scroll::ScrollbarShow::Always
    });
    app.update(cx, |_, cx| cx.notify());
    paint(cx, window);
    let identity = cx.read(|cx| {
        let old = old.read(cx).pdf_probe();
        let new = new.read(cx).pdf_probe();
        (old.source_id, old.zoom, new.source_id, new.zoom)
    });
    assert!(
        !mode(cx, &pane),
        "comparison defaults to independent scrolling"
    );
    let new_initial = probe(cx, &new).scroll.offset();
    scroll_wheel(cx, &old, window, -450.);
    wait_pdf(cx, &old, window);
    assert_eq!(probe(cx, &new).scroll.offset(), new_initial);
    let old_independent = probe(cx, &old).scroll.offset();
    scroll_wheel(cx, &new, window, -800.);
    wait_pdf(cx, &new, window);
    assert_eq!(probe(cx, &old).scroll.offset(), old_independent);

    // A wheel on the unfocused right document is newer than the left's focus
    // event. Moving to the mode control must preserve that latest reading
    // interaction, rather than overriding it with stale focus history.
    focus_body(cx, &old, window);
    scroll_wheel(cx, &new, window, -200.);
    wait_pdf(cx, &new, window);
    assert_body_focus(cx, &old, window);
    let leader_offset = probe(cx, &new).scroll.offset();
    let mode_bounds = control(cx, &pane);
    cx.simulate_click(window, mode_bounds.center(), Modifiers::none());
    paint(cx, window);
    assert!(mode(cx, &pane));
    aligned(cx, &new, &old, window);
    assert_eq!(probe(cx, &new).scroll.offset(), leader_offset);
    assert_ne!(
        probe(cx, &old).scroll.offset().y,
        probe(cx, &new).scroll.offset().y,
        "unequal page sizes require different physical offsets"
    );

    focus_body(cx, &old, window);
    keys(cx, window, "pagedown");
    aligned(cx, &old, &new, window);
    assert_body_focus(cx, &old, window);
    // Toolbar page navigation uses the same synchronization path as wheel/keys.
    assert!(native_ax::press(cx, window, Msg::EditorPdfNext.t()));
    aligned(cx, &old, &new, window);
    scroll_wheel(cx, &new, window, -310.);
    aligned(cx, &new, &old, window);

    focus_body(cx, &new, window);
    keys(cx, window, "end");
    aligned(cx, &new, &old, window);
    assert_body_focus(cx, &new, window);
    assert_eq!(probe(cx, &new).page, 4);
    assert_eq!(
        probe(cx, &old).page,
        2,
        "missing peer pages clamp to the final valid page"
    );
    keys(cx, window, "home space");
    aligned(cx, &new, &old, window);

    // The visible mode is a real named Tab stop, Enter and Space both toggle.
    tab_to_mode(cx, window, true);
    let before = (
        probe(cx, &old).scroll.offset(),
        probe(cx, &new).scroll.offset(),
    );
    keys(cx, window, "enter");
    paint(cx, window);
    assert!(!mode(cx, &pane));
    assert_eq!(
        (
            probe(cx, &old).scroll.offset(),
            probe(cx, &new).scroll.offset()
        ),
        before,
        "turning synchronization off leaves both reading positions intact"
    );
    let old_offset = probe(cx, &old).scroll.offset();
    scroll_wheel(cx, &new, window, -220.);
    wait_pdf(cx, &new, window);
    assert_eq!(probe(cx, &old).scroll.offset(), old_offset);
    tab_to_mode(cx, window, false);
    keys(cx, window, "space");
    paint(cx, window);
    assert!(mode(cx, &pane));

    // Shared scrollbar thumb drag is a genuine native pointer gesture. Keep
    // toolbar focus: dragging must not focus the peer or leak into Graph.
    let initial = wait_pdf(cx, &new, window);
    let bounds = initial.scroll.bounds();
    let extent = initial.scroll.max_offset().y;
    let thumb =
        (bounds.size.height / (bounds.size.height + extent) * bounds.size.height).max(px(48.));
    let start = -initial.scroll.offset().y / extent * (bounds.size.height - thumb);
    let from = point(bounds.right() - px(8.), bounds.top() + start + thumb / 2.);
    let to = point(from.x, bounds.bottom() + px(48.));
    let focused = native_ax::focused(cx, window)["element_id"].clone();
    cx.simulate_mouse_move(window, from, None, Modifiers::none());
    paint(cx, window);
    cx.simulate_mouse_down(window, from, MouseButton::Left, Modifiers::none());
    for step in 1..=5 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        cx.simulate_mouse_move(
            window,
            from + (to - from) * (step as f32 / 5.),
            MouseButton::Left,
            Modifiers::none(),
        );
        paint(cx, window);
    }
    cx.simulate_mouse_up(window, to, MouseButton::Left, Modifiers::none());
    aligned(cx, &new, &old, window);
    assert_eq!(probe(cx, &new).scroll.offset().y, -extent);
    assert_eq!(native_ax::focused(cx, window)["element_id"], focused);
    assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(0));
    assert_eq!(
        cx.read(|cx| {
            let old = old.read(cx).pdf_probe();
            let new = new.read(cx).pdf_probe();
            (old.source_id, old.zoom, new.source_id, new.zoom)
        }),
        identity,
        "synchronization never replaces historical sources or changes either zoom"
    );
    assert_eq!(repo_fingerprint(&repo), fingerprint);
    drop((old, new, pane));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS commit_pdf_sync_scrolling");
}

pub fn scenario_commit_pdf_sync_lifecycle(cx: &mut VisualTestAppContext) {
    let restore = crate::recovery_layout::GlobalSettings::capture();
    let fixture = fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window, pane, old, new) = mount_pair(cx, &repo);
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        app.update(cx, |_, cx| cx.notify());
        paint(cx, window);
        let before = mode(cx, &pane);
        tab_to_mode(cx, window, before);
        keys(cx, window, "space");
        paint(cx, window);
        assert_ne!(mode(cx, &pane), before);
        control(cx, &pane);
    }
    if !mode(cx, &pane) {
        tab_to_mode(cx, window, false);
        keys(cx, window, "enter");
        paint(cx, window);
    }
    focus_body(cx, &new, window);
    keys(cx, window, "pagedown");
    aligned(cx, &new, &old, window);
    let offsets = (
        probe(cx, &old).scroll.offset(),
        probe(cx, &new).scroll.offset(),
    );
    let retained = open(cx, &app, window, DUAL);
    assert!(
        mode(cx, &retained),
        "same immutable comparison keeps its mode"
    );
    assert_eq!(
        viewer(cx, &retained, true).unwrap().entity_id(),
        old.entity_id()
    );
    assert_eq!(
        viewer(cx, &retained, false).unwrap().entity_id(),
        new.entity_id()
    );
    assert_eq!(
        (
            probe(cx, &old).scroll.offset(),
            probe(cx, &new).scroll.offset()
        ),
        offsets
    );
    drop(retained);

    // Replacement drops the old pair's subscriptions even if another consumer
    // still holds the individual old viewer entities. Inject a real entity event
    // at that lifecycle seam, where an unmounted viewer cannot receive wheel input.
    let replacement = open(cx, &app, window, "signature.note");
    assert!(
        !mode(cx, &replacement),
        "new comparison defaults to independent"
    );
    let current = viewer(cx, &replacement, false).unwrap();
    let current_offset = wait_pdf(cx, &current, window).scroll.offset();
    old.update(cx, |_, cx| {
        cx.emit(PdfScrollChanged {
            position: PdfScrollPosition {
                page: 2,
                page_fraction: 0.75,
                horizontal_fraction: 0.5,
            },
        })
    });
    paint(cx, window);
    assert_eq!(
        probe(cx, &new).scroll.offset(),
        offsets.1,
        "retired pair cannot update its stale peer"
    );
    assert_eq!(
        probe(cx, &current).scroll.offset(),
        current_offset,
        "retired pair cannot update the accepted comparison"
    );
    drop((old, new, current, replacement));

    for path in [
        "added.pdf",
        "deleted.pdf",
        "repaired.pdf",
        "malformed.pdf",
        "password.pdf",
    ] {
        let next = open(cx, &app, window, path);
        paint(cx, window);
        assert!(!mode(cx, &next));
        let tree = native_ax::tree(cx, window);
        assert!(
            !tree["nodes"].as_object().unwrap().values().any(|node| {
                node["element_id"] == format!("{:?}", gpui::ElementId::from(CONTROL))
            }),
            "absent or malformed sides have no meaningful synchronization control: {path}"
        );
        drop(next);
    }
    drop(pane);
    unmount(cx, app, window);
    drop(restore);
    eprintln!("[gui-e2e] PASS commit_pdf_sync_lifecycle");
}
