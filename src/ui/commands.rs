//! W5-MENU / ADR-0029: Command Registry + native macOS menu bar.
//!
//! This module is the **single source of truth** for kagi's command surface.
//! The menu bar (`cx.set_menus`), keyboard shortcuts (`KeyBinding`), and the
//! conditional action handlers on the root element all derive from the same
//! [`Command`] table and the same [`command_state`] function — there is no
//! menu-specific behaviour anywhere else (grep for `menu_handlers` /
//! `register_menu_actions` to find the only wiring site, in `mod.rs::render`).
//!
//! ## Disabled = handler-not-registered (verified against gpui 0.2.2)
//!
//! macOS validates each menu item by calling
//! `validate_menu_command(action)` which gpui implements as
//! `cx.is_action_available(action)` — it walks the focused window's dispatch
//! tree and returns `true` only when an `on_action` handler for that action
//! type is registered (see `gpui-0.2.2/src/platform/app_menu.rs::init_app_menus`
//! and `platform/mac/platform.rs::validate_menu_item`).  Therefore, to grey a
//! menu item out we simply **do not register its handler** when
//! [`command_state`] is not [`CommandState::Enabled`].  No `set_menus` rebuild
//! is required — the dispatch tree is rebuilt every frame, so the menu state
//! tracks app state automatically.  This is exactly the ADR-0029 design and it
//! was confirmed to be the real gpui mac behaviour; the fallback "set_menus
//! rebuild" path was therefore **not** needed.
//!
//! ## Keystrokes
//!
//! `cx.set_menus` is passed the live keymap, so keystrokes registered via
//! [`register_keybindings`] are rendered automatically next to each menu item.
//! Edit-menu items use `MenuItem::os_action` (no global `KeyBinding`) so the
//! standard text-input behaviour of cmd-z/x/c/v/a is never overridden.

use std::time::{Duration, Instant};

use gpui::{
    actions, div, prelude::*, rgb, App, Context, KeyBinding, Menu, MenuItem, MouseButton, OsAction,
    SharedString, Window,
};

use kagi_git::CommitId;

/// Interval between background auto-fetches (when the `auto_fetch` setting is on
/// and a repo is open). Kept conservative to avoid hammering the remote.
const AUTO_FETCH_INTERVAL_SECS: u64 = 180;

#[cfg(feature = "gui-e2e")]
static PANIC_NEXT_FETCH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(feature = "gui-e2e")]
static PANIC_NEXT_SNAPSHOT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(feature = "gui-e2e")]
thread_local! {
    static SNAPSHOT_WRITE_HOLD: std::cell::RefCell<Option<gpui::Task<()>>> =
        const { std::cell::RefCell::new(None) };
}

use super::context_menu::CommitAction;
use super::i18n::{self, Lang, Msg};
use super::settings;
use super::theme::{self, theme};
use super::{BottomTab, FooterStatus, KagiApp, ToastKind, ToggleBottomPanel};

// ──────────────────────────────────────────────────────────────────────────
// Actions — one gpui Action per command (1:1, ADR-0029).
// ──────────────────────────────────────────────────────────────────────────
//
// `ToggleTerminal` is intentionally absent: the Terminal toggle reuses the
// existing `ToggleBottomPanel` action (cmd-j) so there is a single handler.
actions!(
    kagi_menu,
    [
        // kagi (app menu)
        About,
        // T-SETTINGS-001 / ADR-0080: open the Settings window (also cmd-,).
        OpenSettings,
        Quit,
        // File
        NewTab,
        CloseTab,
        CloneRepository,
        OpenRepository,
        OpenInTerminal,
        ConnectRemote,
        RefreshRepository,
        // View
        ZoomIn,
        ZoomOut,
        ZoomReset,
        EnterFullScreen,
        ToggleSidebar,
        TogglePrSwimlane,
        ToggleCommitDetails,
        ToggleEditorDetails,
        ToggleDiffView,
        // T-WS-EDITOR-001: Graph ⇄ Editor workspace mode.
        ToggleEditorWorkspace,
        TogglePrMode,
        ShowGraph,
        // Command palette (issue #352): fuzzy-search every command.
        OpenCommandPalette,
        // Repository
        Fetch,
        Pull,
        Push,
        // ADR-0154 / #335: capture the working tree as a savepoint snapshot.
        CreateSnapshot,
        OpenInFinder,
        // Branch
        NewBranch,
        CheckoutBranch,
        RenameBranch,
        DeleteBranch,
        CopySelectedBranchName,
        // Commit (operate on the selected commit; route via dispatch_commit_action)
        CopyCommitHash,
        CheckoutCommit,
        CreateBranchFromCommit,
        CherryPickCommit,
        RevertCommit,
        ResetToCommit,
        CompareWithWorkingTree,
        // Window
        MinimizeWindow,
        ZoomWindow,
        PreviousTab,
        NextTab,
        BringAllToFront,
        NewWindow,
        CloseWindow,
        // Help
        KeyboardShortcuts,
        Documentation,
        ReportIssue,
        // View → Theme (W9-THEME / ADR-0036): the original fixed theme actions,
        // kept dispatchable by name. The menus now use `SetTheme` (#922).
        ThemeCatppuccin,
        ThemeXcodeDark,
        ThemeXcodeLight,
        ThemeOneDark,
        ThemeOneLight,
        ThemeMonokai,
        ThemeColorVision,
        // View → Language (W22-I18N / ADR-0048): one action per UI language.
        LangEnglish,
        LangJapanese,
        // ADR-0084: repository Undo/Redo, distinct from Input's text actions.
        HistoryUndo,
        HistoryRedo,
    ]
);

#[cfg(target_os = "macos")]
actions!(kagi_menu, [HideApp, HideOtherApps, ShowAllApps]);

/// Shared dispatch for native actions and registry-based palette/dropdown calls.
#[cfg(target_os = "macos")]
fn visibility_command_handler(id: &str) -> Option<fn(&App)> {
    match id {
        "app.hide" => Some(App::hide),
        "app.hideOthers" => Some(App::hide_other_apps),
        "app.showAll" => Some(App::unhide_other_apps),
        _ => None,
    }
}

/// Switch to the registered theme with this slug, built-in or custom (#922).
///
/// The View → Theme menus list every theme in [`theme::themes_by_name`], and
/// custom themes only exist at runtime, so their menu items carry the slug
/// instead of naming one of the fixed `Theme*` actions above. Those stay so a
/// dispatch by name (`kagi_menu::ThemeCatppuccin`, …) keeps working. Not
/// keymap-loadable (`no_json`): a theme is bound by slug, not by keystroke.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = kagi_menu, no_json)]
pub struct SetTheme {
    pub slug: SharedString,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = kagi_menu, no_json)]
pub struct SetToolbarDisplay {
    pub mode: settings::ToolbarDisplay,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = kagi_menu, no_json)]
pub struct OpenRecentRepository {
    pub path: std::path::PathBuf,
}
actions!(kagi_menu, [ClearRecentRepositories, FindInFocusedView]);

/// Keyboard-only routing; explicit menu actions do not require Graph focus.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = kagi_menu, no_json)]
pub(super) struct CopyGraphBranchName;

#[derive(Default)]
struct RecentMenuAvailable(bool);
impl gpui::Global for RecentMenuAvailable {}

pub(super) fn register_recent_clear(el: gpui::Div, cx: &App) -> gpui::Div {
    el.when(
        cx.try_global::<RecentMenuAvailable>()
            .is_some_and(|state| state.0),
        |el| {
            el.on_action(|_: &ClearRecentRepositories, _, cx| {
                super::tabs::clear_recent_repos();
                publish_menus(cx);
                cx.refresh_windows();
            })
        },
    )
}

/// Map a language command id back to its [`Lang`].
pub fn lang_for_command(id: &str) -> Option<Lang> {
    match id {
        "lang.english" => Some(Lang::En),
        "lang.japanese" => Some(Lang::Ja),
        _ => None,
    }
}

