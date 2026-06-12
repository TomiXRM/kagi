# ADR-0047: クロスプラットフォーム配布(Phase 1 = 未署名、icon pipeline 込み)

- Status: Accepted(2026-06-13、ユーザー依頼「クロスプラットフォームで配布できるようにしたい。
  Apple の認証は取れていないけど、まあいいでしょう」)
- 関連: ADR-0038(macOS .app/DMG 設計 — 本 ADR で Phase 1 + CI を実施に確定。署名/notarization
  (Phase 2)は Apple Developer Program 取得まで保留)、docs/research/openlogi-learnings.md、ADR-0031

## Decision

### 対象プラットフォーム

| OS | 形態 | 備考 |
|----|------|------|
| macOS (arm64 / x86_64) | `.app` + `.dmg` | **ad-hoc 署名**(`codesign -s -`)。Gatekeeper は右クリック→開く案内を README に記載 |
| Linux (x86_64) | `tar.gz`(bin + .desktop + icon)| AppImage は後続候補 |
| Windows | 対象外(将来) | gpui 0.2.2 の Windows 成熟度待ち。別 ADR |

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
