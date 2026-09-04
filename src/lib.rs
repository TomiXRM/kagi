//! kagi library — exposes the graph/remote/update helpers for integration tests.
//!
//! The Git backend lives in the standalone `kagi-git` crate (ADR-0072 / Phase E);
//! reach it via `kagi_git::` rather than `kagi::git::`.

// klog! (the `[kagi]` contract macro) lives in kagi-ui-core; #[macro_use] keeps
// it available unqualified across the lib (the `ui` module below relies on it),
// mirroring `main.rs`.
#[macro_use]
extern crate kagi_ui_core;

pub use kagi_domain::graph; // ADR-0121: was a shim file
pub mod remote;
// ADR-0166: `ui` (and its one crate-root dependency, `single_instance`) live in
// the lib so a `harness = false` main-thread test runner (`tests/gui_e2e_runner`)
// can mount the real `KagiApp` offscreen. The bin (`main.rs`) is a thin wrapper
// that re-imports `kagi::ui` / `kagi::single_instance`. No `gpui/test-support`
// reaches here — the runner owns that (see `ui::e2e`).
pub mod single_instance;
pub mod ui;
pub mod update;

/// Reverse-DNS application id — the single source of truth for how Kagi
/// identifies itself to the OS. On Linux the main window's Wayland `app_id` /
/// X11 `WM_CLASS` is set from this (`ui::open_main_window`); the desktop
/// environment matches it against the installed `com.tomixrm.kagi.desktop`
/// launcher (its id **and** `StartupWMClass`) to give the window the correct
/// icon and taskbar grouping. Without the match, Ubuntu/GNOME (Wayland) spawns
/// the window as a separate generic ("unknown") entry with the fallback gear
/// icon. Also mirrors the macOS bundle id. Every shipped `.desktop` file must
/// keep `StartupWMClass` equal to this — locked by `tests/desktop_integration_test.rs`.
pub const APP_ID: &str = "com.tomixrm.kagi";
