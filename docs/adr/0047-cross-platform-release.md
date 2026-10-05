# ADR-0047: クロスプラットフォーム配布(Phase 1 = 未署名、icon pipeline 込み)

- Status: Accepted(2026-06-13、ユーザー依頼「クロスプラットフォームで配布できるようにしたい。
  Apple の認証は取れていないけど、まあいいでしょう」)
- 関連: ADR-0038(macOS .app/DMG 設計 — 本 ADR で Phase 1 + CI を実施に確定。署名/notarization
  (Phase 2)は Apple Developer Program 取得まで保留)、docs/research/openlogi-learnings.md、ADR-0031

## Decision

### 対象プラットフォーム

| OS | 形態 | 備考 |
|----|------|------|
| macOS (arm64。x86_64 は 2026-06-13 ユーザー判断で対象外) | `.app` + `.dmg` | **ad-hoc 署名**(`codesign -s -`)。Gatekeeper は右クリック→開く案内を README に記載 |
| Linux (x86_64 / arm64) | `tar.gz`(bin + .desktop + icon)+ **AppImage zip** | 下記追補 |
| Windows | 対象外(将来) | gpui 0.2.2 の Windows 成熟度待ち。別 ADR |

### 追補(2026-06-13、ユーザー依頼): linux-arm64 + AppImage + 同梱インストールスクリプト

- **linux-arm64** を matrix に追加(`ubuntu-24.04-arm`)。GitHub の arm64 runner は
  **public repo では無料**、private のうちは起動しない可能性があるため当面 `continue-on-error`
  (落ちても release は進む。公開後に外す)
- **AppImage**(CANViewer の配布実績パターンを移植):
  `kagi_Linux-AppImage_<arch>.zip` = `Kagi-<arch>.AppImage` + `kagi.png` + `install_linux_desktop.sh`
  - AppImage は xtask `bundle-appimage` で AppDir(AppRun + .desktop + icon + bin)を組み、
    `appimagetool`(CI が公式 release から取得、`--appimage-extract-and-run` で FUSE 不要)で生成。
    ローカル macOS では AppDir レイアウト生成までを検証(appimagetool 不在ならスキップ)
  - lib の同梱(linuxdeploy)は Phase 1 ではしない(Rust 単一バイナリ。system の
    vulkan/xkbcommon 等に依存 — tar.gz と同条件)
- **install_linux_desktop.sh**(オフライン・curl なし): AppImage を `~/.local/bin/Kagi.AppImage` へ、
  icon を hicolor へ、`com.tomixrm.kagi.desktop` を `~/.local/share/applications/` へ配置し
  `update-desktop-database` / `gtk-update-icon-cache` を best-effort 実行。
  curl|bash 型ではなく**zip 同梱・検査可能**な形を採用

#### 追補(2026-07-23): ウィンドウ app_id ↔ `.desktop` の紐付け

メインウィンドウは `WindowOptions.app_id = Some(kagi::APP_ID)`(= `com.tomixrm.kagi`)を
設定する。Linux では gpui がこれを **Wayland `app_id` / X11 `WM_CLASS`** に流す。未設定
(`None`)だと GNOME/Mutter(Ubuntu 既定の Wayland)がウィンドウを `com.tomixrm.kagi.desktop`
ランチャーに紐付けられず、**汎用フォールバックの歯車アイコン・名前 "unknown" の別
taskbar エントリ**として現れる(minimize→そのエントリから復帰、quit で本体が落ちる、と
いうユーザー報告の症状)。macOS/Windows は bundle id で識別するため no-op。

そのため配布経路ごとに散っていた `.desktop` を **id・`StartupWMClass` ともに
`com.tomixrm.kagi` に統一**する:
- `.deb`: `assets/linux/com.tomixrm.kagi.desktop`(旧 `kagi.desktop` をリネーム)
- tar.gz / AppImage 埋込: `xtask` が `com.tomixrm.kagi.desktop` を生成、`StartupWMClass=com.tomixrm.kagi`
- AppImage install: `install_linux_desktop.sh` が `${APP_ID}.desktop` / `StartupWMClass=${APP_ID}`

