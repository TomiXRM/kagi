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

use kagi::git::CommitId;

/// Interval between background auto-fetches (when the `auto_fetch` setting is on
/// and a repo is open). Kept conservative to avoid hammering the remote.
const AUTO_FETCH_INTERVAL_SECS: u64 = 180;

use super::context_menu::CommitAction;
use super::i18n::{self, Lang, Msg};
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
        RefreshRepository,
        // View
        ZoomIn,
        ZoomOut,
        ZoomReset,
        EnterFullScreen,
        ToggleSidebar,
        ToggleCommitDetails,
        ToggleDiffView,
        // Repository
        Fetch,
        Pull,
        Push,
        OpenInFinder,
        // Branch
        NewBranch,
        CheckoutBranch,
        RenameBranch,
        DeleteBranch,
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
        NewWindow,
        CloseWindow,
        // Help
        KeyboardShortcuts,
        Documentation,
        ReportIssue,
        // View → Theme (W9-THEME / ADR-0036): one action per built-in theme.
        ThemeCatppuccin,
        ThemeXcodeDark,
        ThemeXcodeLight,
        ThemeOneDark,
        ThemeOneLight,
        ThemeMonokai,
        // View → Language (W22-I18N / ADR-0048): one action per UI language.
        LangEnglish,
        LangJapanese,
        // ADR-0084: app-level Undo/Redo (Cmd+Z / Cmd+Shift+Z). Distinct from the
        // OsAction text-input EditUndo/EditRedo — these move branch refs via the
        // history plan→confirm modal and are bound only when neither a text Input
        // nor the Terminal is focused.
        HistoryUndo,
        HistoryRedo,
    ]
);

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
        "theme.xcodeDark" => Some("xcode-dark"),
        "theme.xcodeLight" => Some("xcode-light"),
        "theme.oneDark" => Some("one-dark"),
        "theme.oneLight" => Some("one-light"),
        "theme.monokai" => Some("monokai"),
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
    /// A dynamic submenu whose contents (and the "✓ " active marker) are built
    /// at render time.  macOS nests a real `Menu`; Linux expands it inline into
    /// the dropdown panel (it has no nested-panel support) — both behaviours are
    /// preserved from before ADR-0085.
    Submenu(DynSubmenu),
    /// An OS-standard Edit item (handled by the macOS responder chain via
    /// `MenuItem::os_action`).  Only ever appears inside a `mac_only` section,
    /// so Linux skips it wholesale (it has no responder chain — see ADR-0085 §4).
    OsEdit(OsEditItem),
}

/// The two dynamic submenus under View (ADR-0036 theme, ADR-0048 language).
pub enum DynSubmenu {
    Theme,
    Language,
}

/// The OS-standard Edit items (macOS responder chain — ADR-0029).
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
    /// macOS-only section (relies on the responder chain).  Linux skips it
    /// entirely — see ADR-0085 §4 (the intentional Edit-menu OS difference).
    /// Only read by `linux_menu_sections` (dead on non-Linux targets).
    #[cfg_attr(not(any(target_os = "linux", target_os = "freebsd")), allow(dead_code))]
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
            MenuNode::Command("file.cloneRepository"),
            MenuNode::Command("file.openRepository"),
            MenuNode::Command("file.openInTerminal"),
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
            MenuNode::Command("view.toggleTerminal"),
            MenuNode::Command("view.toggleCommitDetails"),
            MenuNode::Command("view.toggleDiffView"),
            MenuNode::Separator,
            // W9-THEME / W22-I18N: dynamic submenus (active item gets "✓ ").
            MenuNode::Submenu(DynSubmenu::Theme),
            MenuNode::Submenu(DynSubmenu::Language),
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
#[cfg_attr(not(any(target_os = "linux", target_os = "freebsd")), allow(dead_code))]
pub fn linux_menu_sections() -> impl Iterator<Item = &'static MenuSection> {
    MENU_BAR.iter().filter(|s| !s.mac_only)
}

/// The ordered theme command ids, as they appear under View → Theme.  Used by
/// the Linux dropdown to inline-expand `DynSubmenu::Theme` (macOS nests
/// [`theme_submenu`] instead).  These mirror the registry's `theme.*` ids.
#[cfg_attr(not(any(target_os = "linux", target_os = "freebsd")), allow(dead_code))]
pub const THEME_COMMAND_IDS: &[&str] = &[
    "theme.catppuccin",
    "theme.xcodeDark",
    "theme.xcodeLight",
    "theme.oneDark",
    "theme.oneLight",
    "theme.monokai",
];

