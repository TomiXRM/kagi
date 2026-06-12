# ADR-0038: macOS .app バンドル化と配布パイプライン(xtask + cargo-bundle + 署名/notarization)

- Status: Accepted(2026-06-13、ADR-0047 で Phase 1 + CI 実施を確定。cargo-bundle/create-dmg は ADR-0047 で手組み xtask + hdiutil に差し替え。Phase 2 署名は Developer ID 取得待ち)
- Date: 2026-06-13
- 関連調査: docs/research/openlogi-learnings.md(OpenLogi の配布パイプライン調査)
- 関連 ADR: 0001(gpui)、0006(gpui-component)、0031(外部コード流用ポリシー)、0012(app shell responsibilities)、0036(color themes)

## Context

kagi は現在 `cargo run` でしか起動できず、**.app バンドル化・配布が未着手**。エンドユーザーに配るには macOS の `.app` / 署名 / notarization / DMG が要る。OpenLogi(gpui + gpui-component、MIT OR Apache-2.0、GPL/FSL 汚染なし)が**フル自動の署名・notarized DMG パイプライン**を持ち、その構造は kagi に高い適合性がある(調査: openlogi-learnings.md)。

主要な制約・前提:
- kagi は gpui **crates.io 0.2.2** pin(ADR-0001)。OpenLogi は zed-git gpui + `gpui_platform` 依存で **API がズレる** → コード逐語コピーではなく**手順・パターンの移植**が中心。
- kagi は**単一プロセス**の Git GUI。OpenLogi の 2 プロセス(agent + GUI)・ネスト helper・inside-out 署名・TCC 配慮は**不要**(シングル署名で足りる)。
- OpenLogi のブランド資産(`design/`)は All Rights Reserved → **アイコンは kagi 自前**で用意する。
- Apple Developer Program 登録・Developer ID 証明書は署名段階の前提(未取得なら未署名段階が当面の到達点)。

## Decision

**`xtask`(workspace 内 Rust バイナリ)にバンドル/DMG/(将来)manifest 生成ロジックを集約し、CI はそれを薄く呼ぶだけ**にする。ローカルでもタグなしでも同一コマンドで再現できることを必須とする(並列 worktree agent・オフライン sandbox でも壊れない方針、ADR-0035 の vendor 思想と整合)。

段階導入(各 Phase で価値が出る):

### Phase 1(MVP 目標): 未署名 .app / DMG
- アイコン: kagi 自前の master PNG(1024²)→ **`sips` + `iconutil`**(macOS 標準、SVG レンダラ不要)で `AppIcon.icns`。
- **`cargo-bundle`** + GUI crate の `[package.metadata.bundle]`(name / identifier / category / `icon` / `osx_minimum_system_version`、**version は書かない**= crate version へ fallback)。
- `xtask bundle-macos`: `cargo install cargo-bundle --locked` →(`xcrun --show-sdk-path` で `SDKROOT` 解決)→ `cargo bundle --release`。
- `xtask dmg-macos`: Homebrew **`create-dmg`**(背景・アイコン座標・`--app-drop-link`・`--format ULMO`)。
- **dev `.app` ラッパ**(`scripts/cargo-run-macos.sh` + `.cargo/config.toml` の `runner`、ビルド済バイナリを **hardlink**、`.dev` identifier の dev `Info.plist`)を併せて導入し、`cargo run` でも正しいアプリ名/Dock アイコン/URL scheme を得る。

### Phase 2: 署名 + notarization
- `codesign --force --options runtime --timestamp --sign <Developer ID>`(hardened runtime + secure timestamp)→ `codesign --verify --strict`。**単一プロセスなので inside-out 不要・シングル署名**。
- `xcrun notarytool submit --wait` → `xcrun stapler staple` + `validate`。

### Phase 3: CI
- `.github/workflows/release.yml`: タグ `v*` トリガ、arm64(`macos-latest`)+ x86_64(`macos-15-intel`)マトリクス。証明書 import = `apple-actions/import-codesign-certs`。秘密情報は GitHub Encrypted Secrets。
- publish = `softprops/action-gh-release@v3`(**draft → 全 asset upload → publish 反転**で "immutable release" 拒否を回避)。`SHA256SUMS` 生成。

### Phase 4(任意・将来): 自動アップデータ + Homebrew
- `minisign` 署名 + `latest.json` manifest(per-asset url/sha256/signature_url)を静的ホスト(R2/S3/GitHub Pages)へ。manifest URL・公開鍵は `option_env!` でビルド時注入。アプリ側は **opt-in consent + `Verification::Strict` の fail-closed** 検証。`gpui-updater` crate の採否は**ライセンス原文確認 + gpui 0.2.2 互換確認後**に別途判断。
- Homebrew cask は publish 後に tap repo へ `repository-dispatch`。

### ライセンス・流用方針(ADR-0031 準拠)
- OpenLogi の配布コードは **MIT OR Apache-2.0**(原文確認済)。`xtask` ロジック・dev ラッパ・config/single-instance は **Port/Reimplement** で取り込み可、出自表記を保持。
- **ブランド資産(`design/`)= All Rights Reserved → 流用しない**(kagi 自前アイコン)。
- gpui 依存差(zed-git vs 0.2.2)により、ウィンドウ/theme/updater のコードは逐語コピーせず **API 合わせの Port** とする。

## Consequences

- `cargo run -p xtask -- bundle-macos` で**未署名でも配布可能な .app/.dmg をローカル再現**できる足場ができ、MVP 配布の到達点が明確になる。
- 署名・notarization は Apple Developer Program 登録(年額)が前提 → Phase 2 以降はその取得待ち。Phase 1 までは登録不要で進められる。
- バンドル/署名ロジックを `xtask` に集約することで、CI・ローカル・worktree agent のいずれでも単一の手順を共有でき、再現性が上がる。
- 単一プロセス前提のため OpenLogi より構成が簡素(ネスト helper・TCC・IPC・tray が不要)になり、移植コストは限定的。
- 本 ADR は **Status: Proposed**。Phase ごとの実施・`gpui-updater` 等の外部依存採用は**ユーザー承認後**に確定する(ADR-0031 のハードルール準拠)。
</content>