/// Map a theme command id back to its theme slug.
pub fn theme_slug_for_command(id: &str) -> Option<&'static str> {
    match id {
        "theme.catppuccin" => Some("catppuccin"),
        // Retired themes — redirected to their Apple successors so an existing
        // keybinding or palette entry still lands somewhere sensible.
        "theme.xcodeDark" => Some("apple-dark"),
        "theme.xcodeLight" => Some("apple-light"),
        "theme.oneDark" => Some("one-dark"),
        "theme.oneLight" => Some("one-light"),
        "theme.monokai" => Some("monokai"),
        "theme.colorVision" => Some("color-vision"),
        _ => None,
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Declarative menu layout — the single source of truth (ADR-0085).
// ──────────────────────────────────────────────────────────────────────────
//
// ADR-0085: the menu *structure* (which command id lives in which section, in
// what order, with which separators / submenus) used to be hand-duplicated in
// two places: `build_menus()` (macOS native `Vec<Menu>`) and `mod.rs`'s
// `PLATFORM_MENUS` (the Linux self-drawn dropdown).  They drifted.  This tree
// is now the *only* canonical layout — both consumers walk `MENU_BAR`, so the
// platforms can never disagree on structure again.  Behaviour/state/labels
// still come from the Command Registry (`COMMANDS` / `command_state`, ADR-0029).

/// A single menu entry.  Pure layout data; the canonical tree is [`MENU_BAR`].
pub enum MenuNode {
    /// A Command Registry id (e.g. `"file.newTab"`).  Label / keystroke / state
    /// are pulled from the registry — never hard-coded here.
    Command(&'static str),
    /// An in-menu divider.
    Separator,
    /// A dynamic submenu whose contents and checked state are built
    /// at render time.  macOS nests a real `Menu`; Linux expands it inline into
    /// the dropdown panel (it has no nested-panel support) — both behaviours are
    /// preserved from before ADR-0085.
    Submenu(DynSubmenu),
    /// Standard Edit entry: focused GPUI Undo/Redo or a native responder-chain
    /// selector. The macOS-only section is skipped by the portable dropdown
    /// (the intentional Edit-menu OS difference — ADR-0085 §4).
    OsEdit(OsEditItem),
    /// The editor's existing Save action, whose dirty-buffer owner is an entity.
    EditorSave,
    #[cfg(target_os = "macos")]
    Services,
}

/// Dynamic View submenus, shared by native and in-app menus (ADR-0085).
pub enum DynSubmenu {
    Theme,
    Language,
    ToolbarDisplay,
    OpenRecent,
    Find,
}

/// Standard macOS Edit entries; Undo/Redo share the focused keyboard actions.
pub enum OsEditItem {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
}

/// One top-level menu section (a head in the macOS menu bar / Linux titlebar).
pub struct MenuSection {
    /// Section head label, e.g. `"File"`.  The first section is the app menu.
    pub label: &'static str,
    /// Ordered entries; separators and submenus are positional.
    pub items: &'static [MenuNode],
    /// macOS-only section. Linux skips it
    /// entirely — see ADR-0085 §4 (the intentional Edit-menu OS difference).
    /// Read by `linux_menu_sections`.
    pub mac_only: bool,
}

/// The canonical menu bar (ADR-0085).  `build_menus()` (macOS) and the Linux
/// dropdown both read this, so structure can never drift between platforms.
/// Order and separator positions mirror the historical `build_menus()` exactly.
pub const MENU_BAR: &[MenuSection] = &[
    // ── Kagi (app menu) ──────────────────────────────────────────────
    MenuSection {
        label: "Kagi",
        mac_only: false,
        items: &[
            MenuNode::Command("app.about"),
            MenuNode::Separator,
            MenuNode::Command("app.settings"),
            MenuNode::Separator,
            #[cfg(target_os = "macos")]
            MenuNode::Services,
            #[cfg(target_os = "macos")]
            MenuNode::Separator,
            #[cfg(target_os = "macos")]
            MenuNode::Command("app.hide"),
            #[cfg(target_os = "macos")]
            MenuNode::Command("app.hideOthers"),
            #[cfg(target_os = "macos")]
            MenuNode::Command("app.showAll"),
            #[cfg(target_os = "macos")]
            MenuNode::Separator,
            MenuNode::Command("app.quit"),
        ],
    },
    // ── File ─────────────────────────────────────────────────────────
    MenuSection {
        label: "File",
        mac_only: false,
        items: &[
            MenuNode::Command("file.newTab"),
            MenuNode::Command("file.closeTab"),
            MenuNode::Separator,
            MenuNode::EditorSave,
            MenuNode::Separator,
            MenuNode::Command("file.cloneRepository"),
            MenuNode::Command("file.openRepository"),
            MenuNode::Submenu(DynSubmenu::OpenRecent),
            MenuNode::Command("file.openInTerminal"),
            MenuNode::Command("file.connectRemote"),
            MenuNode::Separator,
            MenuNode::Command("file.refresh"),
        ],
    },
    // ── Edit (OS-standard; macOS-only, ADR-0085 §4) ──────────────────
    MenuSection {
        label: "Edit",
        mac_only: true,
        items: &[
            MenuNode::OsEdit(OsEditItem::Undo),
            MenuNode::OsEdit(OsEditItem::Redo),
            MenuNode::Separator,
            MenuNode::OsEdit(OsEditItem::Cut),
            MenuNode::OsEdit(OsEditItem::Copy),
            MenuNode::OsEdit(OsEditItem::Paste),
            MenuNode::OsEdit(OsEditItem::SelectAll),
            MenuNode::Separator,
            MenuNode::Submenu(DynSubmenu::Find),
        ],
    },
    // ── View ─────────────────────────────────────────────────────────
    MenuSection {
        label: "View",
        mac_only: false,
        items: &[
            MenuNode::Command("view.zoomIn"),
            MenuNode::Command("view.zoomOut"),
            MenuNode::Command("view.zoomReset"),
            MenuNode::Separator,
            MenuNode::Command("view.fullScreen"),
            MenuNode::Separator,
            MenuNode::Command("view.toggleSidebar"),
            MenuNode::Command("view.togglePrSwimlane"),
            MenuNode::Command("view.toggleTerminal"),
            MenuNode::Command("view.toggleCommitDetails"),
            MenuNode::Command("view.toggleEditorDetails"),
            MenuNode::Command("view.toggleDiffView"),
            MenuNode::Command("view.toggleEditorWorkspace"),
            MenuNode::Command("view.togglePrMode"),
            MenuNode::Command("view.showGraph"),
            MenuNode::Separator,
            // Dynamic selections use native checked state, never title prefixes.
            MenuNode::Submenu(DynSubmenu::Theme),
            MenuNode::Submenu(DynSubmenu::Language),
            MenuNode::Separator,
            MenuNode::Submenu(DynSubmenu::ToolbarDisplay),
        ],
    },
    // ── Repository ───────────────────────────────────────────────────
    MenuSection {
        label: "Repository",
        mac_only: false,
        items: &[
            MenuNode::Command("repo.fetch"),
            MenuNode::Command("repo.pull"),
            MenuNode::Command("repo.push"),
            MenuNode::Separator,
            MenuNode::Command("repo.createSnapshot"),
            MenuNode::Separator,
            MenuNode::Command("repo.openInFinder"),
            MenuNode::Command("file.openInTerminal"),
        ],
    },
    // ── Branch ───────────────────────────────────────────────────────
    MenuSection {
        label: "Branch",
        mac_only: false,
        items: &[
            MenuNode::Command("branch.new"),
            MenuNode::Command("branch.checkout"),
            MenuNode::Command("branch.rename"),
            MenuNode::Command("branch.delete"),
            MenuNode::Command("branch.copySelectedName"),
        ],
    },
    // ── Commit ───────────────────────────────────────────────────────
    MenuSection {
        label: "Commit",
        mac_only: false,
        items: &[
            MenuNode::Command("commit.copyHash"),
            MenuNode::Command("commit.checkout"),
            MenuNode::Command("commit.createBranch"),
            MenuNode::Separator,
            MenuNode::Command("commit.cherryPick"),
            MenuNode::Command("commit.revert"),
            MenuNode::Command("commit.reset"),
            MenuNode::Separator,
            MenuNode::Command("commit.compareWorkingTree"),
        ],
    },
    // ── Window ───────────────────────────────────────────────────────
    MenuSection {
        label: "Window",
        mac_only: false,
        items: &[
            MenuNode::Command("window.minimize"),
            MenuNode::Command("window.zoom"),
            MenuNode::Separator,
            MenuNode::Command("window.previousTab"),
            MenuNode::Command("window.nextTab"),
            MenuNode::Separator,
            MenuNode::Command("window.bringAllToFront"),
            MenuNode::Separator,
            MenuNode::Command("window.new"),
            MenuNode::Command("window.close"),
        ],
    },
    // ── Help ─────────────────────────────────────────────────────────
    MenuSection {
        label: "Help",
        mac_only: false,
        items: &[
            MenuNode::Command("help.shortcuts"),
            MenuNode::Command("help.documentation"),
            MenuNode::Command("help.reportIssue"),
            MenuNode::Separator,
            MenuNode::Command("app.about"),
        ],
    },
];

/// The sections the Linux/FreeBSD self-drawn menu bar renders: [`MENU_BAR`]
/// minus the `mac_only` sections (the Edit menu — ADR-0085 §3/§4).  Heads and
/// the open dropdown both iterate this so their indices (and the dropdown's
/// left offset) stay aligned.
pub fn linux_menu_sections() -> impl Iterator<Item = &'static MenuSection> {
    MENU_BAR.iter().filter(|s| !s.mac_only)
}

/// The ordered language command ids, as they appear under View → Language.
/// Used by the Linux dropdown to inline-expand `DynSubmenu::Language` (macOS
/// nests [`lang_submenu`] instead).  These mirror the registry's `lang.*` ids.
pub const LANG_COMMAND_IDS: &[&str] = &["lang.english", "lang.japanese"];

// ──────────────────────────────────────────────────────────────────────────
// Command registry types (ADR-0029).
// ──────────────────────────────────────────────────────────────────────────

/// A single command in the registry.  Pure data — the behaviour lives in the
/// `on_action` handler wired in `register_menu_actions`, and the enabled/
/// disabled/hidden decision lives in [`command_state`].
#[derive(Clone, Copy, Debug)]
pub struct Command {
    /// Stable dotted id, e.g. `"file.openRepository"`.
    pub id: &'static str,
    /// Human-readable menu label.
    pub label: &'static str,
    /// gpui keystroke notation (macOS `cmd`), or `None` when unbound.
    pub keystroke: Option<&'static str>,
    /// Future red-text / two-step-confirm attribute (ADR-0029).
    pub dangerous: bool,
}

/// Availability of a command (ADR-0029). Binary: available or greyed-out
/// with a reason. (The earlier `Hidden` variant was never used — kagi prefers
/// `Disabled` so the affordance stays visible.)
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandState {
    /// Available — handler is registered, menu item is active.
    Enabled,
    /// Visible but greyed out, with a reason (handler not registered).
    Disabled(&'static str),
}

/// The full command table.  Order is irrelevant (menus are built explicitly in
/// [`build_menus`]); this slice is the canonical id → metadata map used by the
/// `KAGI_MENU_DUMP` verifier and by keystroke lookup.
pub const COMMANDS: &[Command] = &[
    Command {
        id: "app.about",
        label: "About Kagi",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "app.settings",
        label: "Settings…",
        keystroke: Some("secondary-,"),
        dangerous: false,
    },
    Command {
        id: "app.quit",
        label: "Quit kagi",
        keystroke: Some("secondary-q"),
        dangerous: false,
    },
    #[cfg(target_os = "macos")]
    Command {
        id: "app.hide",
        label: "Hide Kagi",
        keystroke: Some("cmd-h"),
        dangerous: false,
    },
    #[cfg(target_os = "macos")]
    Command {
        id: "app.hideOthers",
        label: "Hide Others",
        keystroke: Some("cmd-alt-h"),
        dangerous: false,
    },
    #[cfg(target_os = "macos")]
    Command {
        id: "app.showAll",
        label: "Show All",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "file.newTab",
        label: "New Tab",
        keystroke: Some("secondary-t"),
        dangerous: false,
    },
    Command {
        id: "file.closeTab",
        label: "Close Tab",
        keystroke: Some("secondary-w"),
        dangerous: false,
    },
    Command {
        id: "file.cloneRepository",
        label: "Clone Repository…",
        keystroke: Some("secondary-shift-o"),
        dangerous: false,
    },
    Command {
        id: "file.openRepository",
        label: "Open Repository…",
        keystroke: Some("secondary-o"),
        dangerous: false,
    },
    Command {
        id: "file.openInTerminal",
        label: "Open Repository in Terminal",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "file.connectRemote",
        label: "Connect to Remote Host\u{2026}",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "file.refresh",
        label: "Refresh Repository",
        keystroke: Some("secondary-r"),
        dangerous: false,
    },
    Command {
        id: "view.zoomIn",
        label: "Zoom In",
        keystroke: Some("secondary-="),
        dangerous: false,
    },
    Command {
        id: "view.zoomOut",
        label: "Zoom Out",
        keystroke: Some("secondary--"),
        dangerous: false,
    },
    Command {
        id: "view.zoomReset",
        label: "Actual Size",
        keystroke: Some("secondary-0"),
        dangerous: false,
    },
    Command {
        id: "view.fullScreen",
        label: "Enter Full Screen",
        keystroke: Some("ctrl-cmd-f"),
        dangerous: false,
    },
    Command {
        id: "view.toggleSidebar",
        label: "Toggle Sidebar",
        keystroke: Some("secondary-b"),
        dangerous: false,
    },
    Command {
        id: "view.togglePrSwimlane",
        label: "Toggle PR Graph",
        keystroke: Some("secondary-alt-l"),
        dangerous: false,
    },
    Command {
        id: "view.toggleTerminal",
        label: "Toggle Terminal",
        keystroke: Some("secondary-j"),
        dangerous: false,
    },
    Command {
        id: "view.toggleCommitDetails",
        label: "Toggle Commit Details",
        keystroke: Some("secondary-alt-b"),
        dangerous: false,
    },
    Command {
        id: "view.toggleEditorDetails",
        label: "Toggle Editor Diff/History",
        keystroke: Some("secondary-alt-e"),
        dangerous: false,
    },
    Command {
        id: "view.toggleDiffView",
        label: "Toggle Diff View",
        keystroke: Some("secondary-shift-d"),
        dangerous: false,
    },
    Command {
        id: "view.toggleEditorWorkspace",
        label: "Toggle Editor Workspace",
        keystroke: Some("secondary-shift-e"),
        dangerous: false,
    },
    Command {
        id: "view.togglePrMode",
        label: "Pull Request Mode",
        keystroke: Some("secondary-shift-p"),
        dangerous: false,
    },
    Command {
        id: "view.showGraph",
        label: "Graph Mode",
        keystroke: Some("secondary-shift-g"),
        dangerous: false,
    },
    Command {
        // Issue #352: command palette. Cmd+Shift+P (the usual palette key) is
        // already `view.togglePrMode`, so the nearest free key — Cmd/Ctrl+P — is
        // used (verified free against every `KeyBinding::new`).
        id: "view.commandPalette",
        label: "Command Palette…",
        keystroke: Some("secondary-p"),
        dangerous: false,
    },
    Command {
        // #922: re-read the custom theme files (no watcher). Palette-only.
        id: "view.reloadThemes",
        label: "Reload Themes",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "repo.fetch",
        label: "Fetch",
        keystroke: Some("secondary-shift-f"),
        dangerous: false,
    },
    Command {
        id: "repo.pull",
        label: "Pull",
        keystroke: Some("secondary-shift-l"),
        dangerous: false,
    },
    Command {
        id: "repo.push",
        label: "Push",
        keystroke: Some("secondary-shift-k"),
        dangerous: false,
    },
    Command {
        // ADR-0154 / #335: capture a working-tree savepoint under
        // `refs/kagi/snapshots/`. Non-destructive (only adds a ref).
        id: "repo.createSnapshot",
        label: "Create Snapshot",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "repo.openInFinder",
        label: "Open in Finder",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "branch.new",
        label: "New Branch…",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "branch.checkout",
        label: "Checkout Branch…",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "branch.rename",
        label: "Rename Branch…",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "branch.delete",
        label: "Delete Branch…",
        keystroke: None,
        dangerous: true,
    },
    Command {
        id: "branch.copySelectedName",
        label: "Copy Selected Branch Name",
        keystroke: Some("secondary-shift-c"),
        dangerous: false,
    },
    Command {
        id: "commit.copyHash",
        label: "Copy Commit Hash",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "commit.checkout",
        label: "Checkout Commit",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "commit.createBranch",
        label: "Create Branch from Commit…",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "commit.cherryPick",
        label: "Cherry-pick",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "commit.revert",
        label: "Revert",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "commit.reset",
        label: "Reset HEAD to Commit…",
        keystroke: None,
        dangerous: true,
    },
    Command {
        id: "commit.compareWorkingTree",
        label: "Compare with Working Tree",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "window.minimize",
        label: "Minimize",
        keystroke: Some("secondary-m"),
        dangerous: false,
    },
    Command {
        id: "window.zoom",
        label: "Zoom",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "window.previousTab",
        label: "Show Previous Tab",
        keystroke: Some("secondary-shift-["),
        dangerous: false,
    },
    Command {
        id: "window.nextTab",
        label: "Show Next Tab",
        keystroke: Some("secondary-shift-]"),
        dangerous: false,
    },
    Command {
        id: "window.bringAllToFront",
        label: "Bring All to Front",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "window.new",
        label: "New Window",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "window.close",
        label: "Close Window",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "help.shortcuts",
        label: "Keyboard Shortcuts",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "help.documentation",
        label: "Documentation",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "help.reportIssue",
        label: "Report Issue",
        keystroke: None,
        dangerous: false,
    },
    // The fixed theme commands (W9-THEME / ADR-0036), kept so their ids still
    // dispatch. The menus and palette list every theme at runtime instead (#922).
    Command {
        id: "theme.catppuccin",
        label: "Catppuccin Mocha",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "theme.xcodeDark",
        // The theme was renamed to Apple Dark; the id stays so an existing
        // `keybinding.theme.xcodeDark` override keeps working.
        label: "Apple Dark",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "theme.xcodeLight",
        label: "Apple Light",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "theme.oneDark",
        label: "One Dark",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "theme.oneLight",
        label: "One Light",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "theme.monokai",
        label: "Monokai (Warm Hybrid)",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "theme.colorVision",
        label: "Color Vision (Blue/Orange)",
        keystroke: None,
        dangerous: false,
    },
    // View → Language. Native checked state is applied in `lang_submenu`.
    Command {
        id: "lang.english",
        label: "English",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "lang.japanese",
        label: "日本語",
        keystroke: None,
        dangerous: false,
    },
];

/// Look up a command's metadata by id.
pub fn command(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.id == id)
}

fn selected_branch_name(app: &KagiApp) -> Option<&str> {
    let row = app.view().rows.get(app.ui().selected?)?;
    super::commit_list::graph_branch_name(&row.badges)
}

// ──────────────────────────────────────────────────────────────────────────
// State machine — the one place enabled/disabled/hidden is decided (ADR-0029).
// ──────────────────────────────────────────────────────────────────────────

/// Decide whether a command is enabled / disabled / hidden for the current
/// app state.  This is the **only** place command availability is computed;
/// the menu, the keystrokes, and the `on_action` registration all consult it.
pub fn command_state(app: &KagiApp, id: &str) -> CommandState {
    use CommandState::{Disabled, Enabled};

    // Whether a repository is on screen (a tab is active and Home is not in
    // front of it, ADR-0219). Home = no repo.
    let has_repo = !app.tabs.is_empty() && app.repo_path.is_some() && !app.home_in_front();
    // A background git op is running → block state-changing git commands.
    let busy = app.op_latched();
    // A commit row is currently selected.
    let has_selection = app.ui().selected.is_some();
    // The main diff is open (ADR-0121 B2: a staged headless diff counts too).
    let diff_open = app.ui().main_diff.is_some() || app.pending_headless_diff.is_some();

    match id {
        "file.cloneRepository" if app.has_active_modal() && app.clone_modal().is_none() => {
            Disabled(Msg::FinishDialogFirst.t())
        }
        "view.togglePrSwimlane" if app.pr_mode_visible()
            && app.pr_mode().is_some_and(|m| m.active.is_some()) => Enabled,
        "view.togglePrSwimlane" => Disabled(Msg::NoPrOpen.t()),
        #[cfg(target_os = "macos")]
        "app.hide" | "app.hideOthers" | "app.showAll" => Enabled,
        "window.previousTab" | "window.nextTab" => {
            if app.tabs.len() + usize::from(app.home.is_some()) > 1 {
                Enabled
            } else {
                Disabled(Msg::NoOtherTabs.t())
            }
        }
        // ── Always available ────────────────────────────────────────────
        "app.about"
        | "app.settings"
        | "app.quit"
        | "file.newTab"
        | "file.openRepository"
        | "file.cloneRepository"
        // Remote connect needs no open repo (ADR-0089) — always available.
        | "file.connectRemote"
        // W27-UIPOLISH: zoom is global (rem-size scaling) — always available.
        | "view.zoomIn"
        | "view.zoomOut"
        | "view.zoomReset"
        | "view.fullScreen"
        | "view.toggleSidebar"
        | "view.toggleTerminal"
        // Command palette (issue #352) — always available.
        | "view.commandPalette"
        // #922: custom-theme reload is always available.
        | "view.reloadThemes"
        | "window.minimize"
        | "window.zoom"
        | "window.bringAllToFront"
        | "window.close"
        | "help.shortcuts"
        | "help.documentation"
        | "help.reportIssue"
        // Theme switching is always available (W9-THEME).
        | "theme.catppuccin"
        | "theme.xcodeDark"
        | "theme.xcodeLight"
        | "theme.oneDark"
        | "theme.oneLight"
        | "theme.monokai"
        | "theme.colorVision"
        // Language switching is always available (W22-I18N).
        | "lang.english"
        | "lang.japanese" => Enabled,

        // ── Placeholders (feature not implemented; greyed with a reason) ──
        "window.new" => Disabled(Msg::MultiWindowUnsupported.t()),
        // Reset stays disabled per ADR-0024 (no reset in MVP).
        "commit.reset" => Disabled(Msg::ResetUnimplemented.t()),

        // ── Repo required ────────────────────────────────────────────────
        // Cmd-W closes a document when present, otherwise the guarded host.
        "file.closeTab" => Enabled,
        "file.openInTerminal" | "repo.openInFinder" => {
            if has_repo {
                Enabled
            } else {
                Disabled(Msg::NoRepoOpen.t())
            }
        }
        // Refresh also drives the remote read-only view's re-snapshot (ADR-0089).
        "file.refresh" => {
            // The remote read-only view has no `repo_path` but is a repository
            // on screen — unless Home is in front of it (#927 review).
            if has_repo || (app.remote_view.is_some() && !app.home_in_front()) {
                Enabled
            } else {
                Disabled(Msg::NoRepoOpen.t())
            }
        }

        "branch.copySelectedName" => {
            if !has_repo {
                Disabled(Msg::NoRepoOpen.t())
            } else if app.has_active_modal() {
                Disabled(Msg::FinishDialogFirst.t())
            } else if busy {
                Disabled(Msg::OpInProgress.t())
            } else if !has_selection {
                Disabled(Msg::NoCommitSelected.t())
            } else if selected_branch_name(app).is_none() {
                Disabled(Msg::CmNoSelectedBranch.t())
            } else {
                Enabled
            }
        }

        // ── Repo required + not busy (state-changing git) ────────────────
        "repo.fetch" | "repo.pull" | "repo.push" | "repo.createSnapshot" | "branch.new"
        | "branch.checkout" | "branch.delete" | "branch.rename" => {
            if !has_repo {
                Disabled(Msg::NoRepoOpen.t())
            } else if busy {
                Disabled(Msg::OpInProgress.t())
            } else {
                Enabled
            }
        }

        // ── Commit-scoped (selection required; reset handled above) ───────
        "commit.copyHash"
        | "commit.checkout"
        | "commit.createBranch"
        | "commit.cherryPick"
        | "commit.revert"
        | "commit.compareWorkingTree" => {
            if !has_repo {
                Disabled(Msg::NoRepoOpen.t())
            } else if !has_selection {
                Disabled(Msg::NoCommitSelected.t())
            } else if busy {
                Disabled(Msg::OpInProgress.t())
            } else {
                Enabled
            }
        }

        // ── View toggles tied to view state ──────────────────────────────
        "view.toggleCommitDetails"
        | "view.toggleEditorWorkspace"
        | "view.togglePrMode"
        | "view.showGraph" => {
            if has_repo {
                Enabled
            } else {
                Disabled(Msg::NoRepoOpen.t())
            }
        }
        "view.toggleEditorDetails" => {
            if !has_repo {
                Disabled(Msg::NoRepoOpen.t())
            } else if app.has_active_modal() {
                Disabled(Msg::FinishDialogFirst.t())
            } else if app.workspace_mode() == super::workspace_mode::WorkspaceMode::Editor
                && !app.conflict_body_visible()
            {
                Enabled
            } else {
                Disabled(Msg::EditorDetailsUnavailable.t())
            }
        }
        "view.toggleDiffView" => {
            if diff_open && !app.home_in_front() {
                Enabled
            } else {
                Disabled(Msg::DiffNotOpen.t())
            }
        }

        // Unknown id → disabled defensively.
        _ => Disabled("unknown command"),
    }
}

/// Convenience: is this command currently enabled?
pub(crate) fn is_enabled(app: &KagiApp, id: &str) -> bool {
    matches!(command_state(app, id), CommandState::Enabled)
}

// ──────────────────────────────────────────────────────────────────────────
// Menu construction (gpui native).
// ──────────────────────────────────────────────────────────────────────────

/// Build a `MenuItem::action` for a registry command id, centralising the
/// id → gpui Action pairing in **one** place (ADR-0085 §2).  The label (and the
/// keystroke gpui renders from the keymap) come from the registry, so the menu
/// can never drift from [`COMMANDS`].
///
/// An id present in [`MENU_BAR`] but missing here is a wiring bug; we
/// `debug_assert!` so it's caught in dev, and fall back to a non-dispatching
/// item (`About`, which is always available) so release builds stay usable.
fn action_menu_item(id: &str, state: &super::window_menu::ViewMenuState) -> MenuItem {
    let label: SharedString = super::window_menu::state_label(id, state)
        .unwrap_or_else(|| super::command_palette::command_label(id, i18n::lang()))
        .into();
    match id {
        // kagi (app menu)
        "app.about" => MenuItem::action(label, About),
        "app.settings" => MenuItem::action(label, OpenSettings),
        "app.quit" => MenuItem::action(label, Quit),
        #[cfg(target_os = "macos")]
        "app.hide" => MenuItem::action(
            super::command_palette::command_label(id, i18n::lang()),
            HideApp,
        ),
        #[cfg(target_os = "macos")]
        "app.hideOthers" => MenuItem::action(
            super::command_palette::command_label(id, i18n::lang()),
            HideOtherApps,
        ),
        #[cfg(target_os = "macos")]
        "app.showAll" => MenuItem::action(
            super::command_palette::command_label(id, i18n::lang()),
            ShowAllApps,
        ),
        // File
        "file.newTab" => MenuItem::action(label, NewTab),
        "file.closeTab" => MenuItem::action(label, CloseTab),
        "file.cloneRepository" => MenuItem::action(label, CloneRepository),
        "file.openRepository" => MenuItem::action(label, OpenRepository),
        "file.openInTerminal" => MenuItem::action(label, OpenInTerminal),
        "file.connectRemote" => MenuItem::action(label, ConnectRemote),
        "file.refresh" => MenuItem::action(label, RefreshRepository),
        // View
        "view.zoomIn" => MenuItem::action(label, ZoomIn),
        "view.zoomOut" => MenuItem::action(label, ZoomOut),
        "view.zoomReset" => MenuItem::action(label, ZoomReset),
        "view.fullScreen" => MenuItem::action(label, EnterFullScreen),
        "view.toggleSidebar" => MenuItem::action(label, ToggleSidebar),
        "view.togglePrSwimlane" => MenuItem::action(label, TogglePrSwimlane),
        // Terminal toggle reuses ToggleBottomPanel (cmd-j); registry label is
        // "Toggle Terminal".
        "view.toggleTerminal" => MenuItem::action(label, ToggleBottomPanel),
        "view.toggleCommitDetails" => MenuItem::action(label, ToggleCommitDetails),
        "view.toggleEditorDetails" => MenuItem::action(label, ToggleEditorDetails)
            .checked(super::window_menu::checked(id, state)),
        "view.toggleDiffView" => MenuItem::action(label, ToggleDiffView),
        "view.toggleEditorWorkspace" => MenuItem::action(label, ToggleEditorWorkspace)
            .checked(super::window_menu::checked(id, state)),
        "view.togglePrMode" => {
            MenuItem::action(label, TogglePrMode).checked(super::window_menu::checked(id, state))
        }
        "view.showGraph" => {
            MenuItem::action(label, ShowGraph).checked(super::window_menu::checked(id, state))
        }
        // Repository
        "repo.fetch" => MenuItem::action(label, Fetch),
        "repo.pull" => MenuItem::action(label, Pull),
        "repo.push" => MenuItem::action(label, Push),
        "repo.createSnapshot" => MenuItem::action(label, CreateSnapshot),
        "repo.openInFinder" => MenuItem::action(label, OpenInFinder),
        // Branch
        "branch.new" => MenuItem::action(label, NewBranch),
        "branch.checkout" => MenuItem::action(label, CheckoutBranch),
        "branch.rename" => MenuItem::action(label, RenameBranch),
        "branch.delete" => MenuItem::action(label, DeleteBranch),
        "branch.copySelectedName" => MenuItem::action(label, CopySelectedBranchName),
        // Commit
        "commit.copyHash" => MenuItem::action(label, CopyCommitHash),
        "commit.checkout" => MenuItem::action(label, CheckoutCommit),
        "commit.createBranch" => MenuItem::action(label, CreateBranchFromCommit),
        "commit.cherryPick" => MenuItem::action(label, CherryPickCommit),
        "commit.revert" => MenuItem::action(label, RevertCommit),
        "commit.reset" => MenuItem::action(label, ResetToCommit),
        "commit.compareWorkingTree" => MenuItem::action(label, CompareWithWorkingTree),
        // Window
        "window.minimize" => MenuItem::action(label, MinimizeWindow),
        "window.zoom" => MenuItem::action(label, ZoomWindow),
        "window.previousTab" => MenuItem::action(label, PreviousTab),
        "window.nextTab" => MenuItem::action(label, NextTab),
        "window.bringAllToFront" => MenuItem::action(label, BringAllToFront),
        "window.new" => MenuItem::action(label, NewWindow),
        "window.close" => MenuItem::action(label, CloseWindow),
        // Help
        "help.shortcuts" => MenuItem::action(label, KeyboardShortcuts),
        "help.documentation" => MenuItem::action(label, Documentation),
        "help.reportIssue" => MenuItem::action(label, ReportIssue),
        // Unknown id → caught in dev; release falls back to an always-available
        // item so the menu still builds.
        _ => {
            debug_assert!(false, "action_menu_item: no gpui Action for id {id:?}");
            MenuItem::action(label, About)
        }
    }
}

/// Undo/Redo use the same focused GPUI actions as the keyboard. Other standard
/// Edit commands retain AppKit responder-chain handling.
fn os_edit_menu_item(kind: &OsEditItem, state: &super::window_menu::ViewMenuState) -> MenuItem {
    use gpui_component::input::{Redo, Undo};
    match kind {
        OsEditItem::Undo if state.input_focused => MenuItem::action(Msg::MenuUndo.t(), Undo),
        OsEditItem::Redo if state.input_focused => MenuItem::action(Msg::MenuRedo.t(), Redo),
        OsEditItem::Undo => MenuItem::action(
            history_menu_label(Msg::MenuUndo, state.undo_summary.as_ref()),
            HistoryUndo,
        ),
        OsEditItem::Redo => MenuItem::action(
            history_menu_label(Msg::MenuRedo, state.redo_summary.as_ref()),
            HistoryRedo,
        ),
        OsEditItem::Cut => MenuItem::os_action(Msg::MenuCut.t(), EditCut, OsAction::Cut),
        OsEditItem::Copy => MenuItem::os_action(Msg::MenuCopy.t(), EditCopy, OsAction::Copy),
        OsEditItem::Paste => MenuItem::os_action(Msg::MenuPaste.t(), EditPaste, OsAction::Paste),
        OsEditItem::SelectAll => {
            MenuItem::os_action(Msg::MenuSelectAll.t(), EditSelectAll, OsAction::SelectAll)
        }
    }
}

fn history_menu_label(verb: Msg, summary: Option<&SharedString>) -> SharedString {
    match summary {
        Some(summary) => format!("{} {}", verb.t(), summary).into(),
        None => verb.t().into(),
    }
}

/// Publish the canonical tree, then restore AppKit's role using the canonical
/// section identifier rather than its translated display heading.
pub fn publish_menus(cx: &mut App) {
    let window_section = MENU_BAR
        .iter()
        .position(|section| section.label == "Window")
        .expect("canonical Window menu");
    crate::native_menus::publish(cx, build_menus(cx), window_section);
    let recent = dock_menu_items();
    let available = !recent.is_empty();
    if cx.try_global::<RecentMenuAvailable>().map(|state| state.0) != Some(available) {
        cx.set_global(RecentMenuAvailable(available));
        cx.refresh_windows();
    }
    cx.set_dock_menu(recent);
}

/// Build the shared canonical tree with the active window's presentation labels.
/// Dispatch-tree validation owns availability. Find additionally excludes plain
/// text fields: Kit registers Search even on Inputs that are not searchable.
pub fn build_menus(cx: &App) -> Vec<Menu> {
    let state = super::window_menu::current_state(cx);
    menus_for_state(state)
}

pub fn menus_for_state(state: super::window_menu::ViewMenuState) -> Vec<Menu> {
    MENU_BAR
        .iter()
        .map(|section| {
            let items = section
                .items
                .iter()
                .map(|node| match node {
                    MenuNode::Command(id) => action_menu_item(id, &state),
                    MenuNode::Separator => MenuItem::separator(),
                    MenuNode::Submenu(DynSubmenu::Theme) => MenuItem::submenu(theme_submenu()),
                    MenuNode::Submenu(DynSubmenu::Language) => MenuItem::submenu(lang_submenu()),
                    MenuNode::Submenu(DynSubmenu::OpenRecent) => MenuItem::submenu(recent_menu()),
                    MenuNode::Submenu(DynSubmenu::Find) => MenuItem::submenu(
                        Menu::new(Msg::MenuFind.t()).items([MenuItem::action(
                            Msg::MenuFindOpen.t(),
                            FindInFocusedView,
                        )
                        .disabled(!state.find_sidebar && !state.find_input)]),
                    ),
                    MenuNode::Submenu(DynSubmenu::ToolbarDisplay) => MenuItem::submenu(Menu {
                        name: Msg::SettingsToolbarDisplay.t().into(),
                        items: toolbar_display_menu_entries()
                            .into_iter()
                            .map(|(mode, label, checked)| {
                                MenuItem::action(label, SetToolbarDisplay { mode }).checked(checked)
                            })
                            .collect(),
                        disabled: false,
                    }),
                    MenuNode::OsEdit(kind) => os_edit_menu_item(kind, &state),
                    MenuNode::EditorSave => MenuItem::action(Msg::MenuSave.t(), SaveEditorFile),
                    #[cfg(target_os = "macos")]
                    MenuNode::Services => {
                        MenuItem::os_submenu(Msg::MenuServices.t(), gpui::SystemMenuType::Services)
                    }
                })
                .collect();
            Menu {
                name: i18n::menu_heading(section.label, i18n::lang()).into(),
                items,
                disabled: false,
            }
        })
        .collect()
}

/// File and Dock share Home's history order and the existing repository opener.
pub fn recent_menu() -> Menu {
    let mut items = dock_menu_items();
    let empty = items.is_empty();
    if !empty {
        items.push(MenuItem::separator());
    }
    items.push(MenuItem::action(Msg::MenuClearRecent.t(), ClearRecentRepositories).disabled(empty));
    Menu::new(Msg::MenuOpenRecent.t()).items(items)
}

/// Recent-repository entries published to the Dock.
pub fn dock_menu_items() -> Vec<MenuItem> {
    super::tabs::recent_repos()
        .into_iter()
        .map(|path| {
            MenuItem::action(
                path.to_string_lossy().into_owned(),
                OpenRecentRepository { path },
            )
        })
        .collect()
}

/// Shared localized rows and checked state for both platform menu renderers.
pub fn toolbar_display_menu_entries() -> [(settings::ToolbarDisplay, SharedString, bool); 2] {
    let selected = settings::toolbar_display(&theme().slug);
    [
        (
            settings::ToolbarDisplay::IconAndText,
            Msg::ToolbarIconAndText,
        ),
        (settings::ToolbarDisplay::IconOnly, Msg::ToolbarIconOnly),
    ]
    .map(|(mode, label)| (mode, label.t().into(), selected == mode))
}

/// Build the View → Theme submenu (W9-THEME / ADR-0036, #922).
///
/// One item per registered theme, with selection in the native check column.
/// Rebuild on theme changes to publish the new checked state.
fn theme_submenu() -> Menu {
    let items = theme_menu_entries()
        .into_iter()
        .map(|entry| {
            MenuItem::action(entry.name, SetTheme { slug: entry.slug }).checked(entry.active)
        })
        .collect();
    Menu {
        name: i18n::menu_heading("Theme", i18n::lang()).into(),
        items,
        disabled: false,
    }
}

/// One View → Theme row, shared by the macOS submenu and the Linux dropdown.
pub struct ThemeMenuEntry {
    pub slug: SharedString,
    pub name: SharedString,
    pub active: bool,
}

/// Every registered theme in [`theme::themes_by_name`] order — the order
/// Settings and the command palette use (#921, #922).
pub fn theme_menu_entries() -> Vec<ThemeMenuEntry> {
    let active = theme::theme().key();
    theme::themes_by_name()
        .into_iter()
        .map(|t| ThemeMenuEntry {
            slug: SharedString::from(t.slug.clone()),
            name: SharedString::from(t.display_name().to_string()),
            active: t.key() == active,
        })
        .collect()
}

/// Build the View → Language submenu (W22-I18N / ADR-0048).
///
/// Rebuilt on language changes to publish the new native checked state.
fn lang_submenu() -> Menu {
    let active = i18n::lang();
    let entries = [(Lang::En, "English"), (Lang::Ja, "日本語")];
    let mut items: Vec<MenuItem> = Vec::with_capacity(entries.len());
    for (l, name) in entries {
        let label = SharedString::from(name);
        let item = match l {
            Lang::En => MenuItem::action(label, LangEnglish),
            Lang::Ja => MenuItem::action(label, LangJapanese),
        };
        items.push(item.checked(l == active));
    }
    Menu {
        name: i18n::menu_heading("Language", i18n::lang()).into(),
        items,
        disabled: false,
    }
}

// Cut/Copy/Paste/Select All are real AppKit responder-chain selectors. Undo/Redo
// deliberately use Input or repository actions, not unhandled OS dispatch tags.
actions!(kagi_edit, [EditCut, EditCopy, EditPaste, EditSelectAll]);

// ──────────────────────────────────────────────────────────────────────────
// Keybinding registration.
// ──────────────────────────────────────────────────────────────────────────

/// The keystroke a command is bound to: the user's `settings.json` override
/// (`"keybinding.<command id>": "<gpui keystroke>"`, `""` = unbound) if
/// present, else the registry default. Read once per process — the bindings
/// are registered at startup, so a change takes effect on the next launch.
///
/// Keystroke notation is gpui's: `secondary-shift-b` (`secondary` = Cmd on
/// macOS / Ctrl elsewhere), `alt-`, `ctrl-`, `shift-`; e.g. `"secondary-b"`.
pub fn effective_keystroke(id: &str) -> Option<String> {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static OVERRIDES: OnceLock<HashMap<String, String>> = OnceLock::new();
    let overrides = OVERRIDES.get_or_init(|| {
        COMMANDS
            .iter()
            .filter_map(|c| {
                super::settings::read_setting(&format!("keybinding.{}", c.id))
                    .map(|k| (c.id.to_string(), k.trim().to_string()))
            })
            .collect()
    });
    match overrides.get(id) {
        Some(k) if k.is_empty() => None,
        Some(k) => Some(k.clone()),
        None => command(id).and_then(|c| c.keystroke.map(str::to_string)),
    }
}

/// Register the menu keystrokes as gpui `KeyBinding`s so they (a) actually
/// fire when the root has focus and (b) are rendered next to their menu items.
/// Every binding goes through [`effective_keystroke`], so a `settings.json`
/// `keybinding.*` entry re-maps (or unbinds) it.
///
/// Deliberately **excludes**:
/// - `cmd-j` (already bound by the bottom-panel ticket; reused for
///   Toggle Terminal — re-binding would double it),
/// - all Edit actions (os_action only — must not shadow text-input).
use crate::ui::{
    CloseMainDiff, CommitFirst, CommitLast, CommitPageDown, CommitPageUp, CopyDiffSelection,
    DiffNextFile, DiffPrevFile, PrModeNextPane, PrModePrevPane, SaveEditorFile, TerminalExitFocus,
    TerminalSendShiftTab, TerminalSendTab,
};

/// Every app-level [`KeyBinding`] the real window installs.
///
/// Shared with the GUI E2E harness (`ui::e2e::init_app`), which used to bind
/// only `secondary-j` and `secondary-c` of its own accord. A scenario therefore
/// ran against a *different keymap* than the app: `escape` was bound to nothing,
/// so `CloseMainDiff` — the only route from the Escape key to
/// `cancel_active_modal` — never dispatched, and a modal survived an Esc that
/// closes it for real users. Any keyboard scenario could pass or fail for that
/// reason, so the two keymaps are one function now (#492).
///
/// Registered after `gpui_component::init`, which several of these deliberately
/// outrank — keep the order.
pub(crate) fn bind_app_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "secondary-shift-enter",
        super::issues_composer::FocusIssueEditor,
        Some("IssueComposer"),
    )]);
    cx.bind_keys([KeyBinding::new(
        "secondary-f",
        FindInFocusedView,
        Some("!Terminal && !Input"),
    )]);
    // T-BP-002: register secondary-j (Cmd-J on macOS / Ctrl-J elsewhere) as
    // the toggle key for the bottom panel. context = None means the binding
    // fires regardless of focus context. GUI-CLICK: was `cmd-j`, which on
    // Linux is Super-J — so Ctrl-J never toggled the panel.
    cx.bind_keys([KeyBinding::new("secondary-j", ToggleBottomPanel, None)]);
    // T-UI-003: Esc closes the main diff view (no-op when main_diff is None).
    // Terminal and the built-in Find bar own Escape while their controls focus.
    // A positive owner context keeps this fallback below Input / PopupMenu
    // bindings. A negation-only predicate matches at their deepest context.
    cx.bind_keys([KeyBinding::new(
        "escape",
        CloseMainDiff,
        Some("KagiWorkspace && !Terminal && !SearchPanel"),
    )]);
    cx.bind_keys([KeyBinding::new(
        "secondary-enter",
        super::compact::OpenCommitComposer,
        Some("!Terminal"),
    )]);
    // R1: ⌘C copies the diff line selection. Gated on !Input so a focused
    // text field keeps its own copy; no-ops when nothing is selected.
    cx.bind_keys([KeyBinding::new(
        "secondary-c",
        CopyDiffSelection,
        Some("!Terminal && !Input"),
    )]);
    // T-TERM-INTERACT-001 follow-up: Tab completion in the embedded
    // terminal. Deeper "Terminal" context outranks gpui_component Root's
    // "tab" → focus-cycling binding; handlers live on the terminal
    // wrapper div in render_bottom.rs and write \t / ESC[Z to the PTY.
    cx.bind_keys([
        KeyBinding::new("tab", TerminalSendTab, Some("Terminal")),
        KeyBinding::new("shift-tab", TerminalSendShiftTab, Some("Terminal")),
        KeyBinding::new("ctrl-shift-tab", TerminalExitFocus, Some("Terminal")),
        KeyBinding::new("secondary-shift-j", TerminalExitFocus, Some("Terminal")),
    ]);
    // Arrow keys step through files while the main diff is open
    // (no-ops otherwise; see main_diff_step). Scoped `!Terminal` so up/down
    // reach a focused terminal (shell history), and `!Input` so they reach
    // a focused text field / code editor: these bindings register AFTER
    // gpui_component::init, so at equal context depth they would shadow
    // Input's own MoveUp/MoveDown — the editor cursor stopped moving
    // vertically while left/right (unbound here) still worked
    // (user-reported).
    // Popup menus also own vertical arrows (including ordered conflict adoption).
    cx.bind_keys([
        KeyBinding::new(
            "up",
            DiffPrevFile,
            Some("!Terminal && !Input && !PopupMenu"),
        ),
        KeyBinding::new(
            "down",
            DiffNextFile,
            Some("!Terminal && !Input && !PopupMenu"),
        ),
        KeyBinding::new("home", CommitFirst, Some("!Terminal && !Input")),
        KeyBinding::new("end", CommitLast, Some("!Terminal && !Input")),
        KeyBinding::new("pageup", CommitPageUp, Some("!Terminal && !Input")),
        KeyBinding::new("pagedown", CommitPageDown, Some("!Terminal && !Input")),
        KeyBinding::new("cmd-up", CommitFirst, Some("!Terminal && !Input")),
        KeyBinding::new("cmd-down", CommitLast, Some("!Terminal && !Input")),
        // GitHub Phase 1c: ←/→ cycle PR mode's focused pane. No-op outside
        // PR mode (handler checks), so graph mode keeps ←/→ free.
        KeyBinding::new("left", PrModePrevPane, Some("!Terminal && !Input")),
        KeyBinding::new("right", PrModeNextPane, Some("!Terminal && !Input")),
    ]);
    // #944: ←/→/Home/End inside Kagi's tab lists (Home's switch, the
    // workspace-mode nav). `!Terminal && !Input` above matches at every
    // depth, so these scoped bindings must register after it to outrank it —
    // only while a cell, inside the list's context, holds the focus.
    super::keyboard_nav::bind_keys(cx);
    // PDF body navigation outranks the fallback arrows only while its own
    // viewport holds focus. Tree/history and the PDF toolbar remain separate.
    kagi_ui_editor::bind_keys(cx);
    // #985: the menus' ↑/↓/Home/End outrank the app-wide arrows the same way.
    super::menu_keys::bind_keys(cx);
    super::commit_panel_coauthor::bind_keys(cx);
    // T-WS-EDITOR-002: Cmd-S saves the Editor Workspace's dirty buffer.
    // Input has no Save binding, so this reaches the live editor too; the
    // historical text context excludes it rather than saving a hidden buffer.
    cx.bind_keys([KeyBinding::new(
        "secondary-s",
        SaveEditorFile,
        Some("!EditorSnapshot && !EditorReadOnly && !EditorLoading"),
    )]);
    // ADR-0084: app-level Undo/Redo. Historical text is selectable, not an
    // input, but must never route text undo to repository operation history.
    // A focused text field keeps Input's own Undo/Redo actions; the terminal
    // keeps its own
    // Cmd+Z — the app history move only fires elsewhere (e.g. commit graph).
    // gpui 0.2.2 only accepts `&&`/`||` (single `&` fails to parse).
    cx.bind_keys([
        KeyBinding::new(
            "secondary-z",
            HistoryUndo,
            Some("!Input && !Terminal && !EditorSnapshot && !EditorReadOnly && !EditorLoading && !EditorMarkdownPreview"),
        ),
        KeyBinding::new(
            "secondary-shift-z",
            HistoryRedo,
            Some("!Input && !Terminal && !EditorSnapshot && !EditorReadOnly && !EditorLoading && !EditorMarkdownPreview"),
        ),
    ]);
    // Ctrl+A = Select All in text inputs. gpui-component binds ctrl-a to
    // *both* SelectAll and MoveHome (emacs-style) in the "Input" context,
    // and the later (MoveHome) wins — so on this platform Ctrl+A jumped to
    // line start instead of selecting all. Re-bind it to SelectAll here
    // (registered after gpui_component::init, so it takes precedence).
    // cmd-a (SelectAll) and double-click word-select already work natively.
    cx.bind_keys([KeyBinding::new(
        "ctrl-a",
        gpui_component::input::SelectAll,
        Some("Input"),
    )]);
    #[cfg(target_os = "macos")]
    {
        // Remove only the old code-action candidate. Otherwise, after Input's
        // Escape propagates, ToggleCodeActions consumes Command-Period before
        // its Popover / dialog owner can handle the next matching binding.
        cx.bind_keys([KeyBinding::new(
            "cmd-.",
            gpui::Unbind(gpui::Action::name(&gpui_component::input::ToggleCodeActions).into()),
            Some("Input"),
        )]);
        // Reuse Escape's registered actions AND owner predicates. This keeps
        // child menus / Find / completion ahead of their containing dialog
        // and overrides Input's earlier Command-Period code-action binding.
        // Keymap inspection/loading is public even when a component's action
        // type is private (for example gpui-kit's menu cancellation action).
        let escape = [gpui::Keystroke::parse("escape").expect("valid Escape keystroke")];
        let aliases: Vec<_> = cx
            .key_bindings()
            .borrow()
            .bindings()
            // false means complete; true would mean a multi-key prefix.
            .filter(|binding| binding.match_keystrokes(&escape) == Some(false))
            .map(|binding| {
                KeyBinding::load(
                    "cmd-.",
                    binding.action().boxed_clone(),
                    binding.predicate(),
                    false,
                    None,
                    cx.keyboard_mapper().as_ref(),
                )
                .expect("valid Command-Period keystroke")
            })
            .collect();
        cx.bind_keys(aliases);
        // These native Settings alerts handle Escape as a raw key event,
        // rather than registering an action with the application keymap.
        cx.bind_keys([KeyBinding::new(
            "cmd-.",
            CloseMainDiff,
            Some("KagiCancelDialog"),
        )]);
    }
}