/// The ordered language command ids, as they appear under View → Language.
/// Used by the Linux dropdown to inline-expand `DynSubmenu::Language` (macOS
/// nests [`lang_submenu`] instead).  These mirror the registry's `lang.*` ids.
#[cfg_attr(not(any(target_os = "linux", target_os = "freebsd")), allow(dead_code))]
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

/// Tri-state availability of a command (ADR-0029).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandState {
    /// Available — handler is registered, menu item is active.
    Enabled,
    /// Visible but greyed out, with a reason (handler not registered).
    Disabled(&'static str),
    /// Not shown at all (reserved; currently unused — kagi prefers Disabled).
    #[allow(dead_code)]
    Hidden,
}

/// The full command table.  Order is irrelevant (menus are built explicitly in
/// [`build_menus`]); this slice is the canonical id → metadata map used by the
/// `KAGI_MENU_DUMP` verifier and by keystroke lookup.
pub const COMMANDS: &[Command] = &[
    Command {
        id: "app.about",
        label: "About kagi",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "app.settings",
        label: "Settings…",
        keystroke: Some("cmd-,"),
        dangerous: false,
    },
    Command {
        id: "app.quit",
        label: "Quit kagi",
        keystroke: Some("cmd-q"),
        dangerous: false,
    },
    Command {
        id: "file.newTab",
        label: "New Tab",
        keystroke: Some("cmd-t"),
        dangerous: false,
    },
    Command {
        id: "file.closeTab",
        label: "Close Tab",
        keystroke: Some("cmd-w"),
        dangerous: false,
    },
    Command {
        id: "file.cloneRepository",
        label: "Clone Repository…",
        keystroke: Some("cmd-shift-o"),
        dangerous: false,
    },
    Command {
        id: "file.openRepository",
        label: "Open Repository…",
        keystroke: Some("cmd-o"),
        dangerous: false,
    },
    Command {
        id: "file.openInTerminal",
        label: "Open Repository in Terminal",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "file.refresh",
        label: "Refresh Repository",
        keystroke: Some("cmd-r"),
        dangerous: false,
    },
    Command {
        id: "view.zoomIn",
        label: "Zoom In",
        keystroke: Some("cmd-="),
        dangerous: false,
    },
    Command {
        id: "view.zoomOut",
        label: "Zoom Out",
        keystroke: Some("cmd--"),
        dangerous: false,
    },
    Command {
        id: "view.zoomReset",
        label: "Actual Size",
        keystroke: Some("cmd-0"),
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
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "view.toggleTerminal",
        label: "Toggle Terminal",
        keystroke: Some("cmd-j"),
        dangerous: false,
    },
    Command {
        id: "view.toggleCommitDetails",
        label: "Toggle Commit Details",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "view.toggleDiffView",
        label: "Toggle Diff View",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "repo.fetch",
        label: "Fetch",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "repo.pull",
        label: "Pull",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "repo.push",
        label: "Push",
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
        keystroke: Some("cmd-m"),
        dangerous: false,
    },
    Command {
        id: "window.zoom",
        label: "Zoom",
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
    // View → Theme (W9-THEME / ADR-0036). Labels here are the plain theme names;
    // the live "✓ " active marker is applied in `theme_submenu`.
    Command {
        id: "theme.catppuccin",
        label: "Catppuccin Mocha",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "theme.xcodeDark",
        label: "Xcode Dark",
        keystroke: None,
        dangerous: false,
    },
    Command {
        id: "theme.xcodeLight",
        label: "Xcode Light",
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
    // View → Language (W22-I18N / ADR-0048). The live "✓ " active marker is
    // applied in `lang_submenu`.
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

// ──────────────────────────────────────────────────────────────────────────
// State machine — the one place enabled/disabled/hidden is decided (ADR-0029).
// ──────────────────────────────────────────────────────────────────────────

/// Decide whether a command is enabled / disabled / hidden for the current
/// app state.  This is the **only** place command availability is computed;
/// the menu, the keystrokes, and the `on_action` registration all consult it.
pub fn command_state(app: &KagiApp, id: &str) -> CommandState {
    use CommandState::{Disabled, Enabled};

    // Whether a repository is open (a tab is active).  Welcome = no repo.
    let has_repo = !app.tabs.is_empty() && app.repo_path.is_some();
    // A background git op is running → block state-changing git commands.
    let busy = app.busy_op.is_some();
    // A commit row is currently selected.
    let has_selection = app.selected.is_some();
    // The main (full-width) diff is currently open.
    let diff_open = app.main_diff.is_some();

    match id {
        // ── Always available ────────────────────────────────────────────
        "app.about"
        | "app.settings"
        | "app.quit"
        | "file.newTab"
        | "file.openRepository"
        // W27-UIPOLISH: zoom is global (rem-size scaling) — always available.
        | "view.zoomIn"
        | "view.zoomOut"
        | "view.zoomReset"
        | "view.fullScreen"
        | "view.toggleSidebar"
        | "view.toggleTerminal"
        | "window.minimize"
        | "window.zoom"
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
        // Language switching is always available (W22-I18N).
        | "lang.english"
        | "lang.japanese" => Enabled,

        // ── Placeholders (feature not implemented; greyed with a reason) ──
        "file.cloneRepository" => Disabled(Msg::CloneUnimplemented.t()),
        "branch.rename" => Disabled(Msg::RenameBranchUnimplemented.t()),
        "window.new" => Disabled(Msg::MultiWindowUnsupported.t()),
        // Reset stays disabled per ADR-0024 (no reset in MVP).
        "commit.reset" => Disabled(Msg::ResetUnimplemented.t()),

        // ── Repo required ────────────────────────────────────────────────
        "file.closeTab" => {
            if has_repo {
                Enabled
            } else {
                Disabled(Msg::NoTabsOpen.t())
            }
        }
        "file.openInTerminal" | "file.refresh" | "repo.openInFinder" => {
            if has_repo {
                Enabled
            } else {
                Disabled(Msg::NoRepoOpen.t())
            }
        }

        // ── Repo required + not busy (state-changing git) ────────────────
        "repo.fetch" | "repo.pull" | "repo.push" | "branch.new"
        | "branch.checkout" | "branch.delete" => {
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
        "view.toggleCommitDetails" => {
            if has_repo {
                Enabled
            } else {
                Disabled(Msg::NoRepoOpen.t())
            }
        }
        "view.toggleDiffView" => {
            if diff_open {
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
fn action_menu_item(id: &str) -> MenuItem {
    let label: SharedString = command(id)
        .map(|c| SharedString::from(c.label))
        .unwrap_or_else(|| SharedString::from(id.to_string()));
    match id {
        // kagi (app menu)
        "app.about" => MenuItem::action(label, About),
        "app.settings" => MenuItem::action(label, OpenSettings),
        "app.quit" => MenuItem::action(label, Quit),
        // File
        "file.newTab" => MenuItem::action(label, NewTab),
        "file.closeTab" => MenuItem::action(label, CloseTab),
        "file.cloneRepository" => MenuItem::action(label, CloneRepository),
        "file.openRepository" => MenuItem::action(label, OpenRepository),
        "file.openInTerminal" => MenuItem::action(label, OpenInTerminal),
        "file.refresh" => MenuItem::action(label, RefreshRepository),
        // View
        "view.zoomIn" => MenuItem::action(label, ZoomIn),
        "view.zoomOut" => MenuItem::action(label, ZoomOut),
        "view.zoomReset" => MenuItem::action(label, ZoomReset),
        "view.fullScreen" => MenuItem::action(label, EnterFullScreen),
        "view.toggleSidebar" => MenuItem::action(label, ToggleSidebar),
        // Terminal toggle reuses ToggleBottomPanel (cmd-j); registry label is
        // "Toggle Terminal".
        "view.toggleTerminal" => MenuItem::action(label, ToggleBottomPanel),
        "view.toggleCommitDetails" => MenuItem::action(label, ToggleCommitDetails),
        "view.toggleDiffView" => MenuItem::action(label, ToggleDiffView),
        // Repository
        "repo.fetch" => MenuItem::action(label, Fetch),
        "repo.pull" => MenuItem::action(label, Pull),
        "repo.push" => MenuItem::action(label, Push),
        "repo.openInFinder" => MenuItem::action(label, OpenInFinder),
        // Branch
        "branch.new" => MenuItem::action(label, NewBranch),
        "branch.checkout" => MenuItem::action(label, CheckoutBranch),
        "branch.rename" => MenuItem::action(label, RenameBranch),
        "branch.delete" => MenuItem::action(label, DeleteBranch),
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

/// Build the `MenuItem` for an OS-standard Edit entry (macOS responder chain).
fn os_edit_menu_item(kind: &OsEditItem) -> MenuItem {
    match kind {
        OsEditItem::Undo => MenuItem::os_action("Undo", EditUndo, OsAction::Undo),
        OsEditItem::Redo => MenuItem::os_action("Redo", EditRedo, OsAction::Redo),
        OsEditItem::Cut => MenuItem::os_action("Cut", EditCut, OsAction::Cut),
        OsEditItem::Copy => MenuItem::os_action("Copy", EditCopy, OsAction::Copy),
        OsEditItem::Paste => MenuItem::os_action("Paste", EditPaste, OsAction::Paste),
        OsEditItem::SelectAll => {
            MenuItem::os_action("Select All", EditSelectAll, OsAction::SelectAll)
        }
    }
}

/// Build the full macOS menu bar by walking [`MENU_BAR`] (ADR-0085 §2).  Pure
/// function of the canonical tree + [`COMMANDS`] — no app state is read here;
/// availability is applied later by gpui's per-item validation against the
/// dispatch tree (see module docs).  Called unconditionally on every OS;
/// `cx.set_menus` is a no-op outside macOS, so this stays cross-platform.
pub fn build_menus() -> Vec<Menu> {
    MENU_BAR
        .iter()
        .map(|section| {
            let items = section
                .items
                .iter()
                .map(|node| match node {
                    MenuNode::Command(id) => action_menu_item(id),
                    MenuNode::Separator => MenuItem::separator(),
                    MenuNode::Submenu(DynSubmenu::Theme) => MenuItem::submenu(theme_submenu()),
                    MenuNode::Submenu(DynSubmenu::Language) => MenuItem::submenu(lang_submenu()),
                    MenuNode::OsEdit(kind) => os_edit_menu_item(kind),
                })
                .collect();
            Menu {
                name: section.label.into(),
                items,
            }
        })
        .collect()
}

/// Build the View → Theme submenu (W9-THEME / ADR-0036).
///
/// Each item is one built-in theme; the currently-active theme's label is
/// prefixed with "✓ ".  Because the label changes when the active theme
/// changes, the menu bar must be rebuilt (`cx.set_menus`) on every switch —
/// unlike the disabled/enabled mechanism, which is purely dispatch-tree based.
fn theme_submenu() -> Menu {
    let active = theme::active_index();
    let mut items: Vec<MenuItem> = Vec::with_capacity(theme::THEMES.len());
    for (i, t) in theme::THEMES.iter().enumerate() {
        let label = if i == active {
            format!("\u{2713} {}", t.name)
        } else {
            format!("   {}", t.name)
        };
        let label = SharedString::from(label);
        // Each theme has a distinct action so dispatch is 1:1.
        let item = match t.slug {
            "catppuccin" => MenuItem::action(label, ThemeCatppuccin),
            "xcode-dark" => MenuItem::action(label, ThemeXcodeDark),
            "xcode-light" => MenuItem::action(label, ThemeXcodeLight),
            "one-dark" => MenuItem::action(label, ThemeOneDark),
            "one-light" => MenuItem::action(label, ThemeOneLight),
            "monokai" => MenuItem::action(label, ThemeMonokai),
            _ => continue,
        };
        items.push(item);
    }
    Menu {
        name: "Theme".into(),
        items,
    }
}

/// Build the View → Language submenu (W22-I18N / ADR-0048).
///
/// Two items (English / 日本語); the active language's label is prefixed with
/// "✓ ".  Like the Theme submenu, the label changes on switch, so the menu bar
/// must be rebuilt (`cx.set_menus`) on every language change.
fn lang_submenu() -> Menu {
    let active = i18n::lang();
    let entries = [(Lang::En, "English"), (Lang::Ja, "日本語")];
    let mut items: Vec<MenuItem> = Vec::with_capacity(entries.len());
    for (l, name) in entries {
        let label = if l == active {
            format!("\u{2713} {}", name)
        } else {
            format!("   {}", name)
        };
        let label = SharedString::from(label);
        let item = match l {
            Lang::En => MenuItem::action(label, LangEnglish),
            Lang::Ja => MenuItem::action(label, LangJapanese),
        };
        items.push(item);
    }
    Menu {
        name: "Language".into(),
        items,
    }
}

// Edit-menu action stubs: gpui's `os_action` still needs an `Action` value for
// the dispatch tag, but the OS performs the actual edit via the responder
// chain — these are never dispatched to kagi.  No global KeyBinding is bound to
// them, so they never interfere with text-input cmd-z/x/c/v/a.
actions!(
    kagi_edit,
    [
        EditCut,
        EditCopy,
        EditPaste,
        EditSelectAll,
        EditUndo,
        EditRedo
    ]
);

// ──────────────────────────────────────────────────────────────────────────
// Keybinding registration.
// ──────────────────────────────────────────────────────────────────────────

/// Register all menu keystrokes as gpui `KeyBinding`s so they (a) actually fire
/// when the root has focus and (b) are rendered next to their menu items.
///
/// Deliberately **excludes**:
/// - `cmd-j` (already bound by the bottom-panel ticket; reused for
///   Toggle Terminal — re-binding would double it),
/// - all Edit actions (os_action only — must not shadow text-input).
pub fn register_keybindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        // T-SETTINGS-001: cmd-, opens the Settings window (macOS convention).
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-t", NewTab, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("cmd-shift-o", CloneRepository, None),
        KeyBinding::new("cmd-o", OpenRepository, None),
        KeyBinding::new("cmd-r", RefreshRepository, None),
        // W27-UIPOLISH: UI zoom. `cmd-=` is the conventional "zoom in" (so the
        // user doesn't need shift for `+`); `cmd--` zoom out; `cmd-0` reset.
        KeyBinding::new("cmd-=", ZoomIn, None),
        KeyBinding::new("cmd-+", ZoomIn, None),
        KeyBinding::new("cmd--", ZoomOut, None),
        KeyBinding::new("cmd-0", ZoomReset, None),
        KeyBinding::new("ctrl-cmd-f", EnterFullScreen, None),
        KeyBinding::new("cmd-m", MinimizeWindow, None),
    ]);
}

// ──────────────────────────────────────────────────────────────────────────
// Headless verification — KAGI_MENU_DUMP=1.
// ──────────────────────────────────────────────────────────────────────────

/// Emit one log line per command with its resolved state.  This is the canonical
/// headless verification surface for the menu (the native menu UI cannot be
/// inspected headlessly).  Format (ADR/ticket §5):
/// `[kagi] menu: <id> label="…" key=<ks|-> state=enabled|disabled(<reason>)`
pub fn dump_menu_states(app: &KagiApp) {
    eprintln!("[kagi] menu: dump begin n={}", COMMANDS.len());
    for cmd in COMMANDS {
        let ks = cmd.keystroke.unwrap_or("-");
        let state = match command_state(app, cmd.id) {
            CommandState::Enabled => "enabled".to_string(),
            CommandState::Disabled(reason) => format!("disabled({reason})"),
            CommandState::Hidden => "hidden".to_string(),
        };
        eprintln!(
            "[kagi] menu: {} label=\"{}\" key={} dangerous={} state={}",
            cmd.id, cmd.label, ks, cmd.dangerous, state
        );
    }
    eprintln!("[kagi] menu: dump end");
}

// ──────────────────────────────────────────────────────────────────────────
// Keyboard-shortcuts listing (for the Help → Keyboard Shortcuts modal).
// ──────────────────────────────────────────────────────────────────────────

/// Produce `(label, keystroke)` pairs for every command that has a keystroke,
/// used by the Keyboard Shortcuts modal (auto-generated from the registry).
pub fn shortcut_listing() -> Vec<(SharedString, SharedString)> {
    COMMANDS
        .iter()
        .filter_map(|c| {
            c.keystroke
                .map(|k| (SharedString::from(c.label), SharedString::from(k)))
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
}

/// Transient overlay opened from the menu bar.
#[derive(Clone, Debug)]
pub enum MenuOverlay {
    /// Branch picker list (checkout / delete).
    BranchPicker {
        mode: BranchPickerMode,
        branches: Vec<String>,
    },
    /// Read-only info panel (About / Keyboard Shortcuts), titled with body lines.
    Info {
        title: SharedString,
        lines: Vec<SharedString>,
    },
    /// T-SETTINGS-001 / ADR-0080: the OpenLogi-style Settings window
    /// (Appearance + Language pages). Hosted as an overlay (no sub-window yet).
    Settings,
}

const GITHUB_URL: &str = "https://github.com/TomiXRM/kagi";
const ISSUES_URL: &str = "https://github.com/TomiXRM/kagi/issues";

impl KagiApp {
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
        eprintln!("[kagi] menu: invoke {}", id);
        match id {
            // ── kagi ────────────────────────────────────────────────
            "app.about" => self.open_about_overlay(),
            // T-SETTINGS-001: open the OpenLogi-style Settings overlay.
            "app.settings" => {
                self.menu_overlay = Some(MenuOverlay::Settings);
                cx.notify();
            }
            "app.quit" => cx.quit(),

            // ── File ────────────────────────────────────────────────
            "file.newTab" | "file.openRepository" => self.pick_repository(window, cx),
            "file.closeTab" => {
                if !self.tabs.is_empty() {
                    self.close_tab(self.active_tab, cx);
                }
            }
            "file.cloneRepository" => { /* placeholder — disabled, never dispatched */ }
            "file.openInTerminal" => self.menu_open_terminal(window, cx),
            "file.refresh" => {
                self.reload();
                self.status_footer = FooterStatus::Idle(SharedString::from(Msg::Refreshed.t()));
                self.push_toast(ToastKind::Success, Msg::Refreshed.t());
                // Also fetch the remote (quiet) so changes pushed elsewhere show
                // up — success reloads the graph, failure is silent.
                self.fetch_async(true, cx);
            }

            // ── View ────────────────────────────────────────────────
            // W27-UIPOLISH: zoom mutates the global rem-size factor; the next
            // render reads `theme::rem_size_px()` and re-applies it. Persisted
            // to settings.json by `set_zoom`.
            "view.zoomIn" => {
                let z = theme::set_zoom(theme::zoom() + theme::ZOOM_STEP);
                eprintln!("[kagi] zoom: {:.2}x", z);
            }
            "view.zoomOut" => {
                let z = theme::set_zoom(theme::zoom() - theme::ZOOM_STEP);
                eprintln!("[kagi] zoom: {:.2}x", z);
            }
            "view.zoomReset" => {
                let z = theme::set_zoom(1.0);
                eprintln!("[kagi] zoom: {:.2}x", z);
            }
            "view.fullScreen" => window.toggle_fullscreen(),
            "view.toggleSidebar" => {
                self.sidebar_visible = !self.sidebar_visible;
                eprintln!("[kagi] menu: sidebar_visible={}", self.sidebar_visible);
            }
            "view.toggleTerminal" => {
                self.bottom_panel_open = !self.bottom_panel_open;
                eprintln!("[kagi] menu: bottom_panel_open={}", self.bottom_panel_open);
                if self.bottom_panel_open {
                    self.bottom_tab = BottomTab::Terminal;
                    self.ensure_terminal(window, cx);
                }
            }
            "view.toggleCommitDetails" => {
                self.inspector_visible = !self.inspector_visible;
                eprintln!("[kagi] menu: inspector_visible={}", self.inspector_visible);
            }
            "view.toggleDiffView" => {
                if self.main_diff.is_some() {
                    self.close_main_diff();
                }
            }

            // ── Repository ──────────────────────────────────────────
            "repo.fetch" => self.menu_fetch(cx),
            "repo.pull" => self.open_pull_modal(),
            "repo.push" => self.open_push_modal(),
            "repo.openInFinder" => self.menu_open_in_finder(),

            // ── Branch ──────────────────────────────────────────────
            "branch.new" => {
                let at = self
                    .selected
                    .and_then(|i| self.details.get(i))
                    .or_else(|| self.details.first())
                    .map(|d| CommitId(d.full_sha.to_string()));
                if let Some(id) = at {
                    self.open_create_branch_modal(id, cx);
                }
            }
            "branch.checkout" => self.open_branch_picker(BranchPickerMode::Checkout),
            "branch.delete" => self.open_branch_picker(BranchPickerMode::Delete),

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
            "window.close" => window.remove_window(),

            // ── Help ────────────────────────────────────────────────
            "help.shortcuts" => self.open_shortcuts_overlay(),
            "help.documentation" => cx.open_url(GITHUB_URL),
            "help.reportIssue" => cx.open_url(ISSUES_URL),

            // ── View → Theme (W9-THEME / ADR-0036) ──────────────────
            "theme.catppuccin" | "theme.xcodeDark" | "theme.xcodeLight" | "theme.oneDark"
            | "theme.oneLight" | "theme.monokai" => {
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

    /// Switch the active colour theme (W9-THEME / ADR-0036).
    ///
    /// 1. Update + persist the active theme (`theme::set_active`).
    /// 2. Live-apply the new palette to any running terminal sessions via
    ///    `TerminalView::update_config`.
    /// 3. Rebuild the menu bar so the "✓ " active marker moves (the label
    ///    changes, so `cx.set_menus` must be re-called — unlike the disabled
    ///    mechanism, which is purely dispatch-tree based).
    /// 4. `cx.notify()` so every `theme()`-reading render path repaints.
    pub fn set_theme(&mut self, slug: &str, cx: &mut Context<Self>) {
        if !theme::set_active(slug) {
            return;
        }
        let t = theme::theme();
        eprintln!("[kagi] theme: {} dark={}", t.slug, t.dark);

        // W12-GCADOPT: push the new kagi palette into gpui-component's global
        // ThemeColor so adopted widgets (Input, Tooltip, Scrollbar, Checkbox)
        // follow the switch.  One-way only (kagi → gpui-component).
        theme::sync_gpui_component_theme(cx);

        // Live-apply to running terminal sessions.
        let new_config = super::terminal::build_terminal_config();
        for session in self.terminal_sessions.values() {
            if let Some(view) = session.view.clone() {
                let cfg = new_config.clone();
                view.update(cx, |v, vcx| v.update_config(cfg, vcx));
            }
        }

        // Rebuild the menu bar (active marker moved).
        cx.set_menus(build_menus());
        cx.notify();
    }

    /// Switch the active UI language (W22-I18N / ADR-0048).
    ///
    /// 1. Update + persist the active language (`i18n::set_lang`).
    /// 2. Rebuild the menu bar so the "✓ " active marker moves (the label
    ///    changes, so `cx.set_menus` must be re-called — same as the theme
    ///    submenu).
    /// 3. `cx.notify()` so every prose render path (which reads `Msg::t()` /
    ///    `lang()` live) repaints in the new language.
    pub fn set_lang(&mut self, l: Lang, cx: &mut Context<Self>) {
        i18n::set_lang(l);
        eprintln!("[kagi] lang: {}", l.slug());
        cx.set_menus(build_menus());
        cx.notify();
    }

    /// Map a `commit.*` menu id to the existing [`CommitAction`] and dispatch it
    /// against the currently-selected commit via `dispatch_commit_action`.
    fn dispatch_selected_commit(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let row = match self.selected {
            Some(r) => r,
            None => return,
        };
        let target = match self
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

    /// Open the bottom Terminal panel for the current repo (Repository / File →
    /// Open in Terminal).  Reuses the existing terminal-session plumbing.
    fn menu_open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.bottom_panel_open = true;
        self.bottom_tab = BottomTab::Terminal;
        self.ensure_terminal(window, cx);
    }

    /// Open the repository working tree in Finder via `open <path>` (no shell).
    fn menu_open_in_finder(&mut self) {
        let path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        match std::process::Command::new("open").arg(&path).spawn() {
            Ok(_) => {
                eprintln!("[kagi] menu: open-in-finder {}", path.display());
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
    /// result on the main thread. `silent` suppresses the success/failure toast +
    /// footer (used by auto-fetch) — the commit graph still updates on success via
    /// `reload()` (and the FS watcher would catch the ref change anyway). Never
    /// stacks: a no-op while another fetch is in flight or an operation is busy.
    pub fn fetch_async(&mut self, silent: bool, cx: &mut Context<Self>) {
        if self.fetch_in_flight || self.busy_op.is_some() {
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        self.fetch_in_flight = true;
        if !silent {
            self.refresh_spin_started = Some(Instant::now());
            eprintln!("[kagi] fetch: start");
        }
        let task = cx.background_spawn(async move {
            let backend = kagi::git::Backend::open(&repo_path)
                .map_err(|e| format!("repo open error: {e}"))?;
            backend.fetch_remote().map_err(|e| format!("{e}"))
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.fetch_in_flight = false;
                match result {
                    Ok(outcome) => {
                        app.reload();
                        if silent {
                            eprintln!("[kagi] auto-fetch: ok remote={}", outcome.remote);
                        } else {
                            app.status_footer = FooterStatus::Success(SharedString::from(format!(
                                "Fetched {}",
                                outcome.remote
                            )));
                            app.push_toast(
                                ToastKind::Success,
                                format!("Fetched {}", outcome.remote),
                            );
                        }
                    }
                    Err(e) => {
                        if silent {
                            eprintln!("[kagi] auto-fetch: failed (silent): {e}");
                        } else {
                            app.status_footer = FooterStatus::Failed(SharedString::from(format!(
                                "Fetch failed: {e}"
                            )));
                            app.push_toast(ToastKind::Error, format!("Fetch failed: {e}"));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
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
            gpui::Timer::after(Duration::from_secs(AUTO_FETCH_INTERVAL_SECS)).await;
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
    fn open_branch_picker(&mut self, mode: BranchPickerMode) {
        let branches: Vec<String> = self.branches.iter().map(|(n, _)| n.clone()).collect();
        eprintln!(
            "[kagi] menu: branch-picker mode={:?} n={}",
            mode,
            branches.len()
        );
        self.menu_overlay = Some(MenuOverlay::BranchPicker { mode, branches });
    }

    /// Build the About info overlay.
    fn open_about_overlay(&mut self) {
        self.menu_overlay = Some(MenuOverlay::Info {
            title: SharedString::from("About kagi"),
            lines: vec![
                SharedString::from("kagi — a safe Git GUI client"),
                SharedString::from(format!("version {}", env!("CARGO_PKG_VERSION"))),
                SharedString::from(GITHUB_URL),
            ],
        });
    }

    /// Build the Keyboard Shortcuts overlay from the registry (auto-generated).
    fn open_shortcuts_overlay(&mut self) {
        let lines: Vec<SharedString> = shortcut_listing()
            .into_iter()
            .map(|(label, key)| SharedString::from(format!("{key}    {label}")))
            .collect();
        self.menu_overlay = Some(MenuOverlay::Info {
            title: SharedString::from("Keyboard Shortcuts"),
            lines,
        });
    }

    /// Render the active menu overlay, if any (returns `None` when closed).
    pub fn render_menu_overlay(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        match self.menu_overlay.clone()? {
            MenuOverlay::BranchPicker { mode, branches } => {
                Some(self.render_branch_picker(mode, branches, cx))
            }
            MenuOverlay::Info { title, lines } => Some(self.render_info_overlay(title, lines, cx)),
            // T-SETTINGS-001: the OpenLogi-style Settings window (the field
            // closures route apply/persist through the live `KagiApp` entity).
            MenuOverlay::Settings => Some(super::settings_view::render_settings_overlay(
                cx.entity(),
                self.settings_theme_open,
                self.smart_commit.clone(),
                cx,
            )),
        }
    }

    fn render_branch_picker(
        &self,
        mode: BranchPickerMode,
        branches: Vec<String>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let title = match mode {
            BranchPickerMode::Checkout => "Checkout Branch",
            BranchPickerMode::Delete => "Delete Branch",
        };

        let mut panel = div()
            .w(theme::scaled_px(360.0))
            .max_h(theme::scaled_px(420.0))
            .overflow_hidden()
            .rounded(theme::scaled_px(8.0))
            .border_1()
            .border_color(rgb(theme().selected))
            .bg(rgb(theme().panel))
            .shadow_lg()
            .child(
                div()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(theme().selected))
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(title)),
            );

        if branches.is_empty() {
            panel = panel.child(
                div()
                    .px_3()
                    .py_2()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::NoLocalBranches.t())),
            );
        }

        for (i, name) in branches.into_iter().enumerate() {
            let name_for_click = name.clone();
            let click = cx.listener(move |this, _: &gpui::ClickEvent, _w, cx| {
                this.menu_overlay = None;
                match mode {
                    BranchPickerMode::Checkout => this.open_plan_modal(name_for_click.clone()),
                    BranchPickerMode::Delete => {
                        this.open_delete_branch_modal(name_for_click.clone())
                    }
                }
                cx.notify();
            });
            panel = panel.child(
                div()
                    .id(("branch-pick", i))
                    .px_3()
                    .py(theme::scaled_px(6.0))
                    .text_sm()
                    .text_color(rgb(theme().text_sub))
                    .cursor_pointer()
                    .hover(|s| {
                        s.bg(rgb(theme().selected))
                            .text_color(rgb(theme().text_main))
                    })
                    .on_click(click)
                    .child(SharedString::from(name)),
            );
        }

        self.wrap_overlay(panel.into_any_element(), cx)
    }

    fn render_info_overlay(
        &self,
        title: SharedString,
        lines: Vec<SharedString>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut panel = div()
            .w(theme::scaled_px(420.0))
            .max_h(theme::scaled_px(480.0))
            .overflow_hidden()
            .rounded(theme::scaled_px(8.0))
            .border_1()
            .border_color(rgb(theme().selected))
            .bg(rgb(theme().panel))
            .shadow_lg()
            .child(
                div()
                    .px_4()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(theme().selected))
                    .text_color(rgb(theme().color_branch))
                    .child(title),
            );

        for line in lines {
            panel = panel.child(
                div()
                    .px_4()
                    .py(theme::scaled_px(3.0))
                    .text_sm()
                    .text_color(rgb(theme().text_sub))
                    .child(line),
            );
        }

        self.wrap_overlay(panel.into_any_element(), cx)
    }

    /// Centre an overlay panel over a dim, click-to-dismiss backdrop.
    fn wrap_overlay(&self, panel: gpui::AnyElement, cx: &mut Context<Self>) -> gpui::AnyElement {
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

    /// The inline-expanded theme/language ids (used by the Linux dropdown) must
    /// likewise resolve in the registry — same drift guard for the submenus.
    #[test]
    fn dyn_submenu_ids_exist_in_registry() {
        for id in THEME_COMMAND_IDS.iter().chain(LANG_COMMAND_IDS.iter()) {
            assert!(
                command(id).is_some(),
                "dynamic submenu references unknown command id {id:?}"
            );
            assert!(
                theme_slug_for_command(id).is_some() || lang_for_command(id).is_some(),
                "dynamic submenu id {id:?} maps to neither a theme nor a language"
            );
        }
    }
}
