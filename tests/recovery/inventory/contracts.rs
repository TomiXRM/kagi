use gpui::VisualTestAppContext;
use std::process::Command;

fn runner(filter: &str, out: &std::path::Path, matrix: bool) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("runner executable"));
    command
        .env("KAGI_GUI_E2E", "1")
        .env("KAGI_GUI_E2E_ONLY", filter)
        .env_remove("KAGI_GUI_E2E_EXACT")
        .env_remove("KAGI_GUI_E2E_KEEP_GOING")
        .env_remove("KAGI_GUI_E2E_ONSCREEN")
        .env_remove("KAGI_GUI_E2E_VISIBLE")
        .env_remove("KAGI_GUI_E2E_TIMEOUT_SECS")
        .env("KAGI_INVENTORY_OUT", out)
        .env("KAGI_INVENTORY_LANG", if matrix { "en,ja" } else { "en" })
        .env(
            "KAGI_INVENTORY_THEME",
            if matrix { "dark,light" } else { "dark" },
        );
    command
}

pub(super) fn scenario_inventory_selection(_: &mut VisualTestAppContext) {
    let root = tempfile::tempdir().expect("contract output");
    assert_mixed_selection_rejected();
    assert_timeout_recorded();
    let output = runner("inventory:01-RemoteBrowse", root.path(), false)
        .output()
        .expect("launch inventory selection");
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{log}");
    let mut files: Vec<_> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|item| item.unwrap().file_name())
        .collect();
    files.sort();
    assert_eq!(files, ["01-RemoteBrowse-en-dark.png", "index.md"]);
    assert!(
        log.contains("KEEP_GOING 1/1 inventory:01-RemoteBrowse"),
        "{log}"
    );
    let index = std::fs::read_to_string(root.path().join("index.md")).unwrap();
    assert!(index.contains("01-RemoteBrowse-en-dark.png"));
    assert!(!index.contains("02-Clone"));
}

fn assert_mixed_selection_rejected() {
    let root = tempfile::tempdir().expect("mixed selection output");
    let output = runner("inventory:01-RemoteBrowse,bottom_panel", root.path(), false)
        .output()
        .expect("launch mixed selection");
    let log = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{log}");
    assert!(
        log.contains("inventory: selections cannot be mixed with ordinary scenarios"),
        "{log}"
    );
    assert!(
        !log.contains("KEEP_GOING"),
        "mixed selection must not launch children: {log}"
    );
    assert!(
        !root.path().join("index.md").exists(),
        "rejection must precede inventory initialization"
    );
}

fn assert_timeout_recorded() {
    let root = tempfile::tempdir().expect("timeout output");
    let output = runner("inventory:01-RemoteBrowse", root.path(), false)
        .env("KAGI_GUI_E2E_TIMEOUT_SECS", "0")
        .output()
        .expect("launch timed-out inventory");
    let log = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{log}");
    assert!(log.contains("timeout after 0s (killed)"), "{log}");
    let index = std::fs::read_to_string(root.path().join("index.md")).expect("parent index");
    assert!(
        index
            .lines()
            .any(|line| line.starts_with("| inventory:01-RemoteBrowse |")
                && line.contains("| failed: timeout after 0s (killed)")
                && line.ends_with("| — |")),
        "{index}"
    );
    assert!(
        !index.contains("| captured |"),
        "killed capture must not be reported successful"
    );
    assert!(!root.path().join("01-RemoteBrowse-en-dark.png").exists());
}

pub(super) fn scenario_inventory_matrix(_: &mut VisualTestAppContext) {
    let root = tempfile::tempdir().expect("matrix output");
    let output = runner("inventory:01-RemoteBrowse", root.path(), true)
        .output()
        .expect("launch inventory matrix");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut files: Vec<_> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|item| item.unwrap().file_name())
        .collect();
    files.sort();
    assert_eq!(
        files,
        [
            "01-RemoteBrowse-en-dark.png",
            "01-RemoteBrowse-en-light.png",
            "01-RemoteBrowse-ja-dark.png",
            "01-RemoteBrowse-ja-light.png",
            "index.md"
        ]
    );
    for lang in ["en", "ja"] {
        let dark = image::open(root.path().join(format!("01-RemoteBrowse-{lang}-dark.png")))
            .unwrap()
            .to_rgb8();
        let light = image::open(
            root.path()
                .join(format!("01-RemoteBrowse-{lang}-light.png")),
        )
        .unwrap()
        .to_rgb8();
        assert_eq!(dark.dimensions(), light.dimensions());
        let sum = |image: &image::RgbImage| {
            image
                .as_raw()
                .iter()
                .map(|byte| u64::from(*byte))
                .sum::<u64>()
        };
        assert!(
            sum(&light) > sum(&dark),
            "light capture must really render the light theme"
        );
    }
}

pub(super) fn scenario_inventory_existing_runner(_: &mut VisualTestAppContext) {
    let root = tempfile::tempdir().expect("existing runner output");
    let output = runner("bottom_panel", root.path(), false)
        .output()
        .expect("launch ordinary scenarios");
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{log}");
    assert!(log.contains("[gui-e2e] PASS filtered scenarios"), "{log}");
    assert!(
        !log.contains("[gui-e2e] KEEP_GOING"),
        "ordinary scenarios must stay in-process: {log}"
    );
    assert!(
        !log.contains("[inventory]"),
        "ordinary runs must not enter inventory preparation: {log}"
    );
    let windows: Vec<_> = log
        .lines()
        .filter(|line| line.starts_with("[gui-e2e] window options:"))
        .collect();
    assert!(
        !windows.is_empty(),
        "ordinary scenarios must exercise real window construction: {log}"
    );
    assert!(
        windows
            .iter()
            .all(|line| *line == "[gui-e2e] window options: origin=-10000,-10000 visible=false"),
        "ordinary windows must remain offscreen and hidden: {windows:?}"
    );
    assert!(
        !root.path().join("index.md").exists(),
        "ordinary runs must not initialize inventory output"
    );
}