/// Install the application-wide command bindings and native menu definition.
///
/// Both the real application and the GUI E2E harness must use this after
/// `gpui_component::init`: menu accelerators are derived from the live keymap,
/// so omitting either registration makes a harness exercise a different app.
pub(crate) fn setup_app(cx: &mut App) {
    bind_app_keys(cx);
    register_keybindings(cx);
    cx.on_action(|_: &BringAllToFront, cx| super::window_menu::bring_all_to_front(cx));
    cx.on_action(|action: &OpenRecentRepository, cx| {
        let path = action.path.clone();
        // Global listeners can run while their dispatching window is leased.
        cx.defer(move |cx| super::tabs::open_recent_from_app(&path, cx));
    });
    #[cfg(target_os = "macos")]
    {
        cx.bind_keys([
            KeyBinding::new("cmd-h", HideApp, None),
            KeyBinding::new("cmd-alt-h", HideOtherApps, None),
        ]);
        cx.on_action(|_: &HideApp, cx| cx.hide());
        cx.on_action(|_: &HideOtherApps, cx| cx.hide_other_apps());
        cx.on_action(|_: &ShowAllApps, cx| cx.unhide_other_apps());
    }
    publish_menus(cx);
}

pub(super) fn quit_app(cx: &mut App) {
    #[cfg(feature = "gui-e2e")]
    if super::e2e::capture_app_quit(cx) {
        return;
    }
    cx.quit();
}

