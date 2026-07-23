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
