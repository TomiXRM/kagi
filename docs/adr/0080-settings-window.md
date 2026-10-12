# ADR-0080: Settings Button + Settings Window (OpenLogi-style)

- Status: Accepted / Date: 2026-06-14
- Context: New feature for release. Builds on the existing settings storage
  (`src/ui/settings.rs`: `settings.json`, `read_setting`/`write_setting`,
  `SETTINGS_KEYS`) and theme/i18n/zoom (ADR-0036/0048). Reference: OpenLogi
  `crates/openlogi-gui/src/windows/settings.rs`.
- macOS host superseded by [ADR-0223](0223-macos-settings-window-host.md) (#1198):
  one non-modal native Settings window. The overlay remains on other platforms.

## Decision

- Add a **Settings button (gear icon) in the top-right of the main window header**
  (`render_header_slot`, the right-hand meta-op cluster near Refresh/Terminal).
  Icon: `gpui_component::IconName::Settings`. Also reachable via the menu bar
  ("Settings…") and `cmd-,`.
- Clicking opens a **Settings view modeled on OpenLogi**, built with the SAME
  component OpenLogi uses — `gpui_component::setting::{Settings, SettingPage,
  SettingGroup, SettingItem, SettingField}` (present in our pinned
  gpui-component 0.5.1) — a **left sidebar of pages + per-field rows** (title +
  description + control). Hosted as a Kagi overlay (`MenuOverlay::Settings`,
  consistent with the existing About/Keyboard-Shortcuts overlays), centered, ~820×520.
- **Pages / fields (MVP)** — each wired to the existing `settings.rs`
  `write_setting`/`read_setting` and applied live:
  - **Appearance**: Theme (`Select`, the 6 themes) · UI Zoom (`Slider`/stepper) ·
    Compact graph (`Switch`).
  - **Language**: Interface language (`Select`: English / 日本語) — ADR-0048
    (explanatory prose localized; Git domain words & branch names stay English).
  - (room for **Git/Tools** later: external mergetool path, ADR-0060.)
- **Live apply + persist**: a control change calls the existing apply path
  (`theme::set_theme(slug)`, `theme::set_zoom(...)`, i18n locale switch, compact
  toggle) AND `write_setting(...)`; the open windows refresh. No new persistence
  layer — reuse `settings.rs`.

## なぜ
- Discoverability: a visible top-right gear is the expected place (GitKraken/OpenLogi/
  most apps). The menu-bar-only path is not enough.
- Reuse: `gpui_component::setting` gives the OpenLogi look for free and keeps page
  navigation/search consistent with the component set; `settings.rs` already stores
  every value we expose, so the window is a thin view over existing infra (no Git in
  the view; ADR-0078 invariant holds — settings touch no repo).

## 代替案 / 捨てた案
- **Separate OS window** (OpenLogi's `windows::open_or_focus`) — Kagi has no
  multi-window registry yet; About/Shortcuts are overlays. An overlay matches the
  current architecture and is far cheaper. A real sub-window is deferred (the
  `gpui_component::setting::Settings` content is window/overlay-agnostic, so the move
  is later non-breaking).
- **Hand-rolled sidebar+fields** — rejected; `gpui_component::setting` already exists
  in 0.5.1 and matches OpenLogi exactly.
- **Bumping gpui-component to 0.5.2** (OpenLogi's pin) — unnecessary; 0.5.1 has the
  module.

## 将来の負債 / リスク
- Overlay vs real window (deferred). Settings search (the widget supports it) not
  wired in MVP. The `SETTINGS_KEYS` list must include any new keys exposed.
- Live theme/zoom apply must stay on the lock-free per-frame path (research #8).

## Consequences
- New `MenuOverlay::Settings` variant + a `settings_view` render module; a gear button
  in `render_header_slot`; menu + `cmd-,` action. No repo/git access from the view.
- ticket: T-SETTINGS-001.

## Amendment — 2026-10-12: Appearance accessibility (#1349 / #1350)

The existing open-density Settings controls keep their layout and interaction.
The pinned gpui-kit Select (879f3bf) has no trigger AX builder or public open
getter, so Kagi wraps the call site with ComboBox, localized Theme name, current
display-name value and expanded state. The initial trigger FocusHandle is retained;
the kit's Focusable implementation returns a different list handle while open.
Observing Select notifications keeps the wrapper current without another open flag.
ThemeOption's row rendering adds named ListBoxOption content in EN/JA.

Zoom keeps the kit's outline Buttons. Their AX names describe increase/decrease
and the live percentage; the percentage itself is a localized Status. Comparing
step_zoom with the current value disables only bound actions. The pinned kit
owns disabled paint/interaction but does not publish AX disabled; Kagi adds that
property to the same Button node. Glyphs are children rather than Button labels
so kit rendering cannot overwrite the descriptive AX name with −/+. Existing
hover/pressed/focus styling, selection, Escape,
live apply and persistence remain kit/existing-path owned; loading, empty and
error states are not introduced. Renderer-fed Tier A metadata is not native
VoiceOver evidence; native AX delivery remains a Tier B inspection.
