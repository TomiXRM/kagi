# OpenLogi 流用・学び調査(gpui アプリの .app 配布と実装パターン)

- 調査日: 2026-06-13 / 調査者: research subagent
- 対象: `AprilNEA/OpenLogi`(`/Users/tomixrm/Dev/sandbox/OpenLogi`、workspace version 0.6.9)— gpui + gpui-component で実装された Logitech HID++ 周辺機器の常駐ユーティリティ(Logi Options+ の代替)。**read-only 調査(一切変更せず)**。
- ライセンス(原文確認済):
  - workspace 全 crate = **`MIT OR Apache-2.0`**(`Cargo.toml` `[workspace.package] license = "MIT OR Apache-2.0"`、各 crate は `license.workspace = true`。root `LICENSE-MIT`(`Copyright (c) 2026 AprilNEA`)/ `LICENSE-APACHE`(Apache 2.0 原文)併存)
  - vendored `crates/openlogi-hidpp`(crate 名 `hidpp`、`@lus` 由来)= **`0BSD`**(Cargo.toml で明示。MIT/Apache よりさらに寛容、表記義務なし)
  - **GPL / AGPL / FSL は workspace 内に皆無**(`grep -riE 'GPL|AGPL'` ヒットなし)
  - gpui / gpui_platform = zed-industries/zed の git main(**Apache-2.0**。crate 単位確認は ADR-0034/zed 調査済)
  - **ブランド資産(`design/` のロゴ・アイコン)= All Rights Reserved**(`design/LICENSE`。MIT/Apache 対象外、無断利用不可)→ **kagi は流用不可**
- 関連: ADR-0031(外部コード流用ポリシー)、ADR-0001(gpui)、ADR-0006(gpui-component)、ADR-0008(terminal)、ADR-0029(command registry / menubar)、ADR-0036(color themes)。本調査から ADR-0038(app bundling)を起案。

## 前提(kagi 側の制約)

- kagi は gpui **crates.io 0.2.2** + gpui-component に依存(ADR-0001/0006)。OpenLogi は **zed git main の gpui + 別 crate `gpui_platform`** に依存しており、**API がズレる**。アプリ起動が `gpui_platform::application()`(OpenLogi)vs `gpui::Application::new()`(kagi 0.2.2)、`on_open_urls` / `on_reopen` / `observe_window_appearance` / `Theme::change` の所在・シグネチャも要再確認。**コード転写ではなく「手順とパターンの移植」が主**になる。
- kagi は `cargo run` 起動のみで **.app バンドル化が未着手**。本調査の最大の収穫はここ(観点1)。
- OpenLogi は **2 プロセス構成**(常駐 agent = hook/HID I/O/メニューバー + on-demand GUI)。kagi は単一プロセスの Git GUI なので、tray/hook/accessibility 周りはほぼ不要。一方で **config・single-instance・WindowRegistry・AppState global・theme sync・updater・i18n** はプロセス数非依存でそのまま移植できる。

---

## 観点1: リリース / 配布方法(.app バンドル化・署名・notarization・更新配布)

OpenLogi は **「`xtask`(workspace 内の Rust バイナリ)+ macOS 標準ツール + 少数の Homebrew CLI」**でフル自動の署名・notarized DMG パイプラインを持つ。kagi が最も参考にすべき領域。

### 1-1. macOS .app バンドル(`xtask/src/macos.rs`、`crates/openlogi-gui/Cargo.toml`)

- **`cargo-bundle`** で `.app` を生成。設定は GUI crate の `[package.metadata.bundle]` に集約:
  - `name = "OpenLogi"`、`identifier = "org.openlogi.openlogi"`、`category`、`short/long_description`
  - **`version` をあえて書かない** → cargo-bundle が crate version(workspace 統一)にフォールバック。「リテラルを書くと release-plz の bump で古くなる」
  - `osx_minimum_system_version = "13.0"`、`osx_url_schemes = ["openlogi"]`(deep-link)、`icon = ["icon/AppIcon.icns"]`、`resources = ["assets/**/*"]`
