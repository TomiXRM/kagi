---
name: verify
description: kagi の変更をランタイム検証する手順 — fixture repo 生成、GUI 起動、single-instance ソケット経由の実操作、watcher 経由の reload 発火、web/Playwright ハーネス実行。
---

# kagi verify — ランタイム検証レシピ

## Fixture repo

```bash
bash scripts/make_fixture.sh /tmp/kagi-vfx-a   # → /tmp/kagi-vfx-a/repo(最終行にパス出力)
```
branches(main / feature/one / feature/two)、merge commit、tag、stash 1件、dirty WT、origin(bare)付き。
DEST は /tmp 配下のみ許可。既存パスには上書き拒否。

## GUI 起動(ユーザーセッションを汚さない)

```bash
KAGI_NO_RESTORE=1 ./target/debug/kagi /tmp/kagi-vfx-a/repo 2> /tmp/kagi-live.log &
```

- `KAGI_NO_RESTORE=1` — settings.json のセッション保存/復元を無効化(必須。付けないと fixture タブがユーザーのセッションに保存される)。
- `KAGI_NO_SINGLE_INSTANCE=1` を**付けない**こと(下のソケット制御が使えなくなる)。
  ただしユーザーの kagi が既に起動中なら逆にそこへ forward されてしまう — `pgrep -l kagi` を先に確認。
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
- **クリックは Terminal.app プロキシ経由**(Terminal に Accessibility 付与済み、SSH 直は不可)。
  常駐プロキシ: `.command` ファイルに `tail -f /tmp/kagi-proxy-queue | while read s; do zsh $s > $s.out 2>&1; touch $s.done; done`
  を書いて `open -a Terminal` で起動 → SSH 側からスクリプトパスをキューに echo して結果ファイルを待つ。
- **癖: 1 プロキシスクリプト内で cliclick を複数回起動すると最初の 1 発しか効かない** — 1 スクリプト = 1 cliclick 起動に分割する。
  ただし **1 回の cliclick 起動内のチェーンは OK**: `cliclick m:X,Y w:300 c:.`(移動→待ち→現在位置クリック)。
- **gpui のボタンは「teleport+クリック同時」では反応しない**(タブ行は反応する)。ホバーが先に必要:
  `cliclick m:X,Y` で載せてから(hover 状態をスクショで確認可)、`cliclick c:.` で踏む。上のチェーン形が確実。
- diff を起動時に自動で開く: `KAGI_SELECT_FIRST=1 KAGI_OPEN_FIRST_FILE=1`(headless モードになり single-instance は無効)。
- 座標系: cliclick = ポイント(1920×1080)、スクショ = 物理 px(3840×2160、スケール 2)。
  較正は `cliclick m:X,Y` → `screencapture -C` でカーソル位置を目視。
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