pub fn register_keybindings(cx: &mut App) {
    // (id, action). The keystroke comes from the registry / settings; a
    // command with no effective keystroke is simply not bound.
    macro_rules! bind {
        ($cx:expr, $( $id:literal => $action:expr ),* $(,)?) => {{
            let mut v: Vec<KeyBinding> = Vec::new();
            $(
                if let Some(ks) = effective_keystroke($id) {
                    v.push(KeyBinding::new(&ks, $action, None));
                }
            )*
            $cx.bind_keys(v);
        }};
    }
    bind!(cx,
        "app.quit" => Quit,
        // T-SETTINGS-001: secondary-, opens Settings (Cmd-, on macOS / Ctrl-, elsewhere).
        "app.settings" => OpenSettings,
        "file.newTab" => NewTab,
        "file.closeTab" => CloseTab,
        "file.cloneRepository" => CloneRepository,
        "file.openRepository" => OpenRepository,
        "file.refresh" => RefreshRepository,
        // W27-UIPOLISH: UI zoom. `secondary-=` is the conventional "zoom in" (so
        // the user doesn't need shift for `+`); `secondary--` zoom out; `0` reset.
        // `secondary` = Cmd on macOS, Ctrl on Linux/Windows (GUI-CLICK: the keys
        // were `cmd-*`, which on Linux is the Super key — so Ctrl-+ never fired).
        "view.zoomIn" => ZoomIn,
        "view.zoomOut" => ZoomOut,
        "view.zoomReset" => ZoomReset,
        "view.fullScreen" => EnterFullScreen,
        "view.toggleSidebar" => ToggleSidebar,
        "view.togglePrSwimlane" => TogglePrSwimlane,
        "view.toggleCommitDetails" => ToggleCommitDetails,
        "view.toggleEditorDetails" => ToggleEditorDetails,
        "view.toggleDiffView" => ToggleDiffView,
        // T-WS-EDITOR-001: Graph ⇄ Editor workspace mode (Cmd-Shift-E on
        // macOS / Ctrl-Shift-E elsewhere) — verified free of collisions
        // against every other `KeyBinding::new` in the app.
        "view.toggleEditorWorkspace" => ToggleEditorWorkspace,
        "view.togglePrMode" => TogglePrMode,
        "view.showGraph" => ShowGraph,
        // Issue #352: command palette (Cmd/Ctrl+P).
        "view.commandPalette" => OpenCommandPalette,
        "repo.fetch" => Fetch,
        "repo.pull" => Pull,
        "repo.push" => Push,
        "window.previousTab" => PreviousTab,
        "window.nextTab" => NextTab,
    );
    // Keyboard copy belongs only to the uncovered Graph's root focus.
    // Explicit menu/palette actions remain available in other workspaces.
    if let Some(ks) = effective_keystroke("branch.copySelectedName") {
        cx.bind_keys([KeyBinding::new(
            &ks,
            CopyGraphBranchName,
            Some("!Terminal && !Input && !Editor && !EditorSnapshot && !KagiMenu"),
        )]);
    }
    // Fixed aliases match native normalization without changing registry display.
    cx.bind_keys([
        KeyBinding::new("secondary-+", ZoomIn, None),
        KeyBinding::new("secondary-{", PreviousTab, None),
        KeyBinding::new("secondary-}", NextTab, None),
        KeyBinding::new("secondary-m", MinimizeWindow, None),
    ]);
}