- **アイコン生成**(`generate_macos_icns`): master `design/icon/openlogi.png`(1024²、squircle 焼き込み済)から **`sips`(ダウンスケール)+ `iconutil`(.icns 化)** のみ。**SVG レンダラ(rsvg/resvg)不要** — iconset 各サイズ(16/32/128/256/512 の 1x/2x)を sips で生成。両ツールとも macOS 標準。
- **`xcode_env()`**: `xcrun --show-sdk-path` で `DEVELOPER_DIR` / `SDKROOT` を解決し cargo に渡す(CI で SDK を確実に拾う)。
- **dev 専用バンドル**(`scripts/cargo-run-macos.sh`、`.cargo/config.toml` の `runner` に配線): `cargo run` 時に `target/dev/OpenLogi.app` を組み立て、ビルド済バイナリを **hardlink**(symlink 不可: `current_exe()` が realpath で symlink を target/debug に解決し bundle 連携が壊れる)。これで **開発中でも正しいアプリ名(メニューバー太字)・Dock アイコン・`openlogi://` URL scheme** が効く。`dev/Info.plist` は `.dev` サフィックス identifier で本番 LaunchServices 登録と衝突回避。**kagi にそのまま効く優先テクニック**(バンドルなし `cargo run` でも本物のアプリ体験)。

### 1-2. コード署名(inside-out / hardened runtime)

- `codesign --force --options runtime --timestamp --sign <identity>`(hardened runtime + secure timestamp)。`--deep` は**使わない**(deprecated、ネスト helper に独立署名を与えられない)。
- **inside-out 署名**: ネスト helper(後述 agent)を先に署名 → 外側 .app を署名(署名済 helper を封入)→ `codesign --verify --strict`。**helper に独立した署名 identity を与えることが、その TCC(Accessibility)許可がアップデートを跨いで残る鍵**。
- kagi は単一プロセスなので **ネスト helper は不要**。`codesign --options runtime --timestamp` のシングル署名 + `--verify --strict` だけで足りる。

### 1-3. notarization / staple / DMG(`dmg_macos`、`.github/workflows/release.yml`)

- **DMG 作成 = Homebrew の `create-dmg`**(背景画像・アイコン座標・app-drop-link・拡張子非表示を引数指定)。`--format ULMO`(LZMA、UDZO/zlib より ~20% 小さく macOS 10.15+ でマウント可)。背景 tiff は CDN から curl。
- **notarization = `xcrun notarytool submit … --wait`**(Apple ID + app-specific password + team-id)→ **`xcrun stapler staple` + `stapler validate`**。
- DMG 自体も `codesign --timestamp` で署名。

### 1-4. CI / リリースパイプライン(`.github/workflows/release.yml`、805 行 workflow.rs 群)

タグ `v*` push で起動(`workflow_dispatch` は publish なしのドライラン)。学ぶべき設計が多い:

- **マトリクスビルド**: macOS arm64(`macos-latest`)+ x86_64(`macos-15-intel`)、Windows x64/arm64、Linux amd64/arm64(ネイティブ runner、cross なし)。各 runner で `uname -m` を検証。
- **秘密情報は 1Password**(`1password/load-secrets-action`、`export-env: false`)に集約 → Apple 証明書/ID、Azure 署名、R2、minisign 鍵。**Validate ステップで未設定を即時 fail**(数分後に署名で落ちる前に)。
- **Apple 証明書 import** = `apple-actions/import-codesign-certs`。
- **Windows 署名 = Azure Trusted/Artifact Signing(OIDC フェデレーション、client secret 不要)**。`+crt-static`(VC++ Redist 非依存)、`windows_subsystem = "windows"`(release のみ、console window 抑止)。MSI = WiX 6 pin。
- **Linux = `nfpm`**(`.deb`/`.rpm`、SHA256 検証付き download)。udev rules / systemd user unit / desktop file を同梱(`packaging/linux/`)。
- **publish ジョブ**: `softprops/action-gh-release@v3` が release を **draft で作成 → 全 asset upload → 最後に publish へ反転**(アップロード中は mutable、"immutable release" 拒否を回避)。`needs.macos.result == 'success'` のみ必須にして **Windows leg の失敗が macOS リリースをブロックしない**(`!cancelled()` で暗黙の all-success gate を外す)。
- **更新配布(自動アップデータ)の static manifest**:
  - リリース artifact を **`minisign`** で署名(`.minisig` detached)。
  - `xtask generate-updater-manifest` が `dist/*.dmg` を走査して **`latest.json`**(schema_version / app_id / version / channel / per-asset の url / signature_url / sha256 / size / minimum_os_version)を生成。各 DMG に対応する `.minisig` がないと fail。
  - **Cloudflare R2** にアップロード: `releases/<tag>/`(immutable cache)+ `channels/stable/latest.json`(no-cache)。GitHub Release が最後に publish される。
  - manifest URL / minisign 公開鍵は **`option_env!` でビルド時にバイナリへ焼き込む**(`OPENLOGI_UPDATE_MANIFEST_URL` / `OPENLOGI_UPDATE_MINISIGN_PUBLIC_KEY`)。
