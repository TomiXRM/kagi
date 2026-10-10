//! #1091 enabled Reply Input priority on the runner's real AppKit window.
use crate::issue_conversation_selection::support::*;
use gpui::VisualTestAppContext;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};

const CONVERSATION: &str = "conversation 日本語🙂\n\ncomment café🧭";
const DRAFT: &str = "draft🙂";

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

pub fn scenario_issue_conversation_enabled_input_copy(cx: &mut VisualTestAppContext) {
    let _restore = Restore::capture();
    let producer = Producer::install();
    producer.publish(
        "a",
        "**conversation 日本語🙂**",
        &[("IC_enabled_input_opaque_comment_1", "**comment café🧭**")],
    );
    let (fixture, app, win, before) = fixture(cx, BASE);
    let repo = fixture.path().canonicalize().unwrap();
    let unchanged = RepositoryState::read(&repo);
    load(cx, &app, win);

    let body = visible(cx, win, BODY).expect("conversation body is physically visible");
    let reply = visible(cx, win, &comment(0)).expect("opaque-ID comment is physically visible");
    down(cx, win, start(body));
    move_to(cx, win, end(reply));
    up(cx, win, end(reply));
    assert_eq!(
        copy(cx, win),
        CONVERSATION,
        "native drag and Cmd-C establish a valid exact Unicode conversation selection"
    );

    // This existing product seam focuses the actual rendered Reply InputState
    // and calls its normal replace/Change path; it neither toggles Focus Editor
    // (which retires the thread) nor changes the conversation's selection.
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.insert_issue_reply_body_for_e2e(4, DRAFT, window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(win, "secondary-a");
    assert_eq!(copy(cx, win), DRAFT, "enabled production Reply Input consumes physical SelectAll/Copy ahead of the still-selected conversation");
    assert_eq!(
        cx.read(|cx| app.read(cx).issue_reply_draft_for_e2e(4).body),
        DRAFT,
        "Copy must not mutate the subscribed Reply draft"
    );

    // SelectAll remains active after Copy. Delete through the real enabled
    // Input action route, not set_value or a direct draft/selection mutation.
    cx.simulate_keystrokes(win, "backspace");
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app.read(cx).issue_reply_draft_for_e2e(4).body),
        "",
        "native editing empties the same enabled Reply"
    );
    assert_eq!(
        copy(cx, win),
        POISON,
        "empty enabled Input consumes Copy without falling back to conversation"
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).issue_reply_draft_for_e2e(4).body),
        "",
        "empty Copy leaves the Reply draft empty"
    );

    // A physical Tab leaves the Input through its production wrapper's focus
    // navigation. Do not focus/activate/reload/reselect the conversation. Copy
    // from this non-input route must still find the ORIGINAL selection: without
    // this witness the poison assertion could pass merely by retiring fallback.
    cx.simulate_keystrokes(win, "tab");
    cx.run_until_parked();
    assert_eq!(copy(cx, win), CONVERSATION, "the original conversation selection survived both enabled Input Copy transitions without revival");
    assert_eq!(
        cx.read(|cx| app.read(cx).issue_reply_draft_for_e2e(4).body),
        "",
        "fallback witness sends no Reply and changes no draft"
    );
    assert_eq!(
        producer.requests(),
        vec!["a"],
        "one actual backend detail read; no send or refresh"
    );

    app.update(cx, |app, cx| app.show_graph_mode(cx));
    cx.run_until_parked();
    assert_eq!(
        copy(cx, win),
        POISON,
        "product departure retires the conversation Copy owner"
    );
    assert_eq!(unchanged, RepositoryState::read(&repo), "Copy/edit/departure preserve HEAD, staged paths/OIDs/modes, working bytes, refs, stash and oplog");
    finish(cx, fixture, app, win, before);
}
