# ADR-0166: AI-verifiable GPUI GUI E2E — Lane 1 in-process PoC

- Status: Proposed (PoC findings) / Date: 2026-09-04
- Context: issue #432（親 #359 2026Q3 AI-native）。Lane 1（macOS native in-process）のみ。
  Lane 2（kagi-web / Playwright）は対象外。
- 前提 ADR: 0077（4層テスト戦略、`VisualTestContext` は「window が本当に要る箇所だけ」）、
  ADR-0097（web harness は pure-domain、Backend を含まない）。

## 何を実証したか

`gpui` の `test-support` feature が解禁する **`VisualTestAppContext`**（macOS 専用、
実 Metal off-screen 描画 + 決定論的 `TestDispatcher` + keyboard/mouse/`Action` 注入 +
`capture_screenshot`）で、**実 `KagiApp` root/bootstrap** を fixture repo に対して
mount できることを確認した。

PoC test: `src/ui/gui_e2e_poc.rs::poc_toggle_bottom_panel_visual`
（`#[cfg(all(test, target_os = "macos"))]`, `#[ignore]`）。

シナリオ（read-only / plan 以前）:
1. 一時 git repo（2 commit）を生成 → `kagi_git::open_repository` + `Backend::snapshot`
   でスナップショット化（`git2::` トークン不使用 = src/ui gate 遵守）。
2. `VisualTestAppContext::with_asset_source(current_platform(false), Arc::new(KagiAssets))`
   を構築し、`fonts::load_bundled_fonts` / `gpui_component::init` /
   `theme::sync_gpui_component_theme` / `bind_keys(secondary-j → ToggleBottomPanel)` を実行。
3. `KagiApp::from_snapshot` で実状態を組み、`open_offscreen_window` に
   `open_main_window` と同一形の closure（`KagiApp` entity を `gpui_component::Root` で包む）
   で mount。
4. keyboard `cmd-j` と registered `Action` `ToggleBottomPanel` を dispatch、
   `run_until_parked` で settle（実時間 sleep なし）、`bottom_panel_open` の反転/復帰を assert。
5. before/after PNG を `capture_screenshot` で取得（`$CARGO_TARGET_DIR/gui_e2e_poc/`）。
6. HEAD + `git status --porcelain` の before/after 一致で無変更を assert。

## 動いた API（locked gpui rev `90b3aa0b…`）

- `gpui::VisualTestAppContext`（`app/visual_test_context.rs`, macos + test-support gated）。
  - `with_asset_source(platform, Arc<dyn AssetSource>)` — **`KagiAssets` を渡すと
    bundled font / SVG が実際に描画される**（no-op asset source だと icon が出ない）。
  - `open_offscreen_window(size, build_root)` — window を (-10000,-10000) に置き不可視のまま
    実コンポジタで描画。build_root は `App` 前提の実 KagiApp 構築 closure をそのまま流用可。
  - `simulate_keystrokes(win, "cmd-j")` / `dispatch_action(win, action)` /
    `simulate_click(...)` — 入力・Action 注入。
  - `run_until_parked()` / `advance_clock(dur)` — 決定論的 settle。
  - `capture_screenshot(win) -> image::RgbaImage`（内部 `Window::render_to_image`、
    Metal texture readback。**Screen Recording 権限も可視 window も不要**）。
- `gpui_platform::current_platform(false) -> Rc<dyn Platform>` — 実 MacPlatform を供給。
  これは test-support 非依存で常時 public。

実行前の描画は確認済み: mount 後の first render で
`[kagi] commit list rows: 2` / `sidebar:` / `statusbar:` 等の contract 行が実際に emit され、
**layout + paint が本物のパイプラインで走っている**ことを裏付けた。

## 正しいテスト境界（依存の証明）

test-support を **release binary に混入させない**配線は成立した:

- ルート `Cargo.toml` の `[dev-dependencies]` に
  `gpui = { git = "...zed", features = ["test-support"] }` を追加（source は通常 dep と同一
  = cargo が 1 crate に unify、feature 集合だけが target 種別で変わる）。
- ルート package が edition 2021 かつ `resolver` 明示なし → **feature resolver v2** が有効で、
  dev-dep の feature は通常 build に unify されない。
- 実測（`cargo tree -i gpui --target aarch64-apple-darwin`）:
  - `-e no-dev`（production）: `default,font-kit,wayland,windows-manifest,x11`（**test-support なし**）
  - `-e all`（dev 含む）: 上記 + `backtrace,leak-detection,proptest,test-support`