/// Platform rendering of an effective registry keystroke. macOS shares
/// gpui-kit's native glyph ordering and named keys; in-app menus on other
/// platforms retain their text notation. Native menus derive it from the keymap.
#[cfg(target_os = "macos")]
pub fn display_keystroke(ks: &str) -> String {
    let mut formatted = String::with_capacity(ks.len());
    for part in ks.split_whitespace() {
        let Ok(stroke) = gpui::Keystroke::parse(part) else {
            return ks.to_owned();
        };
        if !formatted.is_empty() {
            formatted.push(' ');
        }
        formatted.push_str(&gpui_component::kbd::Kbd::format(&stroke));
    }
    if formatted.is_empty() {
        ks.to_owned()
    } else {
        formatted
    }
}

#[cfg(not(target_os = "macos"))]
pub fn display_keystroke(ks: &str) -> String {
    display_keystroke_text(ks)
}

// Keep the text projection testable on macOS without shipping unused code there.
#[cfg(any(test, not(target_os = "macos")))]
fn display_keystroke_text(ks: &str) -> String {
    let mut formatted = String::with_capacity(ks.len());
    for stroke in ks.split_whitespace() {
        if !formatted.is_empty() {
            formatted.push(' ');
        }
        append_text_keystroke(stroke, &mut formatted);
    }
    if formatted.is_empty() {
        ks.to_owned()
    } else {
        formatted
    }
}

#[cfg(any(test, not(target_os = "macos")))]
fn append_text_keystroke(ks: &str, formatted: &mut String) {
    // Peel the key (last segment) off the modifiers. The key itself may be "-"
    // (zoom out, written "secondary--"), so handle that trailing case first.
    let (mods_str, key) = if let Some(m) = ks.strip_suffix("--") {
        (m, "-")
    } else if let Some(i) = ks.rfind('-') {
        (&ks[..i], &ks[i + 1..])
    } else {
        ("", ks)
    };

    let start = formatted.len();
    if !mods_str.is_empty() {
        for m in mods_str.split('-') {
            if formatted.len() > start {
                formatted.push('+');
            }
            formatted.push_str(match m {
                "secondary" => "Ctrl",
                "cmd" | "super" | "win" => "Super",
                "ctrl" => "Ctrl",
                "alt" => "Alt",
                "shift" => "Shift",
                "fn" => "Fn",
                other => other,
            });
        }
    }
    if formatted.len() > start {
        formatted.push('+');
    }
    // Uppercase a single-character key (j → J); leave named keys (escape) alone.
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(key), None) => formatted.extend(key.to_uppercase()),
        _ => formatted.push_str(key),
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Headless verification — KAGI_MENU_DUMP=1.
// ──────────────────────────────────────────────────────────────────────────

/// Emit one log line per command with its resolved state.  This is the canonical
/// headless verification surface for the menu (the native menu UI cannot be
/// inspected headlessly).  Format (ADR/ticket §5):
/// `[kagi] menu: <id> label="…" key=<ks|-> state=enabled|disabled(<reason>)`
pub fn dump_menu_states(app: &KagiApp) {
    klog!("menu: dump begin n={}", COMMANDS.len());
    for cmd in COMMANDS {
        let ks = effective_keystroke(cmd.id).unwrap_or_else(|| "-".to_string());
        let state = match command_state(app, cmd.id) {
            CommandState::Enabled => "enabled".to_string(),
            CommandState::Disabled(reason) => format!("disabled({reason})"),
        };
        eprintln!(
            "[kagi] menu: {} label=\"{}\" key={} dangerous={} state={}",
            cmd.id, cmd.label, ks, cmd.dangerous, state
        );
    }
    klog!("menu: dump end");
}

// ──────────────────────────────────────────────────────────────────────────
// Keyboard-shortcuts listing (for the Help → Keyboard Shortcuts modal).
// ──────────────────────────────────────────────────────────────────────────

/// Produce `(label, display keystroke)` pairs for every bound command,
/// using the palette/menu formatter for the Keyboard Shortcuts modal.
pub fn shortcut_listing() -> Vec<(SharedString, SharedString)> {
    COMMANDS
        .iter()
        .filter_map(|c| {
            effective_keystroke(c.id).map(|key| {
                (
                    super::command_palette::command_label(c.id, i18n::lang()).into(),
                    SharedString::from(display_keystroke(&key)),
                )
            })
        })
        .collect()
}

// ──────────────────────────────────────────────────────────────────────────
// Menu-driven overlays (branch picker + info panel).  Collected here so the
// menu's own modal surfaces do not bloat `mod.rs` (ADR-0029 集約).
// ──────────────────────────────────────────────────────────────────────────

/// Which list-picker the menu is currently showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchPickerMode {
    /// Pick a branch to checkout → routes to the existing `open_plan_modal`.
    Checkout,
    /// Pick a branch to delete → routes to the existing delete-branch plan modal.
    Delete,
    /// Pick a branch to rename → routes to the existing rename modal. The
    /// sidebar's context menu has offered this all along; the menu bar used to
    /// grey it out as unimplemented.
    Rename,
}

/// Transient overlay opened from the menu bar.
#[derive(Clone, Debug)]
pub enum MenuOverlay {
    /// Branch picker list (checkout / delete).
    BranchPicker {
        mode: BranchPickerMode,
        branches: std::rc::Rc<[String]>,
        rows: std::rc::Rc<super::branch_picker::PickerRows>,
    },
    /// Read-only Keyboard Shortcuts panel, titled with body lines.
    Info {
        title: SharedString,
        lines: std::rc::Rc<[SharedString]>,
        scroll: gpui::ScrollHandle,
        focus: gpui::FocusHandle,
    },
    /// ADR-0080's portable Settings overlay. macOS uses ADR-0223's independent
    /// native host instead (the E2E portable-host suite still exercises this).
    Settings,
    /// Issue #352: the command palette (fuzzy search over [`COMMANDS`]). The
    /// mutable palette state (search input + highlighted index) lives in
    /// dedicated `KagiApp` fields; this is just the "which overlay" marker.
    CommandPalette,
}

pub(super) const GITHUB_URL: &str = "https://github.com/TomiXRM/kagi";
const ISSUES_URL: &str = "https://github.com/TomiXRM/kagi/issues";

/// The confirmation requested after a successful, admitted upstream fetch.
#[derive(Clone, PartialEq, Eq)]
pub enum PullTarget {
    Current,
    BranchFfOnly(String),
}

/// One fetch operation and the Pull confirmations attached to its frozen owner.
///
/// The flight outlives its tab so closing a tab never cancels the write lease.
/// Detach only prunes that session from `waiters`.
pub struct FetchFlight {
    pub owner: crate::app::SessionId,
    pub visit: u64,
    /// Frozen before dispatch: an earlier visit's failure is recorded even
    /// when the tab is no longer the display owner.
    pub before: kagi_git::StateSummary,
    /// Each waiter keeps its own request time, including when joining an older fetch.
    pub waiters: Vec<(crate::app::SessionId, u64, PullTarget, Instant)>,
    /// Live branch captured before dispatch; same-scope requests may join.
    /// `None` retains the detached/no-upstream ordinary fetch fallback.
    pub upstream_branch: Option<String>,
    /// A later user modal superseded the waiting Pull intent, even if that
    /// modal closed before the fetch finished. A new explicit Pull re-arms it.
    pub pull_confirm_displaced: bool,
    /// Admission superseded the owner's read in flight (`app::admit`), which a
    /// no-op fetch would never replace (ADR-0127): the completion owes it (#851).
    pub superseded_read: bool,
}

/// Why a background fetch failed, kept apart from its display text.
///
/// `TerminationUnknown` means the deadline expired before the child was reaped:
/// the fetch may well have reached the remote. Recording that as `Failed` would
/// invite a retry of something that might have already happened (#646).
#[derive(Clone)]
struct FetchFailure {
    message: String,
    auth_remote: Option<String>,
    termination_unknown: bool,
    /// The local refs the attempt moved (#885). Observed, not assumed: a
    /// fetch writes wherever `remote.<name>.fetch` points, and a mirror-style
    /// refspec (`+refs/heads/*:refs/heads/*`) updates local branches with the
    /// same argv. `Some(empty)` when the repository did not even open.
    ref_moves: Option<Vec<kagi_domain::ref_moves::RefMove>>,
}

impl std::fmt::Display for FetchFailure {
    /// The display text is unchanged from before this type existed, so the
    /// `[kagi]` contract lines and the footer keep their exact wording (#646).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl FetchFailure {
    fn auth_summary(&self) -> Option<String> {
        self.auth_remote.as_deref().map(fetch_auth_summary)
    }

    fn record(
        &mut self,
        app: &mut KagiApp,
        before: kagi_git::StateSummary,
        repo_path: &std::path::Path,
        cx: &mut Context<KagiApp>,
        announce: bool,
        requested_op: &str,
    ) {
        let entry = self.entry(before, repo_path);
        let preview = crate::ui::operations::outcome_text(requested_op, &entry.outcome);
        app.record_op_result(
            entry,
            cx,
            true,
            None,
            Some((ToastKind::Error, preview.into())),
            announce,
            true,
            Some(requested_op),
            false,
            None,
            None,
        );
    }

    fn entry(
        &mut self,
        before: kagi_git::StateSummary,
        repo_path: &std::path::Path,
    ) -> kagi_git::oplog::OpLogEntry {
        let mut entry = kagi_git::oplog::OpLogEntry::new(
            "fetch",
            repo_path.display().to_string(),
            before,
            self.outcome(),
        )
        .with_ref_moves(self.ref_moves.take());
        if self.auth_remote.is_some() {
            entry.technical_detail = Some(self.message.clone());
        }
        entry
    }

    fn outcome(&self) -> kagi_git::oplog::OpOutcome {
        if self.termination_unknown {
            kagi_git::oplog::OpOutcome::Unknown {
                after: kagi_git::StateSummary {
                    head: "unchanged".to_string(),
                    dirty: "unchanged".to_string(),
                },
                evidence: self.message.clone(),
            }
        } else {
            kagi_git::oplog::OpOutcome::Failed {
                error: match self.auth_summary() {
                    Some(summary) => summary,
                    None => i18n::op::op_failed_contract(i18n::Op::Fetch, &self.message),
                },
            }
        }
    }
}

pub(super) fn fetch_auth_summary(remote: &str) -> String {
    Msg::FetchAuthenticationRequired.t().replacen(
        "{}",
        &kagi_domain::text_safety::sanitize_control_bytes(remote),
        1,
    )
}

