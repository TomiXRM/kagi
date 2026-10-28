# ADR-0223: macOS Settings uses one non-modal native window

Status: Accepted — 2026-10-12. Issue #1198. Supersedes ADR-0080's macOS overlay host; ADR-0093, ADR-0183 and ADR-0191 remain unchanged.

## Decision

On macOS every Settings entry point opens or raises one process-wide window,
not a repository overlay. Since #1816 it mounts the shared production Settings
entity and retained InputState in the same gpui-component Root used by repository
windows. The native host owns the OS window and frame; the shared entity owns
pane selection, control focus and detail scrolling. The native adapter retains
store/effect ownership, with no second Settings store or repository read model
in this host. Other platforms keep the portable overlay around the same entity.

The 900-logical-point window is non-resizable and non-modal, with standard
close/minimize lights and disabled zoom. Its transparent unified frame places
the traffic lights inside the canonical sidebar's top inset. Kagi draws no
additional title band: the shared detail title names the pane exactly once.
The host owns dragging in the blank top inset, not pane controls.
The #1816 panes are Appearance, Graph, Editor, Terminal,
Network & Sync, Smart Commit, Analyze and General (language). Pane selection,
position and the last content height are saved through settings::; invalid or
missing selections display Appearance, but a missing preference is not written
merely by opening Settings. Existing legacy selections migrate once.
Disconnected-display positions recenter. Restoring height avoids an unnecessary
initial resize on every reopen.
The selected pane determines content height through real layout, with a
500-logical-point floor when the display permits it. Initial height is 760
points, matching the canonical theme chooser; restored and content-driven
height are bounded only by the current display's visible frame, not by a
compact-height ceiling. Oversized pages remain reachable through the existing
vertical scrollbar.
Width does not scale with UI zoom.
Both position and height are saved on bounds changes as one flat-string
settings_window_frame value ("x,y,content-height[,display-uuid]"), not alternating
preference keys. Display UUID is optional for existing frames and recorded as
32 hexadecimal digits for new captures. The sole store coalesces a continuous
bounds burst, and the normal
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

The Smart Commit pane shares the production choice-group focus owner with the
portable overlay; selection, roving focus and unavailable choices remain distinct.
Analyze Save/Reset outcomes use the originating app's bounded ToastStack in the
window's out-of-flow detail-content slot, never over the sidebar. Feedback cannot
change page layout or content-height measurement. The shared entity owns the
accepted editor baseline and inline failure; the native adapter owns file writes.

Since both hosts render the same ToastStack, each card retains its rendered
close handle per window rather than overwriting one global handle. Both hosts
subscribe to card removal in their own window and repair retired close-button
focus to their own root, preserving pane navigation and native host commands
after either pointer or keyboard dismissal.
Settings host release also removes its own card handles from the shared stack,
without dismissing any feedback or clearing another window's handles. Persistent
error cards therefore cannot accumulate retired Settings windows' focus state.

## Amendment — 2026-10-13: Analyze draft safety and visible form help (#1404 / #1415 / #1416)

Analyze compares its retained InputState text with the last successfully saved
baseline. Save is disabled when they match; localized Saved/Unsaved changes
text makes this explicit. A failed write retains the draft and baseline, with
the existing full inline error and bounded toast. Pane changes and host reopen
retain the same editor.

Reset keeps its explicit immediate-save meaning. When the draft differs from
saved text, KagiApp's Settings form state (`settings_controls.reset_alert`)
owns the AlertDialog outside `active_modal`. Both native and portable Settings
render and key-route it inside their own host. The single `active_modal` slot
remains the main window's modal invariant; a Settings form alert is not another
occupant of that slot and never replaces or invalidates an existing or pending
main-window plan, dirty guard, trust prompt, or other modal.

In the portable host, Settings is an overlay inside the main window. Any
`active_modal` (including AppNotice, About, Update, Clone and RemoteBrowse) or
visible Commit Panel plan causes `sync_pending_focus` to close Settings and
retire its alert without writing. The native host is a separate window:
the alert's originating window id keeps main-window rendering from retiring
it, and SettingsWindow release clears its own alert without writing.

Both hosts use the same alert renderer, focus trap, and AlertReturnFocus helper.
The alert names both discarded unsaved patterns and the saved analyze_ignore
file that defaults will replace. Return and Escape cancel; only explicit Reset
writes through settings::. Cancellation returns focus within Settings and
leaves the portable overlay open. Closing the originating host retires its
pending approval without writing or installing a repository-window scrim.

Settings is an open-density form: important details are secondary text under
the corresponding control, with the existing page scroll owner and no nested
help scroller. Full tooltips remain available, and switch accessibility
descriptions include the same localized detail. Analyze and custom-theme
help follow the same visible-detail policy. This supersedes the tooltip-only
detail presentation without changing live preference apply or storage.


## Amendment — 2026-10-14: Settings screen containment (#1549)

