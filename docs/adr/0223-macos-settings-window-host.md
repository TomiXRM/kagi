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
localized groups. Pane selection, position and the last content height are saved
through settings::; invalid/missing selections use General, and disconnected-
display positions recenter. Restoring height avoids Cocoa's bottom-anchored
initial resize moving the saved frame on each reopen. The selected pane
determines content height through real layout, clamped to 500 logical points and
85% of the current display; oversized pages remain reachable through one
persistent system-scrollbar owner. Width does not scale with UI zoom.
Both position and height are saved on bounds changes as one flat-string
settings_window_frame value ("x,y,content-height"), not alternating preference
keys. The sole store can coalesce a continuous bounds burst, and the normal
application-quit flush also preserves an open Settings frame. The settings module
reads the former settings_window_position/settings_window_height pair once,
converts it to the frame key and removes the obsolete pair; unrelated settings
remain intact.

Closing or Cmd-W removes only the Settings window and flushes the existing
store. It never dispatches repository Close Tab, changes sessions, or exits the
process. Cmd-comma from either window raises the existing host. Escape does
**nothing at the window level**, deliberately matching the requested macOS
behavior; an editing text field retains its own Escape/IME handling. No
repository focus trap or dimming scrim is installed. Repository windows remain
usable and all windows repaint when global appearance/language/zoom changes.

The host and every reused settings callback hold only weak repository entity
handles. Releasing its originating repository closes Settings and flushes the
store; Settings does not keep that repository's watchers, terminal sessions or
periodic fetch owner alive. Other repository windows remain usable. Dock reopen
counts repository Roots, not utility windows, using the same typed-root
predicate as live terminal preference broadcasts, and restores the saved
repository session when only a utility window remains.

## Coverage and evidence boundaries

The existing settings_*, platform_menu_* and filter_menu_then_settings native
scenarios retain every assertion. Their test setup explicitly selects the
portable non-macOS overlay host, allowing those interaction/focus contracts to
be rendered and exercised on the macOS test backend. This is a test host
selection, not a production feature flag; an RAII guard restores it on normal
return and panic. settings_window_open_raise, settings_window_close_keeps_repo,
settings_window_live_apply, settings_window_owner_release,
settings_window_position_restore and settings_window_dock_reopen explicitly
select the production macOS host at EN/JA and 100%/167%. Tests check window
identity/count, independent closure, native repository theme pixels, repository
entity release after a running fetch ticker, session restoration with only
Settings open, stable geometry over three reopen cycles, and repository Cmd-Q
with Settings still open followed by root teardown/recreation. The explicit
Quit interception seam runs normal action dispatch and the quit-time preference
flush without exiting the test controller; teardown bypasses Settings close so
its close handler cannot hide missing geometry persistence.
settings_window_host_isolation also exercises guard cleanup during unwinding.
Isolated real-store unit coverage drives 100 calls through the same production
frame writer and requires at most two successful atomic commits after flush;
it also verifies one-time legacy migration and preservation of unknown keys.

Tier A's hidden windows do not prove real OS activation, dragging, traffic-light
presentation or text-field IME. Tier B must exercise these on the launched PID,
list both repository and Settings CGWindow IDs, and capture Settings with
screencapture -l<settings-window-id>, not the first PID-matching window.

## Amendment — 2026-10-12: settings control feedback (#1298 / #1299)

The Smart Commit pane shares the existing manual TabList choice focus owner
with the portable overlay; pointer activation returns to the active Settings
host's focus, and departing/removed cells yield focus to that host. Analyze
Save/Reset outcomes use the originating app's existing bounded ToastStack,
placed with the repository window's shared out-of-flow toast container so
feedback cannot change page layout or content-height measurement. The saved
editor baseline and full inline failure remain on KagiApp, not on this window.
