---
name: verify
description: kagi の変更をランタイム検証する手順 — fixture repo 生成、GUI 起動、single-instance ソケット経由の実操作、watcher 経由の reload 発火、web/Playwright ハーネス実行。
---

# kagi verify — ランタイム検証レシピ

## Native GUI E2E（コンパイル時 opt-in）

通常の workspace テストは GUI runner をコンパイルしない。macOS で明示実行する:

```bash
KAGI_GUI_E2E=1 \
  cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
```

`gui-e2e` が `gpui/test-support` と recovery シナリオをコンパイル対象にする。
環境変数は実行の許可であり、feature の代わりにはならない。
同じ worktree で他の cargo が動いていれば完了を待つ（CLAUDE.md「Build hygiene」）。
各 worktree は既定の独立した `target/` を使い、`CARGO_TARGET_DIR` や
`build.target-dir` で共有先へ向けない。

## Fixture repo

```bash
bash scripts/make_fixture.sh /tmp/kagi-vfx-a   # → /tmp/kagi-vfx-a/repo(最終行にパス出力)
```
branches(main / feature/one / feature/two)、merge commit、tag、stash 1件、dirty WT、origin(bare)付き。
DEST は /tmp 配下のみ許可。既存パスには上書き拒否。

## GUI 起動(ユーザーセッションを汚さない)

```bash
KAGI_NO_RESTORE=1 KAGI_LOG_DIR="$(mktemp -d)" KAGI_NO_SINGLE_INSTANCE=1 KAGI_NO_ACTIVATE=1 \
  ./target/debug/kagi /tmp/kagi-vfx-a/repo 2> /tmp/kagi-live.log &
PID=$!
```

- `KAGI_NO_RESTORE=1` — settings.json のセッション保存/復元を無効化(必須。付けないと fixture タブがユーザーのセッションに保存される)。
- `KAGI_LOG_DIR=$(mktemp -d)` — **これも必須**。`~/.kagi`(oplog / trust / worktree_ports / settings)への書き込みを隔離する。
  `KAGI_NO_RESTORE=1` だけでは不十分で、**`window_size` はユーザーの `~/.kagi/settings.json` に書かれる**(2026-09-05 実測)。
  recent_repos / session_repos は `KAGI_NO_RESTORE=1` で汚れないことも実測済み。
- `KAGI_NO_SINGLE_INSTANCE=1` — ユーザーの kagi が起動中なら**必須**(付けないとそちらへ forward される)。
  ただし付けると下のソケット制御は使えない。これは通常 GUI の起動ルーティング指定であり、
  headless フックではないため、trusted な worktree `command` step はこれだけでは抑止されない。
  `pgrep -l kagi` を先に確認して選ぶ。
- `KAGI_NO_ACTIVATE=1` — この明示的な起動時の `activate(true)` を抑止し、ユーザーの前面アプリやキー入力を奪わない。headless フックではない。single-instance の focus 転送には適用されないので、検証では `KAGI_NO_SINGLE_INSTANCE=1` と併用する。
- 検証は stderr の `[kagi] …` klog 契約行を tail して行う。

## 実操作(クリックの代替): single-instance ソケット

起動中インスタンスへ 2 回目の起動コマンドが forward される(ADR-0102)。これで実タブ操作を駆動できる:

```bash
KAGI_NO_RESTORE=1 ./target/debug/kagi /tmp/kagi-vfx-b/repo   # → 新タブ open + switch_repo
# ログ: single-instance: open tab … / tab-switch: <name> cached=yes|no / tabs: n=… / tab-load: …
```

初回タブ(CLI 引数で開いた分)は tab_cache 未投入なので最初の switch-back は `cached=no` が正常。

## reload / watcher 発火(実 git イベント)

```bash
echo x >> /tmp/kagi-vfx-a/repo/a.txt          # → watcher: working-tree changed — refreshing WIP
git -C /tmp/kagi-vfx-a/repo commit --allow-empty -m x   # → refreshed (external change) + rows 増加
```
反映まで watcher の debounce があるので 2〜3 秒待って tail する。

## 起動時のみのヘッドレスフック(src/headless.rs)

KAGI_SELECT_FIRST / KAGI_JUMP=<branch> / KAGI_CONTEXT_MENU=<row> / KAGI_COMPARE_HEAD /
KAGI_COMPARE_WT / KAGI_BOTTOM_PANEL / KAGI_TERMINAL / KAGI_MENU_DUMP / KAGI_PULL 等。
これらは launch 時に一度だけ適用(実行中インスタンスへは送れない)。
**solo(toggle_branch_solo)にはフックが無い** — branch バッジのコンテキストメニューの実クリックが必要。

## OS レベルのクリック / スクリーンショット(TCC 権限)

このマシンは SSH セッション(責任プロセス = sshd 系)。2026-07-17 時点の実測:

- **スクショ/録画は SSH 直で OK**(`/usr/libexec/sshd-keygen-wrapper` に画面収録を付与済み)。
  `screencapture -x shot.png` / 動画は `screencapture -v -C -V <sec> out.mov`(-C でカーソル込み)。
- **ウィンドウ単体で撮る(他アプリ・デスクトップを写さない)**: `screencapture -o -l<CGWindowID>`。
  ID は swift で取る(`/usr/bin/swift` あり、pyobjc は無い):
  ```bash
  # scripts/winid.swift: owner 名で layer 0 のウィンドウ ID を print
  WID=$(swift scripts/winid.swift modal_shot); screencapture -x -o -l"$WID" out.png
  ```
  `CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)` を
  owner 名で絞り、`kCGWindowLayer == 0` かつ 200×200 以上を選ぶ。`-o` は影を省く。
- **`screencapture -R<x,y,w,h>`(矩形指定)は使えない** — `could not create image from rect`。
  `-R0,0,400,300` でも失敗するので座標の問題ではない。全画面 + `sips -c H W --cropOffset Y X` で切る。
- **モーダルを開いた状態を撮る**: `cargo run --example modal_shot -- <repo> amend|discard`(#454)。
  plan を組んで `set_*_modal` してから `run_app` するだけの read-only な例。
  runner(`VisualTestAppContext`)側では**撮れない** — ウィンドウがオフスクリーンに開かれるため
  `screencapture` から見えず、`activate(true)` を足しても前面のアプリが撮れる。
  runner 内の `capture_screenshot` は pinned gpui に `render_to_image` の Mac 実装が無いので skip される。
- **cliclick の代替は `scripts/pidclick.swift`**。`CGEventPostToPid` で指定 PID と window ID だけへ配送するため、ユーザーのポインタ、前面アプリ、キーボード focus を奪わない。責任プロセス（このマシンでは Terminal.app プロキシ）には Accessibility の TCC 許可が必要。
  ```bash
  swiftc scripts/pidclick.swift -o /tmp/pidclick
  /tmp/pidclick windows --pid "$PID"
  /tmp/pidclick --pid "$PID" --window-id "$WID" move 120 80
  /tmp/pidclick --pid "$PID" --window-id "$WID" click 120 80
  /tmp/pidclick --pid "$PID" --window-id "$WID" rclick 120 80
  /tmp/pidclick --pid "$PID" --window-id "$WID" key 53
  /tmp/pidclick --pid "$PID" --window-id "$WID" type 'filter text'
  ```
  `windows` 出力の第 1 列から対象ウィンドウの ID を選び、`WID` に設定する。
  座標はウィンドウ左上からの logical point。`click` / `rclick` は hover 用 `mouseMoved` を先に送る。
  `type` は現在のキーボードレイアウトから virtual keycode を得るため、レイアウト依存である。IME、dead key、複数 keycode が必要な文字、改行・Tab を含む文字列には対応しない。未対応文字列はイベントを送る前に失敗する。
- Terminal.app プロキシを使う場合は、`.command` ファイルに `tail -f /tmp/kagi-proxy-queue | while read s; do zsh $s > $s.out 2>&1; touch $s.done; done`
  を書いて `open -a Terminal` で起動 → SSH 側からスクリプトパスをキューに echo して結果ファイルを待つ。
- diff を起動時に自動で開く: `KAGI_SELECT_FIRST=1 KAGI_OPEN_FIRST_FILE=1`(headless モードになり single-instance は無効)。
- スクショは物理 px、pidclick は window-relative logical point なので、Retina の倍率を混同しない。
- ウィンドウ前面化は TCC 不要の裏技: **引数なし `kagi` を起動すると single-instance の focus 転送**で
  実行中インスタンスが `cx.activate(true)` する。
- Solo の操作: サイドバーの branch 行を**右クリック**(`branch-menu: open local <name>` が出る)→
  メニューの「Solo」をクリック → `solo: <name> rows=N (of M)`。解除はグラフ上部の「← Solo: <name>」チップ。

## web / Playwright ハーネス(ADR-0097)

UI ストーリーカタログ(kagi-domain 駆動、Backend/git2 なし)。アプリ本体のフロー検証には使えない。

```bash
rustup target add wasm32-unknown-unknown --toolchain nightly   # build-web.sh は +nightly を使う
cargo install wasm-bindgen-cli --version 0.2.123 --locked      # Cargo.lock の wasm-bindgen と同版
bash scripts/build-web.sh                                      # → crates/kagi-web/dist
cd e2e && npm install && npx playwright install chromium
npx playwright test                                            # 2 specs: boot / resize 1-frame settle
```

注意: `rustup target add --toolchain nightly-2024-10-31` では不十分(`+nightly` は
nightly-aarch64-apple-darwin に解決される)。config は `e2e/playwright.config.ts`。
webServer(python3 http.server :8899)と SwiftShader の WebGPU フラグは config が面倒を見る。

## 片付け

- アプリを終了(single-instance ソケットを塞いだままにしない)。
- fixture は /tmp 配下なので放置可。
