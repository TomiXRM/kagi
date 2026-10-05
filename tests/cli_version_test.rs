//! Version queries must not bootstrap settings, the GUI, or single-instance IPC.
use std::process::Command;

#[test]
fn version_flags_exit_before_bootstrap() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    std::fs::create_dir(&state).unwrap();
    let settings = state.join("settings.json");
    let malformed_settings = b"deliberately malformed: must not be loaded or quarantined";
    std::fs::write(&settings, malformed_settings).unwrap();

    for flag in ["--version", "-V"] {
        let output = Command::new(env!("CARGO_BIN_EXE_kagi"))
            .arg(flag)
            .env_clear()
            .env("HOME", root.path())
            .env("USERPROFILE", root.path())
            .env("KAGI_LOG_DIR", &state)
            .env("KAGI_MENU_DUMP", "1")
            .env("KAGI_OPEN_REPO", root.path().join("missing-repo"))
            .output()
            .expect("run version query");
        assert_eq!(output.status.code(), Some(0), "{flag}: {output:?}");
        assert_eq!(
            output.stdout,
            format!("kagi {}\n", env!("CARGO_PKG_VERSION")).as_bytes(),
            "{flag} must report the compiled package version"
        );
        assert!(output.stderr.is_empty(), "{flag}: {output:?}");
        assert_eq!(std::fs::read(&settings).unwrap(), malformed_settings);
        let entries: Vec<_> = std::fs::read_dir(&state)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, [std::ffi::OsString::from("settings.json")]);
    }
}