- **Homebrew cask** = publish 後に `repository-dispatch` で別 repo(`AprilNEA/homebrew-tap`)へ通知し autobump。

> kagi 示唆: **「xtask に bundle/dmg ロジックを置き、CI はそれを呼ぶだけ」**という構造が最も真似しやすい。ローカルでもタグなしでも同じコマンドで再現できる。署名・notarization・R2 アップデータは MVP では後回し可だが、**最初から `cargo run -p xtask -- bundle-macos` 相当を用意し、未署名 .app をローカルで作れる状態**にしておくと配布検討が一気に楽になる。

---

## 観点2: gpui アプリ実装パターン

### 2-1. ウィンドウ管理(on-demand + マルチウィンドウ)

- **on-demand GUI**: `cx.on_window_closed` で「最後のウィンドウが閉じたら `cx.quit()`」(`main.rs:201`)。OpenLogi では agent が常駐するためこれで良い。**kagi は常駐 agent がないので、この quit-on-last-window はそのまま採用してよい**(macOS の標準的な「ウィンドウ全閉で終了」)。
- **`WindowRegistry`(gpui global)**(`windows/mod.rs`): slot ごとに `Option<WindowHandle<Root>>`(`main` / `settings` / `about` / `add_device` / `update_consent`)。`open_or_focus<V: AuxWindow>(...)` が**シングルトンを強制**(既存 handle の `activate_window()` が成功すれば focus、失敗時のみ新規)。dock 再オープン(`app.on_reopen`)も同 handle を focus し重複を防ぐ。
- **`AuxWindow: Render` トレイト**: `set_appearance_obs(Subscription)` のみ要求。各 aux window root が appearance 購読を保持。
- gpui-component の **`Root`** で view をラップして開く(`Root::new(view, window, cx)`)。

### 2-2. グローバル状態(AppState global)

- **`AppState` を gpui global**(`impl Global for AppState`)として 1 つだけ持つ(`state.rs`)。最初のウィンドウ前に `set_global`。
- 読み: `cx.try_global::<AppState>()`(不在を許容し "Connecting" な中立フレームを描く)。書き: `cx.update_global::<AppState, _>(...)`。view は `cx.observe_global::<AppState>(|_, cx| cx.notify())` を `Subscription` として保持し再描画。
- **設計の要点**(kagi にそのまま効く):
  - 接続状態を**単一 enum**(`AgentLink { Connecting / Unreachable / OutdatedGui / Ready(..) }`)で持ち、`bool`/`Option` のミラーフィールド散在を避ける → 描画が部分的に不整合な状態を**型で排除**。
  - **ナビゲーションは view-local enum**(`AppView` 内の `enum Route`、AppState には置かない)。「gpui にルータはない、ナビは小さな view-local enum で十分」。
  - config を触る変更は必ず `save_atomic()` + agent へ IPC `ReloadConfig`(kagi は IPC 不要なので保存のみ)。

### 2-3. グローバルイベント / 権限(マウスフック・Accessibility)

> **kagi はマウスフック・Input Monitoring・Accessibility(TCC)を必要としない見込み** → この subsystem は**丸ごとスキップ可**。以下は将来 global hotkey 等で必要になった時の参照。

- 権限は**プロンプトを出さずに照会**: Input Monitoring = `IOHIDCheckAccess`(IOKit C FFI)、Bluetooth = `+[CBCentralManager authorization]`、Accessibility = `AXIsProcessTrusted`(`crates/openlogi-hook`)。
- **TCC は署名 identity でキーされる**ため、CGEventTap を実際に持つ agent 本体がプロンプトを出す必要がある(GUI が出すと GUI が許可され、agent の tap は失敗し続ける)。→ 観点1 の inside-out 署名 + ネスト helper の動機。
- System Settings deep link: `open x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility`。**この deep-link 手法と `AnyClass::get(c"...")`(クラス不在で panic せず degrade)は単独で再利用価値あり**。

