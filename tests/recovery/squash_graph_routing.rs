//! #1627: local squash routes through the production callback and real canvas.
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_layout::GlobalSettings;
use gpui::{point, px, AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::graph::{EdgeKind, GraphEdge, WipAnchor};
use kagi::ui::graph_view::{
    self, PaintedGraphDash, PaintedGraphEndpoint, PaintedGraphNode,
    PaintedGraphPathCommand as Command,
};
use kagi::ui::{e2e, graph_squash::GHOST_COLOR, graph_wip::WipTarget, KagiApp};
use kagi_git::ops::SquashLink;
use kagi_ui_core::theme;
use std::path::Path;

const EPS: f32 = 1.5;
const PAIRS: [(usize, usize); 3] = [(1, 4), (3, 6), (7, 10)];
type History = Vec<(String, usize, usize, Vec<GraphEdge>)>;
type Frame = (
    Vec<PaintedGraphNode>,
    Vec<PaintedGraphDash>,
    Vec<PaintedGraphEndpoint>,
);

fn output(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .current_dir(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("fixture git output");
    assert!(out.status.success(), "git {args:?}: {:?}", out.stderr);
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn commit(repo: &Path, tree: &str, parents: &[String], tick: usize, label: &str) -> String {
    let date = format!("{} +0000", 2_000_000_000 + tick);
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(repo)
        .args(["commit-tree", tree, "-m", label]);
    for parent in parents {
        cmd.args(["-p", parent]);
    }
    let out = cmd
        .env("GIT_AUTHOR_NAME", "routing fixture")
        .env("GIT_AUTHOR_EMAIL", "routing@example.invalid")
        .env("GIT_COMMITTER_NAME", "routing fixture")
        .env("GIT_COMMITTER_EMAIL", "routing@example.invalid")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("commit-tree");
    assert!(out.status.success(), "commit-tree: {:?}", out.stderr);
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// Old octopus history really needs 26 simultaneous lanes. A newer merge
/// holds a second real parent rail beside the local first-parent chain, so
/// lanes 0/1 are occupied while lanes 2/3 are locally free. No row projection,
/// lane_count, parent edge or WIP anchor is seeded by the test.
fn fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    let repo = fixture.path();
    let root = output(repo, &["rev-parse", "HEAD"]);
    let tree = output(repo, &["rev-parse", "HEAD^{tree}"]);
    let side = commit(
        repo,
        &tree,
        std::slice::from_ref(&root),
        1,
        "old side parent",
    );
    let parents: Vec<_> = (0..26)
        .map(|ix| {
            commit(
                repo,
                &tree,
                std::slice::from_ref(&root),
                ix + 2,
                &format!("wide {ix}"),
            )
        })
        .collect();
    let mut main = commit(repo, &tree, &parents, 30, "wide old merge");
    for ix in 0..12 {
        main = commit(repo, &tree, &[main], 40 + ix, &format!("local {ix}"));
    }
    let head = commit(
        repo,
        &tree,
        &[main, side],
        60,
        "local merge holds side rail",
    );
    // Every synthetic commit keeps HEAD's original tree, so advancing the ref
    // leaves index/worktree intact. Only the following edit creates WIP.
    git(repo, &["update-ref", "refs/heads/main", &head, &root]);
    std::fs::write(
        repo.join("README.md"),
        "dirty WIP anchor remains unchanged\n",
    )
    .unwrap();
    fixture
}

fn history(app: &KagiApp) -> History {
    app.view()
        .rows
        .iter()
        .map(|row| {
            (
                row.id.0.clone(),
                row.lane,
                row.node_color,
                row.edges
                    .iter()
                    .filter(|edge| edge.color != GHOST_COLOR)
                    .cloned()
                    .collect(),
            )
        })
        .collect()
}

fn anchors(app: &KagiApp) -> Vec<(WipTarget, Option<WipAnchor>)> {
    app.view().wip_lanes.clone()
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> Frame {
    graph_view::take_paint_trace(window.window_id());
    graph_view::take_endpoint_paint_trace(window.window_id());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    let (nodes, dashes) = graph_view::take_paint_trace(window.window_id());
    let endpoints = graph_view::take_endpoint_paint_trace(window.window_id());
    assert!(!nodes.is_empty(), "real canvas did not paint");
    (nodes, dashes, endpoints)
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= EPS
}
fn x(p: gpui::Point<gpui::Pixels>) -> f32 {
    f32::from(p.x)
}
fn y(p: gpui::Point<gpui::Pixels>) -> f32 {
    f32::from(p.y)
}

fn commits(frame: &Frame) -> Vec<&PaintedGraphNode> {
    let mut nodes: Vec<_> = frame.0.iter().filter(|n| !n.hollow).collect();
    nodes.sort_by(|a, b| a.center.y.partial_cmp(&b.center.y).unwrap());
    nodes
}

/// Walk the actual submitted dash segments, allowing only the painter's 3px
/// scaled dash gap (plus snapping), never an absent whole row.
fn reaches(mut segments: Vec<(f32, f32)>, start: f32, end: f32) -> bool {
    segments.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut cursor = start;
    for (lo, hi) in segments {
        if hi < cursor {
            continue;
        }
        if lo > cursor + theme::scaled(3.) + EPS {
            return false;
        }
        cursor = cursor.max(hi);
        if cursor + EPS >= end {
            return true;
        }
    }
    cursor + EPS >= end
}

/// Validate every move/line/curve command, not just the vertical rail's x.
/// These commands are intercepted as they are forwarded to PathBuilder and
/// published only after the real window.paint_path succeeds.
fn endpoint<'a>(
    frame: &'a Frame,
    node: &PaintedGraphNode,
    kind: EdgeKind,
) -> &'a PaintedGraphEndpoint {
    let mut matching = frame
        .2
        .iter()
        .filter(|p| p.kind == kind && near(f32::from(p.bounds.center().y), y(node.center)));
    let path = matching.next().expect("painted squash endpoint missing");
    assert!(matching.next().is_none(), "duplicate squash endpoint paint");
    let [Command::Move(start), Command::Line(leg), Command::Curve { to: arc, control }, Command::Line(end)] =
        path.commands.as_slice()
    else {
        panic!(
            "endpoint lost its full horizontal/rounded/vertical path: {:?}",
            path.commands
        );
    };
    if kind == EdgeKind::OutOfNode {
        assert!(near(x(*start), x(node.center)) && near(y(*start), y(node.center)));
        assert!(
            near(y(*leg), y(*start)) && x(*leg) > x(*start),
            "horizontal outgoing leg missing"
        );
        assert!(near(x(*control), x(*arc)) && near(y(*control), y(*leg)));
        assert!(
            x(*arc) > x(*leg) && y(*arc) > y(*leg),
            "outgoing curve missing"
        );
        assert!(near(x(*end), x(*arc)) && near(y(*end), f32::from(path.bounds.bottom())));
        assert!(y(*end) > y(*arc), "vertical outgoing leg missing");
    } else {
        assert!(near(y(*start), f32::from(path.bounds.top())) && near(x(*start), x(*leg)));
        assert!(y(*leg) > y(*start), "vertical incoming leg missing");
        assert!(near(x(*control), x(*leg)) && near(y(*control), y(*arc)));
        assert!(
            x(*arc) < x(*leg) && y(*arc) > y(*leg),
            "incoming curve missing"
        );
        assert!(
            near(y(*arc), y(*end)) && x(*arc) > x(*end),
            "horizontal incoming leg missing"
        );
        assert!(near(x(*end), x(node.center)) && near(y(*end), y(node.center)));
    }
    path
}

fn path_points(path: &PaintedGraphEndpoint) -> Vec<gpui::Point<gpui::Pixels>> {
    path.commands
        .iter()
        .flat_map(|command| match *command {
            Command::Move(p) | Command::Line(p) => vec![p],
            Command::Curve { to, control } => vec![to, control],
        })
        .collect()
}

fn assert_translated(before: &Frame, after: &Frame, dx: f32, dy: f32) {
    assert_eq!(
        before.2.len(),
        after.2.len(),
        "scroll lost an endpoint paint"
    );
    for old in &before.2 {
        let new = after
            .2
            .iter()
            .find(|new| {
                new.kind == old.kind
                    && near(
                        f32::from(new.bounds.top()),
                        f32::from(old.bounds.top()) + dy,
                    )
            })
            .expect("wheel did not translate the actual endpoint paint");
        assert_eq!(
            old.commands.len(),
            new.commands.len(),
            "scroll changed path topology"
        );
        assert_eq!(old.color, new.color, "scroll changed squash styling");
        let old_points = path_points(old);
        let new_points = path_points(new);
        assert_eq!(old_points.len(), new_points.len());
        for (a, b) in old_points.into_iter().zip(new_points) {
            assert!(
                near(x(a) + dx, x(b)) && near(y(a) + dy, y(b)),
                "wheel broke full horizontal/curved/vertical endpoint geometry: {a:?} -> {b:?}"
            );
        }
    }
}

fn check_routes(frame: &Frame) -> [f32; 3] {
    let nodes = commits(frame);
    assert!(nodes.len() > 10, "all local endpoints must really paint");
    assert_eq!(
        frame.2.len(),
        6,
        "three distinct ghost paths need six endpoint paints"
    );
    let pitch = theme::scaled(graph_view::LANE_W);
    let mut rails = [0.; 3];
    for (ix, &(top, bottom)) in PAIRS.iter().enumerate() {
        let out = endpoint(frame, nodes[top], EdgeKind::OutOfNode);
        let into = endpoint(frame, nodes[bottom], EdgeKind::IntoNode);
        let points = path_points(out);
        let start = *points.first().unwrap();
        let rail = *points.last().unwrap();
        rails[ix] = x(rail);
        let expected_lanes = if ix == 1 { 3. } else { 2. };
        // Old allocator submits x at lane 26+, not a missing-new-API failure.
        assert!(near(x(rail) - x(start), expected_lanes * pitch),
            "ghost {ix} actually painted a far-right/wrong route: start={start:?}, rail={rail:?}, pitch={pitch}");
        let into_points = path_points(into);
        assert!(
            near(x(into_points[0]), x(rail)),
            "endpoint curves do not join the same rail"
        );
        for p in points.iter().chain(&into_points) {
            assert!(
                x(*p) >= x(start) - EPS && x(*p) <= x(rail) + EPS,
                "endpoint/control point escaped the local route: {p:?}"
            );
            assert!(
                p.x >= out.bounds.left() && p.x <= out.bounds.right(),
                "a supposedly local endpoint is clipped horizontally: {p:?}"
            );
        }
        let mut muted: gpui::Hsla = gpui::rgb(theme::theme().text_muted).into();
        muted.a = 0.55;
        assert_eq!(out.color, muted, "outgoing squash style changed");
        assert_eq!(into.color, muted, "incoming squash style changed");
        let segments = frame
            .1
            .iter()
            .filter(|d| {
                near(x(d.from), x(rail))
                    && near(x(d.to), x(rail))
                    && d.color == muted
                    && y(d.from) >= y(rail) - EPS
                    && y(d.to) <= y(into_points[0]) + EPS
            })
            .map(|d| (y(d.from), y(d.to)))
            .collect();
        assert!(
            reaches(segments, y(rail), y(into_points[0])),
            "ghost {ix} lost straight-run continuity between actual endpoint paints"
        );
    }
    assert!(!near(rails[0], rails[1]), "overlapping ghosts share a rail");
    assert!(
        near(rails[0], rails[2]),
        "disjoint ghosts failed to reuse the local rail"
    );
    rails
}

fn assert_preserved(before: &Frame, after: &Frame) {
    assert_eq!(
        before.0.len(),
        after.0.len(),
        "squash changed visible node count"
    );
    for (a, b) in before.0.iter().zip(&after.0) {
        assert_eq!(a.center, b.center, "squash moved a real node/WIP anchor");
        assert_eq!(a.color, b.color, "squash recoloured a real node/WIP anchor");
        assert_eq!(a.hollow, b.hollow);
        assert_eq!(a.radius, b.radius);
    }
    assert!(
        after.0.iter().any(|n| n.hollow),
        "dirty fixture must paint a WIP anchor"
    );
}

fn wheel(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    position: gpui::Point<gpui::Pixels>,
    dx: f32,
    dy: f32,
) {
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(point(px(dx), px(dy))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cx.run_until_parked();
}

pub fn scenario_squash_graph_routing(cx: &mut VisualTestAppContext) {
    let _globals = GlobalSettings::capture();
    // 1440px / 167% is below the current default 70rem compact breakpoint.
    // Keep this graph-column scenario on the supported desktop projection at
    // both zooms; GlobalSettings restores the threshold and persisted key.
    theme::set_compact_threshold_rem(40);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["graph_lane_compact"]);
    struct RestoreStyle(bool);
    impl Drop for RestoreStyle {
        fn drop(&mut self) {
            theme::set_graph_lane_compact(self.0);
        }
    }
    let _style = RestoreStyle(theme::graph_lane_compact());
    let fixture = fixture();
    for avatar in [false, true] {
        for zoom in [1., 1.67] {
            theme::set_graph_lane_compact(avatar);
            theme::set_zoom(zoom);
            assert!(
                !kagi_domain::compact::is_compact(
                    1440.,
                    theme::rem_size_px(),
                    theme::compact_threshold_rem(),
                ),
                "the graph-column fixture unexpectedly entered compact chrome"
            );
            let (app, window): (Entity<KagiApp>, _) = mount(cx, fixture.path());
            let recording = graph_view::record_graph_paints(window.window_id());
            app.update(cx, |app, cx| {
                app.bottom_panel_open = false;
                app.graph_col_w = 176.;
                cx.notify();
            });
            cx.run_until_parked();
            let (baseline, wips, links, width) = cx.read(|cx| {
                let app = app.read(cx);
                let rows = &app.view().rows;
                assert!(
                    rows.len() > 30 && rows[0].lane_count >= 26,
                    "fixture did not load genuinely wide old history"
                );
                for row in &rows[1..=10] {
                    assert_eq!(row.lane, 0, "local first-parent nodes must stay in lane 0");
                    assert!(
                        row.edges.iter().any(|e| e.from_lane == 1 && e.to_lane == 1),
                        "the real side-parent rail must block lane 1"
                    );
                }
                let links = PAIRS
                    .iter()
                    .enumerate()
                    .map(|(ix, &(top, bottom))| SquashLink {
                        branch: format!("ghost-{ix}"),
                        tip: rows[bottom].id.0.clone(),
                        squash: rows[top].id.0.clone(),
                    })
                    .collect::<Vec<_>>();
                (history(app), anchors(app), links, rows[0].lane_count)
            });
            let before = paint(cx, window);
            assert!(
                before.2.is_empty(),
                "fixture already contains squash annotations"
            );
            let (task, reply) = deferred(cx);
            e2e::queue_squash_scan(task);
            // Mount has settled all initial reads/scans. Do not suppress the
            // product's scans_stale scheduler: this scenario neither republishes
            // nor reactivates a read. Assert no replacement scan masks the queued
            // production callback during routing and scrolling.
            let (scan_gen, owner, revision, publish_gen) = app.update(cx, |app, cx| {
                let owner = app.active_session().unwrap();
                app.start_squash_link_scan(cx);
                (
                    app.squash_gen,
                    owner,
                    app.reads.revision(owner),
                    app.ui.get(&owner).unwrap().view_publish_gen,
                )
            });
            reply.send(Ok(links));
            cx.run_until_parked();
            cx.read(|cx| {
                let app = app.read(cx);
                assert_eq!(
                    history(app),
                    baseline,
                    "squash altered real nodes/parent/WIP edges"
                );
                assert_eq!(anchors(app), wips, "squash altered WIP anchor ownership");
                assert!(
                    app.view().rows.iter().all(|r| r.lane_count == width),
                    "free local routes must not expand the global width"
                );
            });
            let after = paint(cx, window);
            assert_preserved(&before, &after);
            let rails = check_routes(&after);

            // A real horizontal wheel traverses the production graph handler.
            // Keep lane 0 visible so complete endpoint paths remain in the mask.
            let position = point(after.0[0].bounds.center().x, commits(&after)[4].center.y);
            let shift = theme::scaled(4.);
            wheel(cx, window, position, -shift, 0.);
            let horizontal = paint(cx, window);
            let shifted = check_routes(&horizontal);
            for (old, new) in rails.into_iter().zip(shifted) {
                assert!(
                    near(old - new, shift),
                    "horizontal wheel did not translate the actual path"
                );
            }
            // GPUI's vertical-only list also consumes horizontal wheel input.
            // Compare against the actual node displacement, not an assumption
            // that the viewport's vertical offset stays fixed.
            let vertical_shift = y(commits(&horizontal)[0].center) - y(commits(&after)[0].center);
            assert_translated(&after, &horizontal, -shift, vertical_shift);
            wheel(cx, window, position, shift, 0.);
            let restored = paint(cx, window);
            check_routes(&restored);
            assert_translated(&after, &restored, 0., 0.);

            // Scroll only one row: WIP can leave the viewport while every ghost
            // endpoint remains visible. Match translated commands, not indices.
            let nodes = commits(&restored);
            let dy = y(nodes[1].center) - y(nodes[0].center);
            let viewport = e2e::control_bounds(window.window_id(), "commit-list-viewport").unwrap();
            wheel(cx, window, viewport.center(), 0., -dy);
            let vertical = paint(cx, window);
            assert_translated(&restored, &vertical, 0., -dy);
            check_routes(&vertical);
            cx.read(|cx| {
                let app = app.read(cx);
                assert_eq!(
                    app.squash_gen, scan_gen,
                    "routing was masked by a replacement scan"
                );
                assert_eq!(
                    app.reads.revision(owner),
                    revision,
                    "routing replaced the real read"
                );
                assert_eq!(
                    app.ui.get(&owner).unwrap().view_publish_gen,
                    publish_gen,
                    "routing republished the real graph"
                );
                assert_eq!(history(app), baseline, "scrolling changed real history");
                assert_eq!(anchors(app), wips, "scrolling changed WIP ownership");
            });
            drop(recording);
            // A redraw after dropping the guard must not record commands or
            // implicitly re-arm a window merely because its ghosts still paint.
            cx.update_window(window, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear();
            })
            .unwrap();
            assert!(
                graph_view::take_endpoint_paint_trace(window.window_id()).is_empty(),
                "dropping the recording must disarm and clear endpoint observations"
            );
            unmount(cx, app, window);
        }
    }
}