`kagi::APP_ID` を単一の真実源とし、`tests/desktop_integration_test.rs` と `xtask` の
ユニットテストでドリフトを固定する。

### 実装方式(ADR-0038 からの確定差分)

- **cargo-bundle は使わない**: `.app` は構造が単純(Contents/MacOS + Info.plist + Resources/icns)なので
  **xtask で手組み**する。外部 cargo install / ネットワーク依存を持たない(worktree agent / オフラインでも再現)
- **DMG は `hdiutil`**(macOS 標準)で生成。create-dmg(brew)依存を持たない
- `xtask`(workspace member)に `icon` / `bundle-macos` / `dmg-macos` / `bundle-linux` サブコマンド
- CI: `.github/workflows/release.yml`、タグ `v*` で macOS(arm64/x86_64)+ ubuntu の matrix →
  draft release に asset + SHA256SUMS

### 追補 (2026-09-06, #483): release と blocking CI の結合

- release は `actions/workflows/ci.yml` から workflow ID を取得し、対象 `GITHUB_SHA`
  の run のみを選ぶ。同名の commit check、別 workflow、別 SHA は根拠にしない。
- 選択方針は **最大 run_number の run、その run の最新 run_attempt**。
  古い run の再実行は新しい run に優先しない。attempt 専用 jobs API を使い、
  run ID / attempt / SHA が一致する `blocking-ci` 集約 job の success のみを受理する。
  成功を返す直前にも最新 run/attempt を照会し、照会中の再実行を取りこぼさない。
- 必須集合は ADR-0077 の `blocking-ci.needs` に一本化し、release 側に invariant 名を列挙しない。
  advisory job の終了や workflow 全体の success は要求しない。
- 未到着の run / 実行中の集約のみ最大30回、60秒間隔で待つ。完了 run に集約がない、
  集約が skipped / cancelled / failure / 非 success、API エラーは即時拒否する。
  最新 attempt に集約がない部分再実行も古い成功では代用せず、CI 全体を再実行する。
- fixture 検証コマンドは ADR-0077 追補を参照。実 release / tag 操作なしで検証できる。

### 追補 (2026-10-06, #1055): リリースアーカイブとワンコマンド導入

- `install.sh` を macOS arm64 / Linux x86_64・aarch64 の第一導入経路とする。
  GitHub Release の `kagi-<version>-arm64-macos.tar.gz` /
  `kagi-<version>-x86_64-linux.tar.gz` /
  `kagi-<version>-aarch64-linux.tar.gz` と既存の各プラットフォーム別
  `SHA256SUMS-*.txt` を取得・照合してから配置する。macOS tar は意図的に
  `aarch64-macos` としない。v0.43 以前の Linux aarch64 更新クライアントが
  Mac 用 tar を Linux 用の旧アセット名として誤認しないためである。
  `--version vX.Y.Z` は latest-release API を呼ばず、`--prefix DIR` / `--dry-run` /
  `--no-modify-path` を提供する。API 失敗時は `--version` 指定を案内して終了する。
  通常の取得は HTTPS・TLS 1.2 以上に限定し、明示指定した fixture URL のみ
  HTTP を許す。API エラーは取得原因を 1 行の `--version` 案内に含める。
- macOS tar は `xtask bundle-macos` が **ad-hoc 署名・検証済み**の
  `target/dist/Kagi.app/` を変更・再署名せずにそのまま格納し、
  tar ルートの `bin/kagi` は `../Kagi.app/Contents/MacOS/kagi` への
  相対 symlink とする。既存の DMG 名・作成手順は維持する。既定は
  `/Applications/Kagi.app` (書き込めない場合 `~/Applications/Kagi.app`) と
  `~/.local/bin/kagi`、明示 `--prefix DIR` は `DIR/Kagi.app` と `DIR/bin/kagi`。
  notarize は未対応で、初回起動の Gatekeeper / quarantine 案内は残す。
  script は検証済み app に限って `xattr -dr com.apple.quarantine` を実行する。
  これはユーザーが信頼した配布物の quarantine を外す判断であり、SHA256 の一致も
  Apple の認証や配布者署名の代替にはならない。Developer ID / notarization は未定。
  app と CLI symlink の切替中だけ HUP / INT / TERM を保留し、直後に戻す。
