//! #1091 offscreen fallback plus #1479-S3 mounted single-post positive coverage.
//! The old offscreen oracle changes only at the atomic #1479-S6 cutover.
use crate::issue_conversation_selection::support::*;
use gpui::VisualTestAppContext;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};

const SOURCE: &str = "**PARTIAL**\n\n日本語🙂 _café_ **code🧭**";
const RENDERED: &str = "PARTIAL\n日本語🙂 café code🧭";
const PARTIAL: &str = "PARTIAL";

#[derive(Debug, PartialEq, Eq)]
struct RepositoryState {
    head: Vec<u8>,
    staged: Vec<u8>,
    refs: Vec<u8>,
    stash: Vec<u8>,
    working: BTreeMap<PathBuf, Vec<u8>>,
    oplog: Option<Vec<u8>>,
}

impl RepositoryState {
    fn read(repo: &Path) -> Self {
        fn output(repo: &Path, args: &[&str]) -> Vec<u8> {
            let result = Command::new("git")
                .current_dir(repo)
                .args(args)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "read-only git {args:?}: {:?}",
                result.stderr
            );
            result.stdout
        }
        fn files(root: &Path, dir: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path == root.join(".git") {
                    continue;
                }
                if path.is_dir() {
                    files(root, &path, result);
                } else {
                    result.insert(
                        path.strip_prefix(root).unwrap().to_path_buf(),
                        std::fs::read(path).unwrap(),
                    );
                }
            }
        }
        let mut working = BTreeMap::new();
        files(repo, repo, &mut working);
        let log =
            PathBuf::from(std::env::var_os("KAGI_LOG_DIR").expect("runner-owned log directory"))
                .join("operations.jsonl");
        let oplog = match std::fs::read(log) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("read runner oplog: {error}"),
        };
        Self {
            head: output(repo, &["rev-parse", "HEAD"]),
            staged: output(repo, &["ls-files", "--stage", "-z"]),
            refs: output(repo, &["for-each-ref", "--format=%(refname) %(objectname)"]),
            stash: output(repo, &["stash", "list", "--format=%gd %H %gs"]),
            working,
            oplog,
        }
    }
}

pub fn scenario_issue_conversation_offscreen_select_all(cx: &mut VisualTestAppContext) {
    let _restore = Restore::capture();
    let producer = Producer::install();
    let owned: Vec<_> = (0..26)
        .map(|index| {
            (
                format!("IC_offscreen_select_all_opaque_{index}"),
                format!("**OTHER_POST_{index:02}**"),
            )
        })
        .collect();
    let posts: Vec<_> = owned
        .iter()
        .map(|(id, text)| (id.as_str(), text.as_str()))
        .collect();
    producer.publish("a", SOURCE, &posts);
    let (fixture, app, win, before) = fixture(cx, BASE);
    let repo = fixture.path().canonicalize().unwrap();
    let unchanged = RepositoryState::read(&repo);
    load(cx, &app, win);
    let draft = cx.read(|cx| app.read(cx).issue_reply_draft_for_e2e(4));

    // Establish the mounted post's partial → full positive independently of the
    // legacy offscreen fallback below. No injected selection or focus changes.
    assert_eq!(
        select_first_line(cx, win, BODY),
        PARTIAL,
        "physical drag selects exactly the first rendered paragraph, not the whole post"
    );
    visible(cx, win, BODY).expect("partial selection belongs to a freshly painted mounted body");
    cx.simulate_keystrokes(win, "secondary-a");
    assert_eq!(
        copy(cx, win),
        RENDERED,
        "mounted Cmd-A/C copies exactly this post's rendered GFM/Unicode, not source or other posts"
    );
    visible(cx, win, BODY).expect("SelectAll leaves the same body mounted");
    assert_eq!(
        producer.requests(),
        vec!["a"],
        "mounted SelectAll/Copy does not ask the producer for another snapshot"
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).issue_reply_draft_for_e2e(4)),
        draft,
        "mounted post commands leave the complete Reply draft unchanged"
    );
    assert_eq!(
        RepositoryState::read(&repo),
        unchanged,
        "mounted post commands preserve repository and oplog state"
    );

    // Reset with a fresh physical partial drag, not with SDK state mutation.
    // Otherwise an already-full range could make offscreen Cmd-A a vacuous pass.
    assert_eq!(
        select_first_line(cx, win, BODY),
        PARTIAL,
        "offscreen leg starts partial again after the mounted full-copy positive"
    );

    // Bounded physical wheel events only: no focus or model/selection mutation.
    // measure clears the control witness before drawing, so None proves this
    // post is no longer mounted, rather than merely clipped by the viewport.
    for _ in 0..32 {
        wheel(cx, win, -180.);
        if measure(cx, win, BODY).is_none() {
            break;
        }
    }
    assert!(
        measure(cx, win, BODY).is_none(),
        "bounded scrolling must actually unmount the selected TextView"
    );
    assert!(
        (0..26).any(|index| visible(cx, win, &comment(index)).is_some()),
        "the same conversation remains active with another physically visible post"
    );
    assert_eq!(
        copy(cx, win),
        PARTIAL,
        "offscreen Copy still reaches the original partial selection before Cmd-A"
    );

    cx.simulate_keystrokes(win, "secondary-a");
    let copied = copy(cx, win);
    assert_eq!(
        producer.requests(),
        vec!["a"],
        "SelectAll/Copy does not reload, send or replace the accepted source"
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).issue_reply_draft_for_e2e(4)),
        draft,
        "physical post SelectAll/Copy leaves the complete Reply draft unchanged"
    );
    assert_eq!(
        RepositoryState::read(&repo),
        unchanged,
        "SelectAll/Copy preserves HEAD, staged paths/OIDs/modes, working bytes, refs, stash and oplog"
    );
    // Run normal repository assertion/unmount/temp cleanup even when the final
    // consumer byte oracle exposes the frozen SDK's missing fallback handler.
    finish(cx, fixture, app, win, before);
    assert_eq!(
        copied,
        RENDERED,
        "physical Cmd-A/C after post unmount must copy the entire focused post's rendered GFM/Unicode bytes, not PARTIAL, raw Markdown or other posts"
    );
}
