# ADR-0223: macOS Settings uses one non-modal native window

Status: Accepted — 2026-10-12. Issue #1198. Supersedes ADR-0080's macOS overlay host; ADR-0093, ADR-0183 and ADR-0191 remain unchanged.

## Decision

On macOS every Settings entry point opens or raises one process-wide window,
not a repository overlay. The host wraps the existing settings entity callbacks,
theme Select/Input entities and #1200 grouped-form section renderers in the
same gpui-component Root used by repository windows. It owns presentation only:
selected pane, focus, scroll and the OS window handle. There is no second
Settings store and no repository read model, tab, modal slot or Git access in
this host. Other platforms keep the portable overlay implementation.

The 715-logical-point window is non-resizable and non-modal, with standard
close/minimize lights and disabled zoom. The transparent unified compact frame
sets app_owns_titlebar_drag and owns dragging only on its blank/title surface,
not pane controls. General, Language, Smart Commit and Analyze use the existing
localized groups. Pane selection and position are saved through settings::;
invalid/missing selections use General, and disconnected-display positions
recenter. The selected pane determines content height through real layout,
clamped to 500 logical points and 85% of the current display; oversized pages
remain reachable through one persistent system-scrollbar owner. Width does not
scale with UI zoom.

Closing or Cmd-W removes only the Settings window and flushes the existing
store. It never dispatches repository Close Tab, changes sessions, or exits the
process. Cmd-comma from either window raises the existing host. Escape does
**nothing at the window level**, deliberately matching the requested macOS
behavior; an editing text field retains its own Escape/IME handling. No
repository focus trap or dimming scrim is installed. Repository windows remain
usable and all windows repaint when global appearance/language/zoom changes.

## Coverage and evidence boundaries

The existing settings_*, platform_menu_* and filter_menu_then_settings native
scenarios retain every assertion. Their test setup explicitly selects the
portable non-macOS overlay host, allowing those interaction/focus contracts to
be rendered and exercised on the macOS test backend. This is a test host
selection, not a production feature flag. settings_window_open_raise,
settings_window_close_keeps_repo and settings_window_live_apply explicitly
select the production macOS host at EN/JA and 100%/167%. Tests check window
identity/count, independent closure and native repository theme pixels.

Tier A's hidden windows do not prove real OS activation, dragging, traffic-light
presentation or text-field IME. Tier B must exercise these on the launched PID,
list both repository and Settings CGWindow IDs, and capture Settings with
screencapture -l<settings-window-id>, not the first PID-matching window.