- Linux tar は既存の `kagi-<version>-<arch>/` という外側ディレクトリ
  (`bin/kagi`、`share/applications/com.tomixrm.kagi.desktop`、
  `share/icons/hicolor/512x512/apps/kagi.png`)を維持し、ファイル名だけ
  `kagi-<version>-<arch>-linux.tar.gz` にする。旧ファイル名
  `kagi-<version>-<arch>.tar.gz` の同内容コピーはリリース workflow が
  `xtask bundle-linux --legacy-linux-name` を明示した移行リリースでのみ公開する。
  本 PR を初めて含むリリースを公開した後に workflow のフラグを外し、
  以降は旧名を生成しない（バージョン番号の比較では制御しない）。
  既定導入先は `~/.local`、明示 prefix は `DIR/bin` と
  `DIR/share`。AppImage zip・deb・Windows zip の名前と内容は変更しない。
  Linux は resource を先に、実行ファイルを最後に配置する。途中で失敗した
  場合は resource が一部変更済みの可能性をエラーに明記する。
- release CI は macOS の署名済み app を tar 化してから従来どおり DMG を作り、
  tar.gz を checksum と release asset に含める。README は script → mise
  (公開済み GitHub asset を直接使う) → 手動 → cargo の順に案内する。
  script と mise の実リリース導入は **v0.44.0 公開後**に可能となる。
- `kagi --version` / `kagi -V` は `src/main.rs` 冒頭で
  `kagi <CARGO_PKG_VERSION>` を stdout に表示して exit 0 とする。
  GPUI・settings・single-instance IPC に触れず、通常の引数と `KAGI_*` の経路は維持する。
  Windows の GUI subsystem ビルドでは `--version` の stdout がコンソールに表示されない制約があり、対応は [#1056](https://github.com/TomiXRM/kagi/issues/1056) に延期する。
  root の binary 起動 test と、実 tar から prefix に入れた CLI の実行で検証する。
- installer CI は `install.sh` / `xtask/**` / `.github/workflows/*.yml` /
  CI fixture の変更時だけ Linux release build と install を実行する。
  選択された install は blocking、非選択時の skip だけ集約が成功扱いし、
  従来の Linux tests は advisory を維持する。
- Windows の PowerShell installer は [#1056](https://github.com/TomiXRM/kagi/issues/1056)、
  mise registry への登録は [#1057](https://github.com/TomiXRM/kagi/issues/1057)
  に分離する。本件の `github:TomiXRM/kagi` は registry 登録なしで使う。

### Icon pipeline(ユーザー素材: assets/icon-512x512.png)

- **Apple スタイルの角丸**を画像加工で適用する(ユーザー依頼)。macOS 標準ツールのみ:
  Swift(CoreGraphics)スクリプトで (1) 1024² キャンバスに **約82% へ inset**(Apple icon grid 相当の余白)、
  (2) **連続角丸(squircle 近似、corner radius ≈ artwork の 22.37%)** でマスク、(3) 透過 PNG 出力
- `sips` + `iconutil` で `AppIcon.iconset` → `AppIcon.icns`(16〜1024。512 源泉の 1024 はアップスケール、
  将来 1024 master に差し替え可能なようスクリプト化)
- Linux 用に 128/256/512 PNG を同スクリプトから出力。生成物は `assets/icon/` 配下にコミット

## Consequences

- 未署名(ad-hoc)配布のため macOS では初回起動に Gatekeeper 回避手順が必要 — README に明記。
  Developer ID 取得後に ADR-0038 Phase 2(notarization)へ進む
- Cargo.toml への workspace member 追加(xtask)を許可(本 ADR が根拠。vendor 純度は不変)