- `cargo build -p kagi`（production）は緑。`bash ci/check-loc.sh` 緑。`cargo fmt --all --check` 緑。

## 何が動かなかったか（正確なブロッカー）

**`cargo test` の libtest harness はテスト関数を worker thread で走らせる**
（`--test-threads=1` でも同じ）。`open_offscreen_window` 内の AppKit `NSWindow` 生成が
main thread 以外だと ObjC 例外を投げ、`fatal runtime error: Rust cannot catch foreign
exceptions` → **SIGABRT** で落ちる。中断は first render（contract 行 emit）**の直後・
window open の戻り前**。これは upstream zed も既知（`gpui_platform` の
VisualTestAppContext テストが全て `#[ignore]`、コメントに "Requires macOS main thread …
window creation fails on test threads"）。

つまり **driver 自体は健全**で、唯一の障害は「実行 host が main thread でない」こと。

full-green に必要な main thread host = `harness = false` の独立ターゲット（自前 `fn main`
= プロセス entry の main thread で走る）。しかしそれは `tests/` に置かれ **`kagi` lib** を
link するが、**現状 lib は `ui` / `KagiApp` を公開していない**（`src/lib.rs` は graph/remote/
update/APP_ID のみ）。かつ `ui` は `crate::single_instance`（bin-root）に 1 箇所依存する
（`src/ui/tabs.rs:737`）。よって app root は **bin 専用**で、test-support ターゲットから
mount するには意図的な seam が要る = これは issue の論点 #1 そのもの。

新 `KAGI_*` env hook で main() から起動する手もあるが **スコープ外**（issue §7）なので採らない。

## 採用提案（team 合意事項）

1. **配線**: `gpui/test-support` は `[dev-dependencies]` のみ。resolver v2 前提を崩さない
   （ルートに `resolver = "2"` を明示追記しておくと将来の edition/member 変更に対して堅牢）。
2. **seam（論点 #1 の回答）**: 実行を main thread に載せるため、app root を test-support
   ターゲットから mount 可能にする最小の seam を切る。候補:
   - (a) `KagiApp` 構築 + offscreen mount を行う薄い `pub` エントリを、`single_instance`
     依存を外した上で lib（もしくは新 `kagi-app` crate、ADR-0077 の 4 層に沿う）へ移す。
   - (b) それを呼ぶ `harness = false` の main-thread runner（`tests/visual_poc.rs` か
     専用 bin）を置く。zed の `visual_test_runner` が先例。
   本 PoC の `#[ignore]` test は seam 成立までの「driver 生存」証跡として残す。
3. **semantic driver の範囲**: #354（role/name/accessible action）成立までは、本 lane は
   「registered `Action` + keybinding + observable state assert」を semantic 境界とする。
   座標 click は fallback に留め public contract にしない。合否は決定論的 assertion が決め、
   screenshot は review/triage 信号（vision-only を merge oracle にしない）。
4. **artifact**: result JSON + stdout/stderr（`[kagi]` 行）+ before/after PNG を bundle 出力。
   本 PoC は PNG を `$CARGO_TARGET_DIR/gui_e2e_poc/` に吐く。schema/retention は別途。

## CI 可能性

- **GitHub-hosted macOS runner を required gate にはしない**（issue の合意 #4 と一致）。
  実 Metal + main-thread window が要るため、まず primary macOS / self-hosted の
  evidence lane に留める。runner 信頼性を実測してから required 化を判断。
- `cargo test --workspace`（既存契約）は無改変で緑のまま: 追加は dev-dep 1 本と
  `#[cfg(all(test, target_os="macos"))] #[ignore]` module のみで、既存テストの
  コンパイル/実行経路に影響しない。visual PoC は明示 `--ignored` でしか走らない。
- LOC ratchet: `src/ui/mod.rs` の baseline を 4190→4193 に更新（module 宣言 3 行）。

## ファイル

- `src/ui/gui_e2e_poc.rs`（PoC test）
- `src/ui/mod.rs`（`#[cfg(all(test, target_os="macos"))] mod gui_e2e_poc;`）
- `Cargo.toml`（`[dev-dependencies] gpui = { …, features=["test-support"] }`, `image`）
- `ci/loc-baseline.txt`（mod.rs 4193）
