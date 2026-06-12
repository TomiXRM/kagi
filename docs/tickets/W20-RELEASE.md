# W20-RELEASE: クロスプラットフォーム配布 Phase 1(icon pipeline + .app/.dmg/tar.gz + CI)

- Status: done
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

## 実装メモ(完了)

ADR-0047 どおり Phase 1(未署名 = ad-hoc 署名)を実装。外部 cargo install / brew / ネットワーク依存なし(worktree / オフライン再現可)。

### 成果物

- **Icon pipeline**: `scripts/make_icon.sh` + `scripts/round_icon.swift`(CoreGraphics)。
  入力 `assets/icon-512x512.png`(512²、alpha)→ 1024² キャンバスに 82% inset、連続角丸
  (squircle 近似、corner radius = artwork の 22.37%、bezier ハンドルを `radius*1.28195` に引いて
  G2 連続に近似)で透過 PNG マスク。`sips` + `iconutil` で `assets/icon/AppIcon.icns`、Linux 用
  128/256/512 PNG。冪等。入力は第1引数で差し替え可(1024 master へ将来移行)。生成物を
  `assets/icon/` にコミット(`AppIcon.icns` / `icon_{128,256,512}x*.png` / `icon-rounded-1024.png` master)。
- **xtask**(新規 workspace member、**stdlib のみ・依存ゼロ**):
  - `icon` … `scripts/make_icon.sh` を呼ぶ(macOS 専用ガード)
  - `bundle-macos` … `cargo build --release -p kagi` → `target/dist/Kagi.app`
    (`Contents/MacOS/kagi` + `Info.plist` + `Resources/AppIcon.icns` + `PkgInfo`)→
    `codesign --force -s - --deep` → `codesign --verify --strict`。
    Info.plist: `CFBundleIdentifier=com.tomixrm.kagi` / `CFBundleName=Kagi` /
    `CFBundleIconFile=AppIcon` / `NSHighResolutionCapable` / `LSMinimumSystemVersion=13.0` /
    version は root Cargo.toml の `[package] version` から(toml crate なしで自前パース)。
  - `dmg-macos` … `hdiutil create`(UDZO)で `Kagi.app` + `/Applications` symlink 入り
    `target/dist/Kagi-<ver>-<arch>.dmg`。
  - `bundle-linux [--bin <path>]` … `bin/kagi` + `share/applications/kagi.desktop` +
    `share/icons/hicolor/512x512/apps/kagi.png` の tar.gz。CI ubuntu は `--bin target/release/kagi`
    を渡す。macOS ローカルでは macOS バイナリを代入してレイアウト生成のみ検証(警告を出力)。
  - ユニットテスト 3 件(version パース / arch / workspace root)。
- **CI**: `.github/workflows/release.yml`。`v*` タグ + `workflow_dispatch`、matrix
  (`macos-latest`=arm64 / `macos-15-intel`=x86_64 / `ubuntu-latest`=x86_64)。xtask 呼び出し →
  `SHA256SUMS-<target>.txt` 生成 → `softprops/action-gh-release@v2` で **draft** release へ全 asset upload
  (`fail-fast: false` で 1 leg 失敗が他をブロックしない)。
- **README**: 「配布とインストール」セクション追記(dmg/tar.gz、SHA256SUMS、macOS Gatekeeper 回避
  = 右クリック→開く / `xattr -dr com.apple.quarantine`、xtask でのローカルバンドル生成手順)。
- **Cargo.toml**: `[workspace] members = ["xtask"]` のみ追加(ADR-0047 根拠)。依存・既存セクション無変更。

### 検証結果(ローカル、macOS arm64)

- `scripts/make_icon.sh` … icns + 128/256/512 PNG 生成成功。角丸(Apple squircle)・82% inset・透過を目視確認。
- `cargo run -p xtask -- bundle-macos` … 完走。`codesign --verify --strict` =
  `valid on disk` / `satisfies its Designated Requirement`。`plutil -lint` OK、Info.plist 全キー一致。
- `cargo run -p xtask -- dmg-macos` … `Kagi-0.1.0-arm64.dmg`(約 15 MB)生成。
- `.app` 起動 … fixture repo を引数に bundled `Contents/MacOS/kagi` を起動 → GUI 起動・repo ロード・
  terminal/watcher 起動まで確認(3 秒生存後 kill)。
- `cargo run -p xtask -- bundle-linux` … tar.gz レイアウト(bin/share/applications/share/icons/...)生成確認。
- `cargo test --workspace` … 全 green(xtask 含む)。own-code warning 0(`block v0.1.6` の
  future-incompat は推移的依存で対象外)。
- `release.yml` … Ruby YAML パーサで構文 OK(tab なし)。実走は PM がタグ push で実施。

### 逸脱・注記

- 角丸は CoreGraphics の cubic-bezier で連続角丸を近似(`CALayer.cornerCurve=.continuous` の正確な
  superellipse ではないが、ハンドルを `radius*1.28195` に引いて G2 連続に近い silhouette を得ている)。
  目視で Apple アプリアイコン相当の squircle。
- DMG format は `UDZO`(zlib)。openlogi-learnings の `ULMO`(LZMA、create-dmg 依存)は brew 依存に
  なるため不採用(hdiutil 標準の UDZO を使用)。
- `codesign` は `--deep` を使用(チケット明記。ネスト helper のない単一バイナリなので問題なし。
  openlogi の inside-out / hardened runtime は Phase 2 で検討)。
- `bundle-macos` は icns 欠如時に自動で icon pipeline を呼ぶ(冪等性のため)。