Settings is an open-density auxiliary window, using macOS Settings as its
placement reference, not a larger repository window or a second form control
convention. Every initial/restored frame fits the selected display's visible
area. Content-driven height changes preserve the top edge where possible and
clamp the entire native frame, including its bottom and right edges. The native
frame constraint accounts for any frame/content inset and uses AppKit's own
screen coordinates, rather than mixing display-local GPUI coordinates across
monitors. The existing scroller owns content that exceeds the visible height.
Repository window defaults and restoration policy are untouched.
Fitting runs only for initial layout, changed intrinsic content height, pane or
zoom changes, and changed connected-display geometry. Ordinary native moves only
persist geometry; redraws do not re-clamp an unchanged layout. A user may therefore
leave a window partially offscreen, under the Dock, or across a display boundary.
The cached layout key includes all displays' full/visible frames, not the current
window's display, so dragging between displays is not a topology change.
Saved display identity uses the GPUI display UUID, not an ambiguous local x/y
coordinate or the currently focused screen. A disconnected UUID recenters on
the primary display; the old three-component frame remains readable and acquires
its display identity at the next native bounds capture.
Unmodified Page Up/Down reach the shared detail viewport when an editing control
has not consumed them. This keeps focused controls intact while allowing keyboard
inspection of page content below the fitted frame.

The native `settings_window_screen_fit` regression checks window bounds against
the display's visible bounds after first open, all pane transitions, 100%→167%
zoom and reopen in EN/JA and Apple Light/Dark. Wheel scrolling and real Tab plus
Page Down must reach the final Appearance switch inside that page viewport; the
native accessibility tree retains its localized name. Saved right/bottom-overflow
frames must restore on their own connected display and fit fully, not merely
expose a titlebar. These deterministic hidden-window bounds do not prove physical
pointer delivery or the physical bottom edge; Tier B captures the real Settings
window on the main display at 167%.
The same regression also moves the live native frame partly beyond the right and
bottom visible edges, drains its bounds notifications and redraws repeatedly,
and requires the chosen position to survive until a pane/content/zoom change.
The pane-transition regression starts from the actual selected pane's successor:
clicking an already-selected Appearance pane is not a layout transition and must
not undo a user's unchanged manual position.

## Amendment — 2026-10-27: compact Appearance theme chooser (#1816)

The owner requests a horizontal theme chooser instead of showing every theme
in a multi-row grid, and a shorter default Settings window. Extend the existing
`SettingsView::theme_cards` and native `SettingsWindow`; do not introduce a
second renderer, preference owner, carousel manager or stored scroll position.
The shared/native/browser boundary in ADR-0121 is unchanged.

The role remains an open-density Settings choice. The subsequent owner
canonical ruling below selects the final PR #1814 `lab_i10_h` geometry:
132×84-point previews, centered captions and fixed-hue tiles. Preview content
uses real theme palettes rather than gallery fixture colors. Both labeled
strips remain mounted, Light Mode above Dark Mode, classified by `Theme.dark`.
Reuse the existing Editor tab-strip
scrollbar and nearest-item reveal pattern. This is one continuously scrollable
radio group, not dual preferences, a numeric slider, auto-playing carousel or pager.
Apple's Scroll views guidance says: “It's alright to place a horizontal scroll
view inside a vertical scroll view (or vice versa), however.”

State contract: rest/hover/pressed use the existing card and activation handler;
selection remains the single effective theme, while focus is independent.
Global Light-then-Dark arrows and Home/End reveal the focused card including its
complete ring; Space/Return applies that choice. Each strip owns its horizontal
offset. Selected-theme reveal runs on opening, returning to Appearance, changed
selection/catalog/zoom and viewport resize, not ordinary redraws, focus loss or
manual browsing. First and last cards keep local ring clearance. Horizontal
gestures and the scrollbar browse the targeted strip without moving the other;
vertical wheel input still scrolls the containing form. Full names, constrained
caption ellipses, palette swatches and low-contrast explanations remain.
Theme loading/error and unavailable filesystem actions remain native-adapter
states, not new carousel states; the normal catalog contains built-ins.
There is no new destructive, approval, IME or persistence behavior.

Initial/restored and content-driven heights fit the visible display area,
retaining the existing 500-point floor where possible. Position, display
identity, unknown settings and normal bounds/quit flushing remain intact.
The window remains non-resizable. Browser narrow-width and zoom cases exercise
shared-view containment; they are not the native window-size or chrome oracle.
At large zoom, all eight destinations remain reachable by wheel and keyboard
with the focused/selected row revealed. The sidebar may flex below its 55%
ceiling to preserve whole detail controls on smaller display areas.

## Amendment — 2026-10-29: owner-designated canonical composition (#1818)

The owner-designated source is `origin/experiment/kagi-settings-skill` at
`1c6486202`: `src/ui/settings_kit/{mod,controls}.rs`,
`experiments/settings-skill/runs/i10/lab_i10_s8.rs.txt` and
`lab_i10_h.rs.txt`. It supersedes the compact host dimensions and extra native
title band, not native ownership, persistence, focus or consent behavior.
The owner explicitly selected canonical 132×84 previews over the later
144/88 port, direct `segment_selected_bg()` selection over hard-coded white,
and a 900-point window with initial 760-point/content-driven screen-bounded
height over the compact ceiling. The eight #1816 panes/order, no Search field
and one-setting panes remain owner-approved exceptions.

The sidebar panel begins at (8,8), is 212 points wide and extends to the
bottom inset. Its 40-point top padding accommodates the OS-drawn traffic lights.
Rows are 32 points with a 2-point gap and neutral selection. Each appearance's
theme strip sits inside its own group card, and Accent has a section heading.
The native `settings_window_composition` scenario captures everything Kagi draws
with `window.render_to_image()` and records actual AppKit traffic-light bounds;
Metal images do not contain the OS-drawn lights. Composition evidence lives in
`target/ui-fidelity/settings/`, including the measured table and same-scale
canonical/native sheets. Bounds relations are asserted on the native owner,
not on copied implementation constants.