impl KagiApp {
    /// ADR-0154 / #335: capture the current working tree as a savepoint
    /// snapshot under `refs/kagi/snapshots/`. Non-destructive (only adds a ref),
    /// so it needs no plan/confirm — a menu action + toast is the whole flow.
    /// Restore lives in the oplog panel (#334), which consumes the tested
    /// `Backend::plan_restore_snapshot` / `run(Operation::RestoreSnapshot)` path.
    #[cfg(feature = "gui-e2e")]
    pub fn panic_next_snapshot_for_e2e() {
        PANIC_NEXT_SNAPSHOT.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    #[cfg(feature = "gui-e2e")]
    pub fn hold_next_snapshot_write_for_e2e(hold: gpui::Task<()>) {
        SNAPSHOT_WRITE_HOLD.with(|slot| assert!(slot.borrow_mut().replace(hold).is_none()));
    }

    fn create_snapshot_now(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let Some(guard) = self.reserve_write("snapshot", &repo_path, cx) else {
            return;
        };
        let owner = self.active_session();
        let visit = owner.and_then(|id| self.app_sessions.visit(id));
        let abandonment = guard.abandonment();
        let supervision = abandonment.supervision();
        let bg_path = repo_path.clone();
        #[cfg(feature = "gui-e2e")]
        let hold = SNAPSHOT_WRITE_HOLD.with(|slot| slot.borrow_mut().take());
        let task = cx.background_spawn(async move {
            let _supervised = kagi_git::proc::supervisor::enter(supervision);
            #[cfg(feature = "gui-e2e")]
            if let Some(hold) = hold {
                hold.await;
            }
            #[cfg(feature = "gui-e2e")]
            if PANIC_NEXT_SNAPSHOT.swap(false, std::sync::atomic::Ordering::SeqCst) {
                let _ = std::panic::catch_unwind(|| panic!("injected snapshot task panic"));
                return None;
            }
            let result = kagi_git::Backend::open(&bg_path).and_then(|backend| {
                let entry = backend.create_snapshot("manual snapshot")?;
                // Enforce the generation cap so the ODB does not grow unbounded.
                let _ = backend.prune_snapshots(kagi_git::DEFAULT_SNAPSHOT_CAP);
                Ok(entry)
            });
            guard.complete_git(&result);
            Some(result)
        });
        cx.spawn(async move |this, acx| {
            let result = task
                .fallible()
                .await
                .flatten()
                .unwrap_or_else(|| Err(abandonment.into_unknown()));
            let _ = this.update(acx, |app, cx| {
                app.refresh_write_busy();
                match &result {
                    Ok(entry) => klog!("snapshot: created {}", entry.id),
                    Err(e) => klog!("snapshot: create failed: {}", e),
                }
                let current = owner.is_some_and(|id| {
                    app.active_session() == Some(id) && app.app_sessions.visit(id) == visit
                });
                if !current {
                    if let Err(e) = &result {
                        app.app_notices.push_back(
                            format!("{}: {}: {e}", repo_path.display(), Msg::SnapshotFailed.t())
                                .into(),
                        );
                    }
                }
                for (id, op, path) in app.app_sessions.drain_unaccounted() {
                    app.notice_reconcile_required(id, op, &path);
                }
                app.present_app_notice();
                if !current {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(_) => {
                        app.push_toast(ToastKind::Success, Msg::SnapshotCreated.t(), cx);
                    }
                    Err(e) => {
                        app.push_toast(
                            ToastKind::Error,
                            format!("{}: {e}", Msg::SnapshotFailed.t()),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Route a menu command (by registry id) to its handler.  This is the only
    /// place menu actions do work; the behaviour reuses existing safe paths
    /// (plan → confirm modals, `dispatch_commit_action`, tabs, etc.).
    ///
    /// Handlers assume the command was enabled (gpui only dispatches when the
    /// handler is registered, which `render` does conditionally on
    /// [`command_state`]); each still falls through harmlessly if state changed.
    pub fn handle_menu_command(
        &mut self,
        id: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        klog!("menu: invoke {}", id);
        #[cfg(target_os = "macos")]
        if let Some(handler) = visibility_command_handler(id) {
            handler(cx);
            return;
        }
        match id {
            // ── kagi ────────────────────────────────────────────────
            "app.about" => self.open_about_modal(window, cx),
            // Settings uses the platform's non-modal window or portable overlay.
            "app.settings" => self.open_settings(window, cx),
            "app.quit" => {
                if !self.hold_host_close(super::EditorPendingIntent::Quit, cx) {
                    quit_app(cx);
                }
            }

            // ── File ────────────────────────────────────────────────
            // New Tab is the tab strip's `+`: the Home tab (#923).
            "file.newTab" => self.open_home_tab(window, cx),
            "file.openRepository" => self.pick_repository(window, cx),
            "file.closeTab" => {
                if !self.tabs.is_empty() && self.home.is_some() && self.home_in_front() {
                    self.close_home_tab(cx);
                } else if !self.tabs.is_empty() {
                    self.close_tab(self.active_tab, cx);
                } else if !self.hold_host_close(
                    super::EditorPendingIntent::CloseWindow(window.window_handle()),
                    cx,
                ) {
                    window.remove_window();
                }
            }
            "file.cloneRepository" => self.open_clone_picker(window, cx),
            "file.openInTerminal" => self.menu_open_terminal(window, cx),
            "repo.createSnapshot" => self.create_snapshot_now(cx),
            "file.connectRemote" => self.open_remote_browse(cx),
            "file.refresh" => {
                // ADR-0089 Phase 2b: a remote read-only view re-snapshots over
                // SSH instead of touching a local repo.
                if self.remote_view.is_some() {
                    self.refresh_remote_view(cx);
                } else {
                    // #487: background read; `Refreshed` / `Refresh failed`
                    // feedback is emitted when it lands.
                    self.reload_manual(cx);
                    // Also fetch the remote (quiet) so changes pushed elsewhere
                    // show up — success reloads the graph, failure is silent.
                    self.fetch_async(true, cx);
                }
            }

            // ── View ────────────────────────────────────────────────
            // W27-UIPOLISH: zoom mutates the global rem-size factor; the next
            // render reads `theme::rem_size_px()` and re-applies it. Persisted
            // to settings.json by `set_zoom`.
            "view.zoomIn" => {
                let z = theme::set_zoom(theme::step_zoom(theme::zoom(), true));
                klog!("zoom: {:.2}x", z);
                // The terminal is a pixel-sized PTY grid, not rem-scaled text,
                // so it needs the new zoom pushed in explicitly.
                self.apply_terminal_config(cx);
            }
            "view.zoomOut" => {
                let z = theme::set_zoom(theme::step_zoom(theme::zoom(), false));
                klog!("zoom: {:.2}x", z);
                // The terminal is a pixel-sized PTY grid, not rem-scaled text,
                // so it needs the new zoom pushed in explicitly.
                self.apply_terminal_config(cx);
            }
            "view.zoomReset" => {
                let z = theme::set_zoom(1.0);
                klog!("zoom: {:.2}x", z);
                // The terminal is a pixel-sized PTY grid, not rem-scaled text,
                // so it needs the new zoom pushed in explicitly.
                self.apply_terminal_config(cx);
            }
            "view.fullScreen" => window.toggle_fullscreen(),
            "view.toggleSidebar" => {
                if self.conflict_body_visible() {
                    if let Some(entity) = self.ui().conflict.as_ref() {
                        entity.update(cx, |view, cx| view.toggle_sidebar(window, cx));
                    }
                } else if self.pr_mode_visible()
                    && self.pr_mode().is_some_and(|m| m.active.is_some())
                {
                    self.toggle_pr_companion(super::pr_layout::Companion::Navigator, window);
                } else {
                    self.sidebar.visible = !self.sidebar.visible;
                }
                klog!("menu: sidebar_visible={}", self.sidebar.visible);
            }
            "view.togglePrSwimlane" => {
                self.toggle_pr_companion(super::pr_layout::Companion::Swimlane, window);
            }
            "view.toggleTerminal" => {
                self.toggle_bottom_panel(window, cx);
                klog!("menu: bottom_panel_open={}", self.bottom_panel_open);
                if self.bottom_panel_open {
                    self.bottom_tab = BottomTab::Terminal;
                    self.activate_terminal(window, cx);
                }
            }
            "view.toggleCommitDetails" => {
                self.inspector_visible = !self.inspector_visible;
                klog!("menu: inspector_visible={}", self.inspector_visible);
            }
            "view.toggleEditorDetails"
                if matches!(command_state(self, id), CommandState::Enabled) =>
            {
                if let Some(editor) = self.ui().editor_workspace.clone() {
                    editor.update(cx, |view, cx| view.toggle_details(window, cx));
                }
            }
            "view.toggleDiffView" => {
                if self.ui().main_diff.is_some() {
                    self.close_main_diff();
                }
            }
            // Issue #352: open the command palette.
            "view.commandPalette" => self.open_command_palette(window, cx),
            // #922: re-read the custom theme files (explicit; no watcher).
            "view.reloadThemes" => self.reload_themes(window, cx),
            // ── Workspace modes: Graph | PRs | Editor, mutually exclusive ──
            // Each button names the mode it selects (not what it toggles to)
            // and highlights while active — two morphing toggles could both
            // read "Graph" at once, which is what made this confusing.
            "view.showGraph" => self.show_graph_mode(cx),
            "view.togglePrMode" => self.show_pr_mode(cx),
            "view.toggleEditorWorkspace" => self.show_editor_mode(cx),

            // ── Repository ──────────────────────────────────────────
            "repo.fetch" => self.menu_fetch(cx),
            "repo.pull" => self.open_pull_modal(cx),
            "repo.push" => self.open_push_modal(cx),
            "repo.openInFinder" => self.menu_open_in_finder(),

            // ── Branch ──────────────────────────────────────────────
            "branch.new" => {
                if let Some(id) = self.selected_or_head_commit() {
                    self.open_create_branch_modal(id, cx);
                }
            }
            "branch.checkout" => self.open_branch_picker(BranchPickerMode::Checkout, window, cx),
            "branch.delete" => self.open_branch_picker(BranchPickerMode::Delete, window, cx),
            "branch.rename" => self.open_branch_picker(BranchPickerMode::Rename, window, cx),
            "branch.copySelectedName" => {
                if is_enabled(self, id) {
                    if let Some(name) = selected_branch_name(self).map(str::to_owned) {
                        super::branch_menu::copy_branch_name(self, name, cx);
                    }
                }
            }

            // ── Commit (selected commit → dispatch_commit_action) ────
            "commit.copyHash"
            | "commit.checkout"
            | "commit.createBranch"
            | "commit.cherryPick"
            | "commit.revert"
            | "commit.compareWorkingTree" => {
                self.dispatch_selected_commit(id, window, cx);
            }

            // ── Window ──────────────────────────────────────────────
            "window.minimize" => window.minimize_window(),
            "window.zoom" => window.zoom_window(),
            "window.previousTab" => self.show_adjacent_tab(false, window, cx),
            "window.nextTab" => self.show_adjacent_tab(true, window, cx),
            "window.bringAllToFront" => super::window_menu::bring_all_to_front(cx),
            "window.close" => {
                if !self.hold_host_close(
                    super::EditorPendingIntent::CloseWindow(window.window_handle()),
                    cx,
                ) {
                    window.remove_window();
                }
            }

            // ── Help ────────────────────────────────────────────────
            "help.shortcuts" => self.open_shortcuts_overlay(window, cx),
            "help.documentation" => cx.open_url(GITHUB_URL),
            "help.reportIssue" => cx.open_url(ISSUES_URL),

            // ── View → Theme (W9-THEME / ADR-0036) ──────────────────
            "theme.catppuccin" | "theme.xcodeDark" | "theme.xcodeLight" | "theme.oneDark"
            | "theme.oneLight" | "theme.monokai" | "theme.colorVision" => {
                if let Some(slug) = theme_slug_for_command(id) {
                    self.set_theme(slug, cx);
                }
            }

            // ── View → Language (W22-I18N / ADR-0048) ───────────────
            "lang.english" | "lang.japanese" => {
                if let Some(l) = lang_for_command(id) {
                    self.set_lang(l, cx);
                }
            }

            _ => {}
        }
        cx.notify();
    }

    /// Rebuild terminal config and push it into every retained session.
    pub(crate) fn apply_terminal_config(&mut self, cx: &mut Context<Self>) {
        let config = super::terminal::build_terminal_config();
        self.apply_terminal_config_local(&config, cx);
        let owner = cx.entity_id();
        cx.defer(move |cx| {
            for handle in cx.windows() {
                let _ = handle.update(cx, |_, window, cx| {
                    let Some(app) =
                        super::repository_window(window, cx).filter(|app| app.entity_id() != owner)
                    else {
                        return;
                    };
                    app.update(cx, |app, cx| app.apply_terminal_config_local(&config, cx));
                });
            }
        });
        cx.refresh_windows();
    }

    fn apply_terminal_config_local(
        &mut self,
        config: &gpui_terminal::TerminalConfig,
        cx: &mut Context<Self>,
    ) {
        for session in self
            .ui
            .values()
            .filter_map(|ui| ui.terminal_session.as_ref())
        {
            if let Some(view) = session.view.clone() {
                let cfg = config.clone();
                view.update(cx, |v, vcx| v.update_config(cfg, vcx));
            }
        }
    }

    /// Switch the active colour theme (W9-THEME / ADR-0036).
    ///
    /// 1. Update + persist the active theme (`theme::set_active`).
    /// 2. Live-apply the new palette to any running terminal sessions via
    ///    `TerminalView::update_config`.
    /// 3. Rebuild the menu bar to publish the active theme's checked state.
    /// 4. `cx.notify()` so every `theme()`-reading render path repaints.
    pub fn set_theme(&mut self, slug: &str, cx: &mut Context<Self>) {
        if !theme::set_active(slug) {
            return;
        }
        let t = theme::theme();
        klog!("theme: {} dark={}", t.slug, t.dark);

        // W12-GCADOPT: push the new kagi palette into gpui-component's global
        // ThemeColor so adopted widgets (Input, Tooltip, Scrollbar, Checkbox)
        // follow the switch.  One-way only (kagi → gpui-component).
        theme::sync_gpui_component_theme(cx);

        // Live-apply to running terminal sessions.
        self.apply_terminal_config(cx);

        // Rebuild the menu bar (active marker moved).
        publish_menus(cx);
        cx.refresh_windows();
        cx.notify();
    }

    /// Switch the active UI language (W22-I18N / ADR-0048).
    ///
    /// 1. Update + persist the active language (`i18n::set_lang`).
    /// 2. Rebuild the menu bar to publish the active language's checked state.
    /// 3. Notify the persistent Settings picker as well as this view; its
    ///    `SelectItem::title` resolves localized names at render time.
    pub fn set_lang(&mut self, l: Lang, cx: &mut Context<Self>) {
        i18n::set_lang(l);
        klog!("lang: {}", l.slug());
        publish_menus(cx);
        cx.refresh_windows();
        if let Some(select) = self.theme_select.as_ref() {
            select.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }

    /// Map a `commit.*` menu id to the existing [`CommitAction`] and dispatch it
    /// against the currently-selected commit via `dispatch_commit_action`.
    fn dispatch_selected_commit(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let row = match self.ui().selected {
            Some(r) => r,
            None => return,
        };
        let target = match self
            .view()
            .details
            .get(row)
            .map(|d| CommitId(d.full_sha.to_string()))
        {
            Some(t) => t,
            None => return,
        };
        let action = match id {
            "commit.copyHash" => CommitAction::CopySha,
            "commit.checkout" => CommitAction::CheckoutCommit,
            "commit.createBranch" => CommitAction::CreateBranchHere,
            "commit.cherryPick" => CommitAction::CherryPick,
            "commit.revert" => CommitAction::Revert,
            "commit.compareWorkingTree" => CommitAction::CompareWithWorkingTree,
            _ => return,
        };
        self.dispatch_commit_action(action, target, window, cx);
    }

    /// Toggle the panel from either the native View action / Cmd+J or the
    /// command palette / platform menu. The menu route additionally starts
    /// the terminal when opening; both routes share this focus transition.
    pub(super) fn toggle_bottom_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_settings_for_command(window, cx);
        self.bottom_panel_open = !self.bottom_panel_open;
        if !self.bottom_panel_open {
            self.yield_terminal_focus(window, cx);
        }
        cx.notify();
    }

    /// Open the bottom Terminal panel for the current repo (Repository / File →
    /// Open in Terminal). Reuses the existing terminal-session plumbing.
    fn menu_open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_settings_for_command(window, cx);
        self.bottom_panel_open = true;
        self.bottom_tab = BottomTab::Terminal;
        self.activate_terminal(window, cx);
    }

    /// Open the repository working tree in Finder via `open <path>` (no shell).
    fn menu_open_in_finder(&mut self) {
        let path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        match std::process::Command::new("open").arg(&path).spawn() {
            Ok(_) => {
                klog!("menu: open-in-finder {}", path.display());
                self.status_footer =
                    FooterStatus::Idle(SharedString::from(Msg::OpenedInFinder.t()));
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("open failed: {e}")));
            }
        }
    }

    /// Fetch the current repo's remote (fetch-only; never merges — W5-MENU).
    /// Runs synchronously via the CLI wrapper, then reloads + toasts.
    fn menu_fetch(&mut self, cx: &mut Context<Self>) {
        self.fetch_async(false, cx);
    }

    /// Background fetch of the upstream remote, then reload on success. Runs the
    /// network `git fetch` off the UI thread (`background_spawn`) and applies the
    /// result on the main thread. `silent` suppresses ordinary toast + footer
    /// feedback (auto-fetch), but a current Pull waiter still receives failure.
    /// The graph updates on success via `reload()` or the ref watcher. Never
    /// stacks: a no-op while another fetch is in flight or an operation is busy.
    pub fn fetch_async(&mut self, silent: bool, cx: &mut Context<Self>) {
        let _ = self.fetch_async_for(silent, None, cx);
    }

    /// Inject an executor unwind after admission, before the fetch work begins.
    #[cfg(feature = "gui-e2e")]
    pub fn panic_next_fetch_for_e2e(&mut self) {
        PANIC_NEXT_FETCH.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// [`Self::fetch_async`], plus the tab whose Pull waits for this fetch
    /// before confirming or claiming it is up to date. The launch request and
    /// later same-owner requests all live in the flight's waiter list.
    pub fn fetch_async_for(
        &mut self,
        silent: bool,
        pull_confirm: Option<PullTarget>,
        cx: &mut Context<Self>,
    ) -> bool {
        let request_started = cx.background_executor().now();
        self.refresh_write_busy();
        let requested_session = self.active_session();
        let live_current_branch = || {
            self.ui()
                .repo_session
                .as_ref()
                .and_then(|session| session.backend().current_branch_name())
        };
        let upstream_branch = match pull_confirm.as_ref() {
            Some(PullTarget::BranchFfOnly(branch)) => Some(branch.clone()),
            Some(PullTarget::Current) | None => live_current_branch().filter(|branch| {
                self.ui()
                    .repo_session
                    .as_ref()
                    .is_some_and(|session| session.backend().pull_branch_ref_state(branch).is_ok())
            }),
        };
        let refresh_current_ruleset =
            !matches!(pull_confirm.as_ref(), Some(PullTarget::BranchFfOnly(_)));
        if let Some(flight) = self.fetch_in_flight.as_mut() {
            // A same-owner visit can only join the scope it actually requested.
            if flight.upstream_branch.as_deref() == upstream_branch.as_deref() {
                if let (Some(target), Some(session)) = (pull_confirm, requested_session) {
                    if session == flight.owner {
                        if let Some(visit) = self.app_sessions.visit(session) {
                            if let Some(waiter) =
                                flight
                                    .waiters
                                    .iter_mut()
                                    .find(|(owner, incarnation, _, _)| {
                                        *owner == session && *incarnation == visit
                                    })
                            {
                                waiter.2 = target;
                            } else {
                                flight
                                    .waiters
                                    .push((session, visit, target, request_started));
                            }
                            flight.pull_confirm_displaced = false;
                            klog!("pull-confirm: waiting on the fetch already in flight");
                            return true;
                        }
                    }
                }
            }
            return false;
        }
        if silent && self.op_latched() {
            return false;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return false,
        };
        let Some(owner) = self.active_session() else {
            return false;
        };
        // ADR-0204 決定 7: a quiet fetch would move remote-tracking refs in
        // the middle of this tab's queue, changing what the next replan sees.
        if silent && !self.op_queue.queue.auto_fetch_allowed(owner) {
            klog!("auto-fetch: skipped while operations are queued");
            return false;
        }
        let Some(visit) = self
            .app_sessions
            .attachment(owner)
            .map(|attachment| attachment.visit)
        else {
            return false;
        };
        // Before admission: e.g. Cmd+R's read, about to be superseded (#851).
        let superseded_read = self.reads.is_loading(owner);
        // A quiet fetch is background work, never this tab's `&&` predecessor.
        let queue_owner = (!silent).then_some(owner);
        let Some(lease) = self.reserve_write_for("fetch", &repo_path, queue_owner, cx) else {
            return false;
        };
        let before = kagi_git::StateSummary {
            head: format!("branch: {}", self.view().status_summary.branch),
            dirty: "unchanged".to_string(),
        };
        if pull_confirm.is_some() {
            // A newly admitted request may share the runner's frozen clock
            // with a completed one; joining the same live waiter above does
            // not reset dismissal. Unrelated fetches never erase its owner.
            self.start_busy_request(super::busy::BusyIdentity::pull((
                owner,
                visit,
                request_started,
            )));
        }
        self.fetch_in_flight = Some(FetchFlight {
            owner,
            visit,
            before,
            upstream_branch,
            waiters: pull_confirm
                .map(|target| (owner, visit, target, request_started))
                .into_iter()
                .collect(),
            pull_confirm_displaced: false,
            superseded_read,
        });
        let repo_path_guard = repo_path.clone();
        if !silent {
            self.refresh_spin_started = Some(Instant::now());
            klog!("fetch: start");
        }
        let abandonment = lease.abandonment();
        let supervision = abandonment.supervision();
        #[cfg(feature = "gui-e2e")]
        let hold = super::busy::take_fetch_hold();
        let upstream_branch = self
            .fetch_in_flight
            .as_ref()
            .and_then(|flight| flight.upstream_branch.clone());
        let result_started = cx.background_executor().now();
        let task = cx.background_spawn(async move {
            let _supervised = kagi_git::proc::supervisor::enter(supervision);
            #[cfg(feature = "gui-e2e")]
            if let Some(hold) = hold {
                hold.await;
            }
            #[cfg(feature = "gui-e2e")]
            if PANIC_NEXT_FETCH.swap(false, std::sync::atomic::Ordering::SeqCst) {
                // GPUI's test dispatcher propagates uncaught task panics.
                // Catch the injected work panic and return an abandoned task
                // result; production's fallible() catches real unwinds.
                let _ = std::panic::catch_unwind(|| panic!("injected fetch worker panic"));
                return None;
            }
            let (result, ref_moves, open_failed) = match kagi_git::Backend::open(&repo_path) {
                // Nothing ran: nothing moved.
                Err(error) => (Err(error), Some(Vec::new()), true),
                Ok(backend) => {
                    let (result, moves) =
                        backend.observe_ref_moves(|b| match upstream_branch.as_deref() {
                            Some(branch) => {
                                let result = b.fetch_branch_upstream(branch);
                                // Preserve ordinary fetch's existing successful
                                // ruleset refresh when using the exact config route.
                                if result.is_ok() && refresh_current_ruleset {
                                    if let Some(current) = b.current_branch_name() {
                                        let _ = b.refresh_ruleset(&current);
                                    }
                                }
                                result
                            }
                            None => b.fetch_remote(),
                        });
                    (result, moves, false)
                }
            };
            lease.complete_git(&result);
            // Keep *why* it failed, not just the text. A deadline that expired
            // is not proof the fetch did not happen, so it must reach the oplog
            // as `Unknown` and never as a retryable `Failed` (ADR-0177). The
            // previous `format!` flattened both into a string here, which is
            // why the distinction could not be made at the call site (#646).
            Some(match result {
                Ok(outcome) => Ok((outcome, ref_moves)),
                Err(error) => Err(FetchFailure {
                    auth_remote: match &error {
                        kagi_git::GitError::FetchAuthentication { remote, .. } => {
                            Some(remote.clone())
                        }
                        _ => None,
                    },
                    termination_unknown: matches!(error, kagi_git::GitError::TerminationUnknown(_)),
                    ref_moves,
                    message: if open_failed {
                        format!("repo open error: {error}")
                    } else {
                        format!("{error}")
                    },
                }),
            })
        });
        cx.spawn(async move |this, acx| {
            let mut result = task.fallible().await.flatten().unwrap_or_else(|| {
                let error = abandonment.into_unknown();
                Err(FetchFailure {
                    message: error.to_string(),
                    auth_remote: None,
                    termination_unknown: true,
                    ref_moves: None,
                })
            });
            let _ = this.update(acx, |app, cx| {
                // Retirement may run after taking the flight but before a
                // confirmation exists. Carry this waiter's presentation only
                // across that synchronous ownership gap.
                let hidden_progress = app.hidden_busy_job;
                let Some(flight) = app.fetch_in_flight.take() else {
                    return;
                };
                let long_running = cx
                    .background_executor()
                    .now()
                    .saturating_duration_since(result_started)
                    >= std::time::Duration::from_secs(2);
                let requested_op = if flight.waiters.is_empty() {
                    "fetch"
                } else {
                    "pull"
                };
                app.poll_app_jobs(cx);
                app.present_app_notice();
                let mut recording_failed = false;
                if let Ok((outcome, ref_moves)) = &mut result {
                    if outcome.changed {
                        // Fetch changes tracking refs even when its owner departs.
                        // Persist once, without adding a second success toast or
                        // presenting a quiet auto-fetch on the active tab.
                        let recording = kagi_git::backend::recording::finalize(
                            kagi_git::oplog::OpLogEntry::new(
                                "fetch",
                                repo_path_guard.display().to_string(),
                                flight.before.clone(),
                                kagi_git::oplog::OpOutcome::Success {
                                    after: flight.before.clone(),
                                },
                            )
                            .with_ref_moves(ref_moves.take()),
                        );
                        recording_failed =
                            app.present_operation_completion(&recording, false, false, None, cx);
                    }
                }
                // A visit is a stay in the same tab, not merely its session.
                // The flight owns one receipt; only waiters on the current
                // visit may receive a result from an earlier visit's flight.
                let current_request_started =
                    flight
                        .waiters
                        .iter()
                        .find_map(|(session, visit, _, started)| {
                            (app.active_session() == Some(*session)
                                && app.app_sessions.visit(*session) == Some(*visit))
                            .then_some(*started)
                        });
                let current_waiter = current_request_started.is_some();
                let announce = app.operation_result_toast_needed(current_request_started.map_or(
                    long_running,
                    |started| {
                        cx.background_executor()
                            .now()
                            .saturating_duration_since(started)
                            >= std::time::Duration::from_secs(2)
                    },
                ));
                if app.active_session() != Some(flight.owner)
                    || app.app_sessions.attachment(flight.owner).map(|a| a.visit)
                        != Some(flight.visit)
                {
                    // The auto-fetch contract lines are emitted whatever the
                    // visit: only the presentation is withheld (#992 review).
                    if silent {
                        match &result {
                            Ok((outcome, _)) => klog!("auto-fetch: ok remote={}", outcome.remote),
                            Err(e) => klog!("auto-fetch: failed (silent): {e}"),
                        }
                    }
                    match &mut result {
                        Err(failure) => {
                            if current_waiter {
                                failure.record(
                                    app,
                                    flight.before,
                                    &repo_path_guard,
                                    cx,
                                    announce,
                                    requested_op,
                                );
                            } else {
                                let recording = app.record_ref_fetch_failure(
                                    "fetch",
                                    Some(flight.owner),
                                    Some(flight.visit),
                                    flight.before,
                                    failure.outcome(),
                                    failure.ref_moves.take(),
                                    failure
                                        .auth_remote
                                        .as_ref()
                                        .map(|_| failure.message.clone()),
                                    &repo_path_guard,
                                    cx,
                                );
                                // The frozen owner is away, so leave the live
                                // footer/panes alone and offer its durable result.
                                // A Pull waiter owns the requested name; a quiet
                                // auto-fetch with no waiter has no notification.
                                if (!silent || !flight.waiters.is_empty())
                                    && !matches!(
                                        recording,
                                        kagi_git::backend::recording::Recording::Failed { .. }
                                    )
                                {
                                    let entry = recording.entry();
                                    let _ = app.push_operation_result(
                                        entry,
                                        requested_op,
                                        ToastKind::Error,
                                        crate::ui::operations::outcome_text(
                                            requested_op,
                                            &entry.outcome,
                                        )
                                        .into(),
                                        cx,
                                    );
                                }
                            }
                        }
                        Ok((outcome, _)) if current_waiter => {
                            for (session, visit, target, request_started) in flight.waiters {
                                if app.active_session() == Some(session)
                                    && app.app_sessions.visit(session) == Some(visit)
                                {
                                    app.deliver_pull_confirm(
                                        (session, visit, request_started),
                                        flight.pull_confirm_displaced,
                                        target,
                                        &outcome.remote,
                                        hidden_progress
                                            == Some(super::busy::BusyIdentity::pull((
                                                session,
                                                visit,
                                                request_started,
                                            ))),
                                        cx,
                                    );
                                }
                            }
                        }
                        Ok(_) => {}
                    }
                    // A delivered Pull confirmation (or failure preview) must
                    // be drawn now, not on the next unrelated frame.
                    cx.notify();
                    return;
                }
                let mut fetched_remote = None;
                if flight.superseded_read && !result.as_ref().is_ok_and(|(o, _)| o.changed) {
                    app.reload(cx);
                }
                if recording_failed && !silent {
                    app.status_footer = FooterStatus::Idle("".into());
                }
                match result {
                    Ok((outcome, _)) => {
                        // ADR-0127: a no-op fetch skips the reload below, so the
                        // snapshot-derived fetch timestamp would go stale and the
                        // age indicator would falsely warn — stamp it in place.
                        app.view_mut().status_summary.last_fetch_secs =
                            Some(super::commit_list::now_unix_secs());
                        // Only reload when the fetch actually moved a ref. A no-op
                        // fetch (the common auto-fetch case) used to re-snapshot the
                        // whole graph AND close + discard the HEAD-versioned overlays
                        // (Analyze / File History) every interval for nothing.
                        if outcome.changed {
                            app.reload(cx);
                        }
                        if silent {
                            klog!("auto-fetch: ok remote={}", outcome.remote);
                        } else if flight.waiters.is_empty() && !recording_failed {
                            let text = SharedString::from(
                                i18n::op::user_text::operations::completed("fetch"),
                            );
                            if app.operation_result_toast_needed(long_running) {
                                if let Some(stack) = app.toast_stack.clone() {
                                    let key =
                                        format!("{}\0fetch\0success", repo_path_guard.display());
                                    stack.update(cx, |stack, cx| {
                                        stack.push_keyed_notify(
                                            ToastKind::Success,
                                            text,
                                            Some(key),
                                            cx,
                                        );
                                    });
                                }
                                app.status_footer = FooterStatus::Idle("".into());
                            } else {
                                app.status_footer = FooterStatus::Success(text);
                            }
                        }
                        fetched_remote = Some(outcome.remote);
                    }
                    Err(mut e) => {
                        // Record even a silent auto-fetch failure. A stopped
                        // child with an uncertain effect is Unknown, not Failed.
                        if !silent || current_waiter {
                            // The receipt presenter owns the single failure
                            // toast and footer, including joined Pull waiters.
                            e.record(
                                app,
                                flight.before,
                                &repo_path_guard,
                                cx,
                                announce,
                                requested_op,
                            );
                        } else {
                            // No presentation owner: preserve the receipt and
                            // recording-failure notice without changing the UI.
                            app.record_ref_fetch_failure(
                                "fetch",
                                None,
                                None,
                                flight.before,
                                e.outcome(),
                                e.ref_moves.take(),
                                e.auth_remote.as_ref().map(|_| e.message.clone()),
                                &repo_path_guard,
                                cx,
                            );
                        }
                        if silent {
                            klog!("auto-fetch: failed (silent): {e}");
                        }
                    }
                }
                // Failed fetches already own one durable receipt. A waiter
                // only offers a confirmation after a successful fetch.
                if let Some(fetched_remote) = fetched_remote {
                    for (session, visit, target, request_started) in flight.waiters {
                        if app.active_session() == Some(session)
                            && app.app_sessions.visit(session) == Some(visit)
                        {
                            app.deliver_pull_confirm(
                                (session, visit, request_started),
                                flight.pull_confirm_displaced,
                                target,
                                &fetched_remote,
                                hidden_progress
                                    == Some(super::busy::BusyIdentity::pull((
                                        session,
                                        visit,
                                        request_started,
                                    ))),
                                cx,
                            );
                        }
                    }
                }
                cx.notify();
            });
            #[cfg(feature = "gui-e2e")]
            super::e2e::mark_fetch_completion_returned();
        })
        .detach();
        // The request now belongs to the flight above: this fetch will deliver it.
        true
    }

    /// Lazily spawn the periodic background auto-fetch ticker (called from
    /// `render`). Runs only while the `auto_fetch` setting is on and a repo is
    /// open. Each interval it triggers a silent `fetch_async` (itself a no-op when
    /// busy / offline / already fetching); the task exits if auto-fetch is turned
    /// off. Mirrors the toast ticker's lazy-spawn pattern.
    pub fn ensure_auto_fetch_ticker(&mut self, cx: &mut Context<Self>) {
        if self.auto_fetch_ticker_alive || !theme::auto_fetch() || self.repo_path.is_none() {
            return;
        }
        self.auto_fetch_ticker_alive = true;
        eprintln!(
            "[kagi] auto-fetch: ticker start ({}s)",
            AUTO_FETCH_INTERVAL_SECS
        );
        cx.spawn(async move |this, acx| loop {
            acx.background_executor()
                .timer(Duration::from_secs(AUTO_FETCH_INTERVAL_SECS))
                .await;
            let keep = this.update(acx, |app, cx| {
                if !theme::auto_fetch() {
                    app.auto_fetch_ticker_alive = false;
                    return false;
                }
                app.fetch_async(true, cx);
                true
            });
            match keep {
                Ok(true) => {}
                Ok(false) | Err(_) => break,
            }
        })
        .detach();
    }

    /// Open the branch picker overlay listing local branches.
    fn open_branch_picker(&mut self, mode: BranchPickerMode, window: &Window, cx: &Context<Self>) {
        let branches: std::rc::Rc<[String]> = self
            .view()
            .branches
            .iter()
            .map(|(n, _)| n.clone())
            .collect();
        eprintln!(
            "[kagi] menu: branch-picker mode={:?} n={}",
            mode,
            branches.len()
        );
        self.leave_settings_for_overlay();
        self.capture_overlay_return_focus(window, cx);
        let rows = std::rc::Rc::new(super::branch_picker::PickerRows::new(branches.len(), cx));
        self.menu_overlay = Some(MenuOverlay::BranchPicker {
            mode,
            branches,
            rows,
        });
    }

    /// Build the Keyboard Shortcuts overlay from the registry (auto-generated).
    fn open_shortcuts_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_settings_for_overlay();
        self.capture_overlay_return_focus(window, cx);
        let mut lines: Vec<SharedString> = shortcut_listing()
            .into_iter()
            .map(|(label, key)| SharedString::from(format!("{key}    {label}")))
            .collect();
        // Tell the user these are re-mappable, and how (settings.json is the
        // one config surface kagi has; no in-app editor for this yet).
        lines.push(SharedString::from(""));
        lines.push(SharedString::from(Msg::ShortcutsRemapHelp.t()));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.menu_overlay = Some(MenuOverlay::Info {
            title: super::command_palette::command_label("help.shortcuts", i18n::lang()).into(),
            lines: lines.into(),
            scroll: gpui::ScrollHandle::new(),
            focus,
        });
    }

    /// Render the active menu overlay, if any (returns `None` when closed).
    pub fn render_menu_overlay(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        match self.menu_overlay.clone()? {
            MenuOverlay::BranchPicker {
                mode,
                branches,
                rows,
            } => Some(self.render_branch_picker(mode, branches, rows, window, cx)),
            MenuOverlay::Info {
                title,
                lines,
                scroll,
                focus,
            } => Some(self.render_info_overlay(title, lines, (&scroll, &focus), window, cx)),
            // T-SETTINGS-001: the OpenLogi-style Settings window (the field
            // closures route apply/persist through the live `KagiApp` entity).
            MenuOverlay::Settings => Some(super::settings_view::render_settings_overlay(
                cx.entity(),
                self.theme_select.clone(),
                self.settings_switches.clone(),
                self.settings_focus.clone(),
                self.smart_commit.clone(),
                self.analyze_ignore_input.clone(),
                &self.settings_controls,
                self.analyze_ignore_dirty(cx),
                self.analyze_reset_alert().cloned(),
                self.settings_scroll.clone(),
                window,
                cx,
            )),
            // Issue #352: command palette.
            MenuOverlay::CommandPalette => Some(self.render_command_palette(window, cx)),
        }
    }

    fn render_info_overlay(
        &self,
        title: SharedString,
        lines: std::rc::Rc<[SharedString]>,
        navigation: (&gpui::ScrollHandle, &gpui::FocusHandle),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (scroll, focus) = navigation;
        let gutter = theme::scaled_px(16.);
        let viewport = window.viewport_size();
        let mut body = super::operations::oplog_restore_inspection::read_only_scroll_body(
            scroll,
            focus,
            title.clone(),
        )
        .gap_0()
        // Keep the last line clear of the body's fixed focus-border band.
        .pb(gpui::px(super::keyboard_nav::RING))
        .child(super::e2e::measure_inside("shortcuts-body"));
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.iter().enumerate() {
            body = body.child(
                div()
                    .relative()
                    .flex_shrink_0()
                    .when(index == last, |row| {
                        row.child(super::e2e::measure_inside("shortcuts-tail"))
                    })
                    .px_4()
                    .py(theme::scaled_px(3.))
                    .text_sm()
                    .text_color(rgb(theme().text_sub))
                    .child(line.clone()),
            );
        }
        let panel = div()
            .relative()
            .child(super::e2e::measure_inside("shortcuts-panel"))
            .w(theme::scaled_px(420.).min(viewport.width - gutter * 2.))
            .h(theme::scaled_px(480.).min(viewport.height - gutter * 2.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(theme::scaled_px(8.))
            .border_1()
            .border_color(rgb(theme().elevated_border(theme().selected)))
            .bg(rgb(theme().elevated_surface(theme().panel)))
            .shadow_lg()
            .child(self.render_info_header(title, cx))
            .child(super::render_helpers::with_system_vertical_scrollbar(
                "shortcuts-scrollbar",
                scroll,
                body,
                cx,
            ));
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(self.wrap_overlay(panel.into_any_element(), cx))
            .into_any_element()
    }

    fn render_info_header(&self, title: SharedString, cx: &mut Context<Self>) -> gpui::Div {
        use gpui_component::button::{Button, ButtonVariants as _};
        use gpui_component::Sizable as _;
        div()
            .relative()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(rgb(theme().selected))
            .text_color(rgb(theme().color_branch))
            .child(super::e2e::measure_inside("shortcuts-header"))
            .child(div().min_w_0().child(title))
            .child(super::e2e::measure_control(
                "shortcuts-dismiss",
                Button::new("shortcuts-dismiss")
                    .label(Msg::AppNoticeDismiss.t())
                    .ghost()
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.menu_overlay = None;
                        cx.notify();
                    })),
            ))
    }

    /// Centre an overlay panel over a dim, click-to-dismiss backdrop.
    pub(super) fn wrap_overlay(
        &self,
        panel: gpui::AnyElement,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let dismiss = cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
            this.menu_overlay = None;
            cx.stop_propagation();
            cx.notify();
        });
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(rgb(theme().bg_base))
                    .opacity(0.55)
                    .on_mouse_down(MouseButton::Left, dismiss),
            )
            .child(panel)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR-0085: guard against a typo'd id in the canonical [`MENU_BAR`].  Every
    /// `MenuNode::Command(id)` must resolve in the Command Registry, otherwise
    /// the menu would show a raw id (macOS) / lose its label and state (Linux).
    #[test]
    fn menu_bar_command_ids_exist_in_registry() {
        for section in MENU_BAR {
            for node in section.items {
                if let MenuNode::Command(id) = node {
                    assert!(
                        command(id).is_some(),
                        "MENU_BAR references unknown command id {id:?} (section {:?})",
                        section.label,
                    );
                }
            }
        }
    }

    /// The inline-expanded language ids (used by the Linux dropdown) must
    /// likewise resolve in the registry — same drift guard for the submenu.
    #[test]
    fn dyn_submenu_ids_exist_in_registry() {
        for id in LANG_COMMAND_IDS {
            assert!(
                command(id).is_some(),
                "dynamic submenu references unknown command id {id:?}"
            );
            assert!(
                lang_for_command(id).is_some(),
                "dynamic submenu id {id:?} maps to no language"
            );
        }
    }
}

#[cfg(test)]
mod keybinding_tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn display_keystrokes_share_native_glyph_order_and_named_keys() {
        assert_eq!(display_keystroke("secondary-shift-o"), "⇧⌘O");
        assert_eq!(display_keystroke("ctrl-alt-shift-secondary-k"), "⌃⌥⇧⌘K");
        for key in ["enter", "escape", "backspace", "delete", "space", "up", "-"] {
            let binding = format!("secondary-{key}");
            let stroke = gpui::Keystroke::parse(&binding).unwrap();
            assert_eq!(
                display_keystroke(&binding),
                gpui_component::kbd::Kbd::format(&stroke)
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn display_keystrokes_format_each_chord_stroke() {
        assert_eq!(display_keystroke("secondary-k secondary-s"), "⌘K ⌘S");
        assert_eq!(
            display_keystroke("secondary-k   ctrl-alt-shift-secondary-s"),
            "⌘K ⌃⌥⇧⌘S"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn display_keystrokes_preserve_unformattable_bindings_without_panicking() {
        for binding in ["not-a-keystroke", "secondary-k bogus-modifier-x", "", "  "] {
            assert_eq!(display_keystroke(binding), binding);
        }
    }

    #[test]
    fn display_keystrokes_retain_text_notation() {
        assert_eq!(display_keystroke_text("secondary-shift-o"), "Ctrl+Shift+O");
        assert_eq!(
            display_keystroke_text("ctrl-alt-shift-k"),
            "Ctrl+Alt+Shift+K"
        );
        assert_eq!(display_keystroke_text("secondary--"), "Ctrl+-");
    }

    #[test]
    fn display_keystrokes_text_formats_each_chord_stroke() {
        assert_eq!(
            display_keystroke_text("secondary-k secondary-s"),
            "Ctrl+K Ctrl+S"
        );
        assert_eq!(
            display_keystroke_text("secondary-k   ctrl-alt-shift-secondary-s"),
            "Ctrl+K Ctrl+Alt+Shift+Ctrl+S"
        );
        assert_eq!(
            display_keystroke_text("secondary-- secondary-escape"),
            "Ctrl+- Ctrl+escape"
        );
        assert_eq!(display_keystroke_text(""), "");
        assert_eq!(display_keystroke_text("  "), "  ");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn display_keystrokes_route_chords_to_text_notation() {
        assert_eq!(
            display_keystroke("secondary-k secondary-s"),
            "Ctrl+K Ctrl+S"
        );
    }

    /// Every default keystroke in the registry must be unique — two commands
    /// on one key would make the later binding silently win.
    #[test]
    fn default_keystrokes_are_unique() {
        let mut seen = std::collections::HashMap::new();
        for c in COMMANDS {
            if let Some(k) = c.keystroke {
                if let Some(prev) = seen.insert(k, c.id) {
                    panic!("{} and {} both bound to {}", prev, c.id, k);
                }
            }
        }
    }

    /// `effective_keystroke` is what actually builds the keymap, so all three
    /// of its branches need covering - reading `COMMANDS[..].keystroke` back out
    /// of the table proves nothing about the bindings the app registers.
    ///
    /// NOTE: the override map is a `OnceLock` filled on the FIRST call in the
    /// process, so this must stay the only test that calls
    /// `effective_keystroke`.
    #[test]
    fn effective_keystroke_layers_settings_over_defaults() {
        let _g = crate::ui::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("settings.json"),
            "{\n  \"keybinding.view.toggleSidebar\": \"secondary-alt-1\",\n\
             \"keybinding.repo.fetch\": \"\"\n}\n",
        )
        .expect("write settings.json");
        std::env::set_var("KAGI_LOG_DIR", tmp.path());

        // 1. a non-empty override replaces the registry default.
        assert_eq!(
            effective_keystroke("view.toggleSidebar").as_deref(),
            Some("secondary-alt-1"),
            "settings.json override must win"
        );
        // 2. an EMPTY override means unbound - not "fall back to the default".
        assert_eq!(
            effective_keystroke("repo.fetch"),
            None,
            "empty override must unbind, not restore the default"
        );
        // 3. no override at all -> the registry default is used.
        assert_eq!(
            effective_keystroke("repo.pull").as_deref(),
            Some("secondary-shift-l"),
            "un-overridden command keeps its default"
        );
        // An unknown id resolves to nothing rather than panicking.
        assert_eq!(effective_keystroke("no.such.command"), None);

        std::env::remove_var("KAGI_LOG_DIR");
    }
}

#[cfg(test)]
mod theme_menu_order_tests {
    use super::theme_menu_entries;
    use crate::ui::theme;

    /// The View → Theme menus offered seven fixed commands while Settings
    /// listed every theme (#922): every registered theme is a menu row, in
    /// the case-insensitive name order the other pickers use.
    #[test]
    fn the_theme_menu_lists_every_theme_in_name_order() {
        let entries = theme_menu_entries();
        let mut slugs: Vec<String> = entries.iter().map(|e| e.slug.to_string()).collect();
        let names: Vec<String> = slugs
            .iter()
            .map(|slug| {
                theme::theme_by_slug(slug)
                    .expect("registered theme")
                    .name
                    .to_lowercase()
            })
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        slugs.sort();
        let mut registered: Vec<String> = theme::all_themes()
            .iter()
            .map(|t| t.slug.to_string())
            .collect();
        registered.sort();
        assert_eq!(slugs, registered);
        assert_eq!(entries.iter().filter(|e| e.active).count(), 1);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod standard_app_menu_tests {
    use super::*;

    #[test]
    fn standard_app_menu_order() {
        let menus = menus_for_state(super::super::window_menu::ViewMenuState::default());
        let items = &menus[0].items;
        let labels: Vec<&str> = items
            .iter()
            .map(|item| match item {
                MenuItem::Action { name, .. } => name.as_ref(),
                MenuItem::SystemMenu(menu) => menu.name.as_ref(),
                MenuItem::Separator => "---",
                MenuItem::Submenu(menu) => menu.name.as_ref(),
            })
            .collect();
        assert_eq!(
            labels,
            [
                Msg::AboutTitle.t(),
                "---",
                "Settings…",
                "---",
                "Services",
                "---",
                "Hide Kagi",
                "Hide Others",
                "Show All",
                "---",
                "Quit kagi",
            ]
        );
        assert!(matches!(&items[4], MenuItem::SystemMenu(menu)
            if menu.menu_type == gpui::SystemMenuType::Services));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod standard_app_action_tests {
    use super::*;
    use gpui::Action;

    #[test]
    fn registry_visibility_commands_have_platform_handlers_and_localized_labels() {
        for (id, handler, en, ja) in [
            ("app.hide", App::hide as fn(&App), "Hide Kagi", "Kagiを隠す"),
            (
                "app.hideOthers",
                App::hide_other_apps as fn(&App),
                "Hide Others",
                "ほかを隠す",
            ),
            (
                "app.showAll",
                App::unhide_other_apps as fn(&App),
                "Show All",
                "すべてを表示",
            ),
        ] {
            assert!(std::ptr::fn_addr_eq(
                visibility_command_handler(id).unwrap(),
                handler
            ));
            assert_eq!(
                super::super::command_palette::command_label(id, Lang::En),
                en
            );
            assert_eq!(
                super::super::command_palette::command_label(id, Lang::Ja),
                ja
            );
        }
        assert!(visibility_command_handler("app.quit").is_none());
    }

    #[test]
    fn visibility_actions_and_standard_keystrokes() {
        assert_eq!(command("app.hide").unwrap().keystroke, Some("cmd-h"));
        assert_eq!(
            command("app.hideOthers").unwrap().keystroke,
            Some("cmd-alt-h")
        );
        assert_eq!(command("app.showAll").unwrap().keystroke, None);
        for (id, expected) in [
            ("app.hide", HideApp.name()),
            ("app.hideOthers", HideOtherApps.name()),
            ("app.showAll", ShowAllApps.name()),
        ] {
            match action_menu_item(id, &super::super::window_menu::ViewMenuState::default()) {
                MenuItem::Action {
                    action, os_action, ..
                } => {
                    assert_eq!(action.name(), expected);
                    // This GPUI revision has no visibility OsAction variants.
                    assert!(os_action.is_none());
                }
                _ => panic!("visibility command must be an action"),
            }
        }
    }
}
