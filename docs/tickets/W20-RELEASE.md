# W20-RELEASE: クロスプラットフォーム配布 Phase 1(icon pipeline + .app/.dmg/tar.gz + CI)

- Status: in-progress
- 担当: worktree agent(Opus)
- 仕様の正: **ADR-0047**(+ ADR-0038 の背景)。必読

## スコープ

1. **Icon pipeline**(`scripts/make_icon.sh` + 必要なら `scripts/round_icon.swift`)
   - 入力: `assets/icon-512x512.png`(ユーザー支給、512²、alpha 有)
   - Apple スタイル角丸: 1024² キャンバス、artwork ~82% inset、続き角丸(radius ≈ artwork の 22.37%)、
     透過 PNG。CoreGraphics(swift)で実装、ImageMagick 等の外部依存禁止
   - `AppIcon.icns`(iconutil)+ Linux 用 128/256/512 PNG → `assets/icon/` にコミット
   - 再実行可能(冪等)。1024 master 差し替えに備え引数化
2. **xtask**(workspace member 新規 `xtask/`)
   - `cargo run -p xtask -- icon | bundle-macos | dmg-macos | bundle-linux`
   - bundle-macos: release build → `target/dist/Kagi.app`(Contents/MacOS/kagi + Info.plist +
     Resources/AppIcon.icns)→ **ad-hoc 署名 `codesign --force -s - --deep`** → `codesign --verify`
   - Info.plist: CFBundleIdentifier=com.tomixrm.kagi、CFBundleName=Kagi、NSHighResolutionCapable、
     LSMinimumSystemVersion、CFBundleShortVersionString は crate version から
   - dmg-macos: `hdiutil` で Applications シンボリックリンク入り dmg(`target/dist/Kagi-<ver>-<arch>.dmg`)
   - bundle-linux: 既存ビルド成果物を tar.gz 化(bin + kagi.desktop + icon)。macOS 上では
     レイアウト生成のみ検証(クロスコンパイル不要、CI の ubuntu runner が実ビルド)
3. **CI**: `.github/workflows/release.yml` — タグ `v*`、matrix(macos-latest=arm64 /
   macos-15-intel=x86_64 / ubuntu-latest)、xtask 呼び出し、SHA256SUMS、draft release に upload
4. **README に配布セクション**: 未署名のため Gatekeeper 回避(右クリック→開く / xattr -dr
   com.apple.quarantine)を明記

## 触ってよいファイル

- `scripts/make_icon.sh` / `scripts/round_icon.swift`(新規)/ `assets/icon/`(生成物)
- `xtask/`(新規 crate)/ **ルート `Cargo.toml`(workspace members への xtask 追加のみ可、ADR-0047 根拠。
  依存追加・既存セクション変更は禁止)**
- `.github/workflows/release.yml`(新規)/ `README.md`(配布セクション追記のみ)
- `docs/tickets/W20-RELEASE.md`

## 検証

- `scripts/make_icon.sh` 実行 → icns + PNG 生成、角丸が効いていること(PM が目視)
- `cargo run -p xtask -- bundle-macos && dmg-macos` がローカル完走、`open target/dist/Kagi.app` 起動可
- `codesign --verify --strict target/dist/Kagi.app` が通る(ad-hoc)
- `cargo test` 全パス + own-code warning 0(xtask 含む)
- CI yaml は構文チェックまで(実走は PM がタグ push で行う)

## 共通規約

- fixture / tempdir のみ。ユーザー repo 禁止。chars() 切り詰め。theme() 経由(該当なし想定)
- macOS に timeout なし。`cargo test` exit code 確認
- 完了時: 本チケット末尾に実装メモ + Status: done、worktree branch に commit(push/merge しない)
