//! Issue #548: expanding a huge Operation Log entry must not cost seconds per
//! draw. Included by `gui_e2e_runner`, not libtest — it needs the real GPUI
//! layout + paint of the production `OpLogPanel` renderer, on the main thread.
//!
//! Method (identical to the investigation in #548 so the numbers compare):
//! mount the real root, push one synthetic `Failed` entry whose error is the
//! measured 158,051 bytes, expand it, then time `window.refresh();
//! window.draw(cx).clear()` — one warm-up draw dropped, median of 7.

use std::time::Instant;

use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::{e2e, BottomTab};

/// The size of the real `rebase --abort` error that triggered #548.
const PAYLOAD_BYTES: usize = 158_051;

/// Upper bound for the median draw. The bug measured ~9,700 ms; with selection
/// off for a block this size it is single-digit ms. Generous by ~40x so a busy
/// machine cannot flake it, and still three orders of magnitude below the bug.
const MAX_MS_PER_DRAW: f64 = 200.;

/// An ASCII run of comma-separated paths, exactly `bytes` long — the shape of a
/// real abort error (no real repository names or paths).
fn payload(bytes: usize) -> String {
    let unit = "src/components/long_directory_name/file_name.rs, ";
    let mut s = unit.repeat(bytes.div_ceil(unit.len()));
    s.truncate(bytes); // ASCII: always a char boundary.
    s
}

fn draw_ms(cx: &mut VisualTestAppContext, win: AnyWindowHandle) -> f64 {
    let start = Instant::now();
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw window");
    start.elapsed().as_secs_f64() * 1000.
}

pub fn scenario_expanded_detail_draw(cx: &mut VisualTestAppContext) {
    let fixture = crate::macos::build_fixture();
    let repo_path = fixture.path().canonicalize().unwrap();
    let before_fp = crate::macos::repo_fingerprint(&repo_path);
    let (kagi, win) = crate::macos::mount(cx, &repo_path);

    kagi.update(cx, |app, cx| {
        app.bottom_panel_open = true;
        app.bottom_tab = BottomTab::OperationLog;
        e2e::push_failed_op(app, "rebase-abort", payload(PAYLOAD_BYTES), cx);
        cx.notify();
    });
    cx.run_until_parked();

    let panel = cx
        .read(|app| kagi.read(app).op_log.clone())
        .expect("op_log entity");
    panel.update(cx, |p, cx| {
        p.toggle_expanded(0);
        cx.notify();
    });
    cx.run_until_parked();

    let warmup = draw_ms(cx, win);
    let mut times: Vec<f64> = (0..7).map(|_| draw_ms(cx, win)).collect();
    times.sort_by(f64::total_cmp);
    let (median, max) = (times[times.len() / 2], *times.last().unwrap());

    // The whole detail really is laid out (the row grew far past the 22px
    // summary line) — otherwise a clipped row would "measure" fast for free.
    let height = cx.read(|app| {
        panel
            .read(app)
            .scroll_handle()
            .bounds_for_item(0)
            .expect("row 0 laid out")
            .size
            .height
    });
    assert!(
        height > gpui::px(1000.),
        "expanded row is only {height:?} tall — the 158 KB detail was not laid out"
    );
    assert_eq!(
        before_fp,
        crate::macos::repo_fingerprint(&repo_path),
        "repo mutated during a read-only scenario"
    );
    eprintln!(
        "[gui-e2e] PASS oplog_detail_draw bytes={PAYLOAD_BYTES} \
         warmup_ms={warmup:.3} median_ms={median:.3} max_ms={max:.3} row_height={height:?}"
    );
    assert!(
        median < MAX_MS_PER_DRAW,
        "expanded {PAYLOAD_BYTES}-byte op-log detail costs {median:.3} ms/draw \
         (limit {MAX_MS_PER_DRAW} ms) — issue #548 regressed"
    );
}
