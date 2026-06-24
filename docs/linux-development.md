# Linux / Ubuntu development & testing

Practical notes for building, running, and testing Kagi on Linux (verified on
**Ubuntu 24.04, GNOME Wayland, Intel Meteor Lake iGPU**). Kagi is a GPUI app, so
most of this is really "how to run a GPUI/Blade app on Linux."

> **TL;DR for "it feels sluggish / clicks seem to lag on Linux but it's fine on
> macOS":** you are almost certainly running a **debug** build. Build & run
> `--release`. Debug builds make diff + tree-sitter highlighting + graph layout
> 10–50× slower (≈0.2 s lag per file click, janky scroll). See
> [Performance](#performance-debug-vs-release).

## 1. System dependencies (apt)

```sh
sudo apt-get install -y \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libx11-dev libxcb1-dev libfontconfig-dev libfreetype-dev \
  libasound2-dev libvulkan-dev libzstd-dev
```

Plus Rust stable via rustup.

### Gotcha: missing `libxkbcommon-x11-dev` → link error

If `libxkbcommon-x11-dev` is not installed, the **binary fails at link time** even
though the library crate compiles:

```
rust-lld: error: unable to find library -lxkbcommon-x11
```

Fix: install the package above. If you cannot install it (no sudo), a temporary
link-only workaround:

```sh
mkdir -p /tmp/kagi_link
ln -sf /lib/x86_64-linux-gnu/libxkbcommon-x11.so.0 /tmp/kagi_link/libxkbcommon-x11.so
RUSTFLAGS="-L /tmp/kagi_link" cargo build --release
```

## 2. Build & run

```sh
# Development (fast iteration on non-perf-critical code)
cargo run -- /path/to/repo

# Real use / perf testing — ALWAYS release
cargo build --release
./target/release/kagi /path/to/repo
```

First build takes a few minutes (gpui / libgit2); incremental builds are seconds.
Point Kagi at a normal repo with a working tree (bare repos are unsupported).

## 3. Performance: debug vs release

Kagi does real compute on interaction — `git2` diffs, **tree-sitter** syntax
highlighting, and commit-graph layout. In a **debug** build these are 10–50×
slower, which shows up as:

- ~0.2 s delay when clicking a file in the commit panel (diff + highlight),
- janky / slow commit-list scrolling (graph layout),
- *resizing stays smooth* (it repaints without recomputing) — a useful tell.

A **release** build is dramatically faster and matches macOS. macOS users compare
against the installed (release) app, so "Linux is way slower" is usually
release-vs-debug, not a GPUI/Wayland/GPU problem.

Quick frame-rate sanity check (Kagi logs a frame counter):

```sh
KAGI_DEBUG_RENDER=1 ./target/release/kagi . 2>&1 | grep render:
# Idle should NOT keep climbing (Kagi repaints on demand, not every frame).
```

## 4. Wayland vs X11 (XWayland)

GPUI supports both. Backend is chosen at startup (`gpui::guess_compositor`):
`WAYLAND_DISPLAY` set → Wayland; else `DISPLAY` set → X11; else headless.

Force XWayland (useful to isolate a Wayland-specific issue):

```sh
WAYLAND_DISPLAY="" ./target/release/kagi .   # or: env -u WAYLAND_DISPLAY ...
```

On Linux the window uses **client-side decorations** (`WindowDecorations::Client`)
and Kagi's own in-app menu bar / title bar (`src/ui/mod.rs`,
`render_platform_titlebar`), since GNOME/Mutter does not do server-side
decorations.

### Gotcha: clicks land low / toolbar buttons don't respond (Wayland CSD offset)

On some **native-Wayland + fractional-scaling** sessions (e.g. GNOME at 125 %/
150 %), the client-side-decoration inset shifts hit-testing so clicks land a bit
**below** where they render: top-of-window controls (Pull, Settings, the Analyze
✕) stop responding and a horizontal **resize cursor** flickers on the dead
clicks, while large areas (branch list, commit graph) still work. It is
intermittent and depends on which output/scale the window opens on. This is an
upstream gpui CSD/coordinate issue, not Kagi's wiring.

Workarounds:

```sh
KAGI_NO_CSD=1 ./target/release/kagi .   # server-side decorations: drops the inset, realigns input
WAYLAND_DISPLAY="" ./target/release/kagi .   # or fall back to XWayland (also unaffected)
```

`KAGI_NO_CSD=1` keeps native Wayland and only loses the drop shadow / rounded
corners (Kagi draws its own title bar). If you can reproduce it reliably, grab
the offset so it can be fixed properly: build with a click logger and compare the
reported position to where the button renders.

## 5. GPU / Vulkan (Blade renderer)

GPUI renders via **Blade → Vulkan**, so a working Vulkan driver is required.

```sh
sudo apt install -y vulkan-tools
vulkaninfo --summary | grep -iE 'deviceName|deviceType|driverName'
vkcube   # spinning cube ⇒ Vulkan OK
```

Caveat worth knowing: `blade-graphics` 0.7.1 picks the **first** Vulkan device
that passes inspection (no hardware-over-software ranking). If the hardware ICD is
rejected or enumerated after `lvp` (lavapipe / llvmpipe = CPU software Vulkan),
you can silently land on software rendering and everything is slow. Pin the
hardware driver to test:

```sh
# Intel example — adjust the ICD path to your GPU
VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json ./target/release/kagi .
```

If forcing the hardware ICD changes nothing, you were already on hardware (good —
look elsewhere, e.g. debug vs release).

## 6. Tests

```sh
cargo test            # full suite (≈550+ tests; git-logic, view-models, undo/redo, …)
cargo test --test ops_test status_test diff_test    # focused
```

The headless `KAGI_*` harness (see `src/headless.rs`) and the `tests/` files do
not need a display for the logic layer. One live test is `#[ignore]`d
(`remote_ssh_live_test`, needs a real SSH endpoint).

GUI behavior (real clicks/hover) can't be asserted headlessly — run the app and
watch the `[kagi] …` stderr state dumps (selection, diff, file-history, toolbar)
to confirm interactions land.

## 7. Bundling

```sh
cargo run -p xtask -- bundle-linux       # tar.gz (bin + .desktop + icon)
cargo run -p xtask -- bundle-appimage    # AppImage (needs appimagetool)
```

Both produce **release** builds. For local install onto `PATH`:
`cargo install --path .`.

## 8. Troubleshooting quick table

| Symptom | Likely cause | Action |
|---|---|---|
| UI sluggish / clicks "lag" vs macOS | running a **debug** build | `cargo build --release` |
| `-lxkbcommon-x11` link error | `libxkbcommon-x11-dev` missing | install it (§1) |
| Blank/unresponsive window, very slow | software Vulkan (lvp) or no GPU | check §5, pin hardware ICD |
| Wayland-only weirdness | GPUI Wayland backend | retest with `WAYLAND_DISPLAY=""` (§4) |
| Top buttons dead, clicks land low, resize cursor flickers | Wayland CSD + fractional-scale offset | `KAGI_NO_CSD=1` (§4) or `WAYLAND_DISPLAY=""` |