### 2-4. メニューバー / tray(NSStatusItem)

- **gpui に status-bar API はない** → `objc2` で手書き。OpenLogi では**常駐 agent** 側(`crates/openlogi-agent/src/status_item.rs` / `tray.rs`)に置く。
  - `objc2` 0.6 + framework crates 0.3。所有は **`Retained<T>`**(Drop で release。旧 `cocoa`/`objc` 0.x の CFString leak 回避 = issue #99)。スレッド親和性は型で表現(`NSMenu`/`NSMenuItem` は `MainThreadOnly`、`MainThreadMarker` 要求)。
  - アクションターゲットは `define_class!` でサブクラス生成、`#[unsafe(method(...))]`。AppKit はターゲットを**弱参照**するので tray 構造体が retain し続ける。
  - tray のアクションは GUI を直接呼ばず **`open openlogi://show` 等の URL を発行**(Apple Event で GUI に配送)。agent は `NSApplication` を `ActivationPolicy::Accessory`(Dock アイコンなし)で main thread に立て、tokio は別スレッド。
- **`#[expect(unsafe_code)]` の局所 opt-in**: workspace は `unsafe_code = "deny"`。FFI モジュールだけ module-level `#![expect(unsafe_code, reason = "...")]` で opt-in し、本当に unsafe な objc2 呼び出しのみ `unsafe { }` + `// SAFETY:`。**kagi の安全方針(unsafe 最小化)と相性が良い手本**。
- kagi 示唆: kagi がメニューバー常駐を採るなら単一プロセス GUI 内に直接 `NSStatusItem` を置けばよく(2 プロセス分割は不要)、`status_item.rs` の `Retained<T>` ラッパは**ほぼそのまま移植可**(MIT/Apache)。ただし MVP では不要。

### 2-5. 起動構成

- **single instance**(`crates/openlogi-core/src/single_instance.rs`): `fs4` の advisory exclusive file lock(`try_lock`、非ブロッキング)。`acquire("openlogi.lock")` が config dir 下にロックファイルを作り、guard を `main` が保持(プロセス終了で OS が解放 → stale 掃除不要)。`WouldBlock` → 丁寧に exit 0。**役割ごとに別ファイル**(gui=`openlogi.lock` / agent=`agent.lock`)。**kagi は MIT/0BSD なのでほぼ verbatim 移植可**。
- 起動は **HID 列挙でブロックしない**設計(watcher を別スレッドで spawn、最初のフレームを即描画)。kagi の repo ロードも async(ADR-0030)なので思想一致。
- URL scheme: `app.on_open_urls(...)` で cold/warm 両対応の deep-link 受信。

### 2-6. 設定永続化(config)

- **TOML**、場所は **`$XDG_CONFIG_HOME/openlogi/config.toml`**(既定 `~/.config/openlogi/config.toml`)。**macOS でも XDG**(`~/Library/Application Support` をあえて使わない)。`paths.rs` に `config_path` / `config_dir` / `xdg_config_home`(絶対パスのみ honor)。data dir = `~/.local/share/openlogi`。
- `config.rs`: `Config::load_or_default()`(欠損→Default、エラーにしない)、`Config::save_atomic()`(`.tmp` へ書き → fsync → rename、Unix mode `0600`)。`serde` + `toml`。
- **schema バージョニング**: `SCHEMA_VERSION = 2`。loader は ≤ current を受理(古いものは serde shim で migrate し次回保存で自己修復)、新しいものは loud に reject。
- `AppSettings` は全フィールド `#[serde(default)]` + `skip_serializing_if`(既定値ならブロックごと省略 → クリーンな TOML)。フィールド例: `launch_at_login` / `check_for_updates`(既定 false=プライバシー)/ `update_prompt_seen` / `show_in_menu_bar`(既定 true)/ `language: Option<String>`(None=システム追従)。
- **launch-at-login(macOS)**: `~/Library/LaunchAgents/org.openlogi.agent.plist` を文字列レンダリングして書く/消す(`RunAtLoad=true`、`KeepAlive={SuccessfulExit:false}`、exe パスを XML escape)。**冪等 reconcile**(内容差分時のみ書く)。TODO で `SMAppService` へ移行予定と明記。Linux=systemd user unit、Windows=`HKCU\...\Run`。
- kagi 示唆: **`paths.rs` + `config.rs`(atomic save / schema_version gate / serde-default + skip_serializing_if)はほぼそのまま Port 候補**。ただし kagi が macOS ネイティブ感を重視するなら config 置き場は要検討(OpenLogi は cross-platform 一貫性のため XDG を選択、これは設計判断であり唯一解ではない)。

### 2-7. 更新配布(updater)

- 外部 crate **`gpui-updater`**(git, AprilNEA, tag `v0.0.4`)。`Entity<Updater>` を `SharedUpdater` global として publish。`StaticManifestSource::new(MANIFEST_URL)` + `EngineConfig::new(version).verification(Verification::Strict).minisign_public_key(key)`。
- 検証 = manifest の SHA-256 + minisign 署名。**fail-closed**(鍵なしの dev ビルドは install せず check がエラー)。
- **opt-in consent**(既定 off、README の "no telemetry/auto-poller" 約束)。初回起動時だけ consent ウィンドウ(`windows/update_consent.rs`、`update_prompt_seen` で 1 回のみ)。opt-in 時は**起動時に 1 回だけ check**(ポーリングなし・自動 DL なし)。手動 "Check for Updates" はいつでも可。
- manifest URL / 公開鍵は `option_env!` でビルド時注入。
- kagi 示唆: kagi が将来自動更新を持つなら **この opt-in consent + fail-closed strict verify + single shared `Entity<Updater>` global** はそのまま設計を移植できる。`gpui-updater` 自体の採否は別途ライセンス・gpui バージョン互換確認が要る(後述テーブル)。

### 2-8. i18n

- `rust-i18n`(`rust_i18n::i18n!("locales", fallback="en")`)。`locales/*.yml` をコンパイル時ロード。crate-local `macro_rules! tr!` が `t!` を **`gpui::SharedString`** にラップ(borrowed ヒットはコピーなし)。**English 文字列を msgid キー**にする。
- locale 解決: 明示設定 → `sys_locale::get_locale()` → `"en"`。BCP-47 を ~20 ロケールへ畳む(zh script/region、pt BR/PT 等を特別処理)。ライブ切替は `set_locale` 後に `cx.refresh_windows()` + `app_menu::rebuild(cx)`。
- **gpui-component 自身の widget 文字列も同じ rust_i18n global で localize される**(ただし gpui-component が bundle するロケール = en/zh-CN/zh-HK/it のみ)。
- kagi 示唆: i18n を入れるなら `tr!`→SharedString + English-as-key + 共有 resolve/activate は綺麗。MVP では不要。

---

## 観点3: コンポーネント構成(真似する/しない)

- **components/ は汎用プリミティブではなく「製品固有パネルの再エクスポート」**(`carousel` / `dpi_panel` / `smartshift_panel` / `lighting_panel`)。各パネルがローカル状態を持ち AppState 経由で協調。gpui-component の上に薄く製品 widget を載せる構成。
- **真似すべき構造**:
  - `WindowRegistry` global + `open_or_focus` + `AuxWindow` トレイト(シングルトン Settings/About)。
  - 単一 `AppState` global(`try_global` 読み / `observe_global` 購読 / `update_global` 書き)。
  - **接続/ロード状態を単一 enum で**(部分不整合を型排除)。
  - **ナビは view-local enum**(router を作らない)。
  - **platform FFI を `src/platform/` に隔離**し、`platform/CLAUDE.md` に objc2/`Retained<T>`/`MainThreadMarker`/`#[expect(unsafe_code)]` の規約を文書化(そのまま style guide として優秀)。
  - xtask に bundle/dmg/manifest を集約し、CI は薄く呼ぶだけ。
- **しない方がよい / kagi に不要な構造**:
  - **2 プロセス(agent + GUI)分割と tarpc IPC**: HID 常駐ハードウェア制御ゆえの構成。Git GUI には過剰、単一プロセスで足りる。
  - **gpui を zed git main + `gpui_platform` で追う**: kagi は ADR-0001 どおり **crates.io 0.2.2 を pin 継続**(再現性・安定性)。OpenLogi のコードを参照する際は API ズレに注意。
  - **マウスフック / Accessibility / Input Monitoring TCC**: 不要。
  - **ブランド資産の流用**: `design/` は All Rights Reserved、不可。

---

## 観点4: ライセンスと流用可否(ADR-0031 ゲート通過)

- **workspace コード = `MIT OR Apache-2.0`(原文 `LICENSE-MIT` / `LICENSE-APACHE` 確認済)** → ADR-0031 のライセンスゲートで **Port/Adopt 可**(NOTICE/著作権表記の保持義務を守る。MIT は著作権表示の保持、Apache は NOTICE がある場合の保持)。
- vendored `hidpp` = **`0BSD`** → 表記義務すらなし(ただし HID++ ドメイン依存なので kagi には無関係)。
- **GPL/AGPL/FSL の汚染なし**(grep ヒット 0)→ ADR-0031 の "FSL Competing Use" / "GPL 転写禁止" のいずれにも該当せず、**OpenLogi は kagi にとって最も流用安全な調査対象**(jj=Apache だが gix 依存、gitbutler=FSL、zed=GPL 混在 と比べ、ライセンス・依存・ドメインのいずれでも障壁が低い)。
- **ただし最大の障壁は「ライセンスではなく gpui バージョン」**: OpenLogi コードは zed-git gpui + `gpui_platform` 前提で、kagi(0.2.2)へ**そのままビルドは通らない**。→ 多くが **Port(API 合わせの移植)** か **Reimplement(パターン参照)** に倒れる。コード片の逐語コピーより**「手順・設計の移植」**が中心。
- **ブランド資産(`design/`)= All Rights Reserved** → アイコン・ロゴは**流用不可**。kagi は自前アイコンを用意する。

### ADR-0031 ⑩項目チェックリスト(主要候補の代表評価)

`paths.rs` / `config.rs`(設定永続化)を代表例に:
1. ライセンス: MIT OR Apache-2.0(原文確認済)→ Port 可
2. 依存 crate: `serde` / `toml` / `fs4`(single_instance)— いずれも kagi で問題なし。gix/git2/tokio/DB/Tauri 結合なし
3. UI/ロジック分離: config は `openlogi-core`(UI 非依存の純ロジック crate)。分離良好
4. 単独 crate 切り出し: 容易(core はそもそも UI から独立)
5. gpui 統合: config は gpui 非依存 → バージョン問題の影響を受けない(数少ない「コードごと移植しやすい」候補)
6. 流用 vs 再実装: **Port**(serde 構造体名・XDG ポリシーは kagi 流に調整)
7. MVP or later: バンドル化は MVP 後半、config schema は MVP で必要
8. テスト戦略: manifest 生成にユニットテストあり(`xtask/src/manifest.rs` の `#[cfg(test)]`)— atomic save / schema migrate も同様にテスト可
9. 既存アーキ影響: kagi の oplog(JSONL)/ git2 単一 backend と無衝突。config 層は独立
10. メンテリスク: 低(標準 crate のみ、外部 git 依存なし)

---

## 観点5: kagi への提案リスト(Adopt/Port/Reimplement/Study/Reject)

| # | 候補 | 分類 | 理由 | コスト | リスク |
|---|---|---|---|---|---|
| 1 | **xtask ベースの bundle/dmg/manifest 構造**(`cargo run -p xtask -- bundle-macos`) | **Reimplement** | MIT だがロジックは cargo-bundle/sips/iconutil/create-dmg/notarytool の手順そのもの。kagi 用に書き起こす(コードより手順移植) | 中 | 低 |
| 2 | **`cargo-bundle` + `[package.metadata.bundle]`**(version 省略→crate fallback、url_schemes、resources) | **Adopt** | 標準ツール。設定パターンをそのまま採用 | 小 | 低 |
| 3 | **dev 用 .app ラッパ**(`scripts/cargo-run-macos.sh` + `.cargo` runner、hardlink + dev Info.plist) | **Port** | `cargo run` でも本物のアプリ名/Dock/URL scheme。kagi に即効。MIT なのでスクリプトを移植・改変 | 小 | 低 |
| 4 | **アイコン生成パイプライン**(master PNG → sips → iconutil、SVG レンダラ不要) | **Adopt** | macOS 標準のみ。kagi 自前アイコンに適用(OpenLogi のアイコン画像は流用不可) | 小 | 低 |
| 5 | **inside-out codesign + hardened runtime + notarytool/stapler** | **Adopt**(手順) | Apple 標準フロー。kagi は単一プロセスなのでネスト helper 部分は削れる(シングル署名でよい) | 中 | 中(証明書・Apple Developer 登録が前提) |
| 6 | **GitHub Actions リリース構造**(matrix・1Password secrets・validate gate・draft→publish・部分公開の非ブロック) | **Study** → 後に Port | 設計が秀逸。kagi の CI に段階移植。1Password 部分は kagi の secret 管理に合わせ調整 | 中 | 低 |
| 7 | **設定永続化 `config.rs`/`paths.rs`**(atomic save・schema_version gate・serde default + skip_serializing_if) | **Port** | gpui 非依存・MIT。kagi の型に合わせ移植。最も移植容易な候補 | 小 | 低 |
| 8 | **single instance**(`fs4` advisory file lock、役割別ロックファイル) | **Port** | gpui 非依存・MIT・小コード。ほぼ verbatim | 小 | 低 |
| 9 | **`WindowRegistry` global + `open_or_focus` + `AuxWindow` トレイト**(シングルトン aux window) | **Port** | gpui パターン。0.2.2 へ API 合わせ移植(`WindowHandle`/`activate_window` の確認) | 小 | 低(gpui バージョン差) |
| 10 | **`AppState` global**(単一 enum 状態・`observe_global` 購読・view-local route) | **Reimplement** | 設計言語を kagi 流に。kagi は既に Command Registry 等あり、状態モデルの指針として採用 | 小 | 低 |
| 11 | **theme sync**(gpui-component `Theme::change` + `observe_window_appearance` + 保持 Subscription、独自 `Palette`) | **Port** | ADR-0036 と整合。0.2.2 + kagi の gpui-component rev で API 確認の上移植 | 小 | 中(gpui-component rev 差) |
| 12 | **platform FFI 隔離 + `platform/CLAUDE.md` 規約**(`Retained<T>`/`MainThreadMarker`/`#[expect(unsafe_code)]`) | **Adopt**(規約) | kagi の unsafe 最小化方針と一致。将来 objc2 を触る時の style guide | 小 | 低 |
| 13 | **NSStatusItem メニューバー実装**(`status_item.rs` の objc2 ラッパ) | **Study** | kagi が常駐メニューバーを採るなら verbatim 近く移植可。MVP では不要 | 中 | 中(objc2 + gpui 版差) |
| 14 | **updater**(opt-in consent + fail-closed strict minisign verify + `latest.json`/R2 + `option_env!` 注入) | **Study** → 必要時 Port | 設計は移植可。`gpui-updater` crate 採否は別途ライセンス/gpui 版互換確認が要る | 中 | 中 |
| 15 | **i18n**(rust-i18n + `tr!`→SharedString + English-as-key) | **Study** | 綺麗だが kagi MVP に i18n 要件なし。将来の指針として記録 | 小 | 低 |
| 16 | **2 プロセス agent + tarpc IPC** | **Reject** | HID 常駐制御ゆえの構成。Git GUI に過剰、単一プロセスで足りる | — | — |
| 17 | **マウスフック / Accessibility / Input Monitoring TCC** | **Reject** | kagi に該当機能なし | — | — |
| 18 | **zed-git gpui + `gpui_platform` 追従** | **Reject** | kagi は crates.io 0.2.2 pin 継続(ADR-0001、再現性) | — | — |
| 19 | **ブランド資産(`design/` のアイコン/ロゴ)** | **Reject** | All Rights Reserved。流用不可 | — | — |

---

## .app 配布までの推奨手順(kagi 向け、具体ツール・コマンド)

OpenLogi の構造を kagi の単一プロセス・gpui 0.2.2 前提に縮約した推奨手順。**段階的**に進められる(各段で価値が出る)。

### Phase 0: ローカル開発体験(署名不要・即効)
1. `crates/<gui>/icon/AppIcon.icns` を用意。master `assets/icon.png`(1024²、角丸焼き込み)から:
   ```sh
   # iconset を作って icns 化(sips/iconutil は macOS 標準)
   for s in 16 32 128 256 512; do
     sips -z $s $s icon.png --out AppIcon.iconset/icon_${s}x${s}.png
     sips -z $((s*2)) $((s*2)) icon.png --out AppIcon.iconset/icon_${s}x${s}@2x.png
   done
   iconutil -c icns AppIcon.iconset -o AppIcon.icns
   ```
2. **dev `.app` ラッパ**を `scripts/cargo-run-macos.sh` として移植し `.cargo/config.toml` の `[target.'cfg(target_os="macos")'] runner = "..."` に配線。ビルド済バイナリを **hardlink**(symlink 不可)で `target/dev/kagi.app/Contents/MacOS/` に置き、`dev/Info.plist`(`.dev` identifier)を Resources に。→ `cargo run` で正しいアプリ名・Dock アイコン。

### Phase 1: 未署名 .app / DMG(配布の足場)
3. GUI crate に `[package.metadata.bundle]`(name / identifier / category / `icon=["icon/AppIcon.icns"]` / `osx_minimum_system_version` / version は書かない)。
4. `xtask` クレートを追加し `bundle-macos` サブコマンド: `cargo install cargo-bundle --locked` →（`xcrun --show-sdk-path` で `SDKROOT` 解決）→ `cargo bundle --release`。
5. `dmg-macos` サブコマンド: Homebrew `create-dmg`(`brew install create-dmg`)で背景・アイコン座標・`--app-drop-link`・`--format ULMO`。
   ```sh
   cargo run -p xtask -- bundle-macos   # → target/release/bundle/osx/kagi.app
   cargo run -p xtask -- dmg-macos      # → target/release/kagi.dmg(未署名)
   ```
   → この時点で「ダウンロードして Applications にドラッグ」できる(Gatekeeper は警告するが動く)。

### Phase 2: 署名 + notarization(Gatekeeper クリーン)
6. Apple Developer Program 登録 → "Developer ID Application" 証明書。
7. 署名(単一プロセスなら inside-out 不要):
   ```sh
   codesign --force --options runtime --timestamp --sign "$IDENTITY" kagi.app
   codesign --verify --strict --verbose=2 kagi.app
   ```
8. DMG 署名 → notarize → staple:
   ```sh
   xcrun notarytool submit kagi.dmg --apple-id "$APPLE_ID" --password "$APP_PW" --team-id "$TEAM" --wait
   xcrun stapler staple kagi.dmg && xcrun stapler validate kagi.dmg
   ```

### Phase 3: CI 自動化
9. `.github/workflows/release.yml`: タグ `v*` トリガ、arm64(`macos-latest`)+ x86_64(`macos-15-intel`)マトリクス。秘密情報は GitHub Encrypted Secrets か 1Password。証明書 import = `apple-actions/import-codesign-certs`。
10. publish = `softprops/action-gh-release@v3`(draft → upload → publish 反転で immutable 拒否回避)。SHA256SUMS 生成。

### Phase 4(任意): 自動アップデータ + Homebrew
11. リリース artifact を `minisign` 署名、`latest.json`(per-asset url/sha256/signature_url)を生成、静的ホスト(R2/S3/GitHub Pages)へ。manifest URL と minisign 公開鍵を `option_env!` でビルド時注入。
12. アプリ側は opt-in consent + `Verification::Strict` の fail-closed チェック(`gpui-updater` 採否は要ライセンス/gpui 版互換確認)。
13. Homebrew cask は publish 後に tap repo へ `repository-dispatch`。

> **最小実用ライン**: Phase 1 までで「未署名だが配布可能な .app/.dmg」が `cargo run -p xtask` で再現できる。kagi の MVP 配布はここを目標にし、Phase 2 以降は Apple Developer 登録後に。

---

## 確認できなかった事項 / 注意

- OpenLogi の gpui は **zed git main + `gpui_platform`**。本調査のコード参照箇所(`gpui_platform::application()`、`on_open_urls`、`observe_window_appearance`、`Theme::change` 等)は **kagi の gpui 0.2.2 でシグネチャ・所在が異なり得る**。Port 時は各 API を docs.rs/gpui/0.2.2 と kagi の gpui-component rev で要確認。
- `gpui-updater`(AprilNEA 製、git tag v0.0.4)の**ライセンスは未確認**(OpenLogi 本体とは別 repo)。採用時は原文確認が必須。
- `cargo-bundle` / `create-dmg` / `nfpm` / WiX のバージョン挙動差は OpenLogi が pin で対処済。kagi も pin 推奨。
- Apple Developer Program(年額)・Apple ID app-specific password・Developer ID 証明書は Phase 2 以降の前提。kagi 側で未取得なら Phase 1(未署名)が当面の到達点。
</content>
</invoke>
