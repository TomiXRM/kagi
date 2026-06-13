# W21-APPIMAGE: linux-arm64 + AppImage + 同梱インストールスクリプト

- Status: done
- 担当: worktree agent(Opus)
- 仕様の正: ADR-0047 の「追補(2026-06-13)」セクション。必読

## スコープ

1. **xtask `bundle-appimage`**(`xtask/src/`):
   - `cargo run -p xtask -- bundle-appimage --bin <path> [--arch x86_64|aarch64]`
   - AppDir 生成: `Kagi.AppDir/{AppRun(シンボリックリンク or 起動シェル), kagi.desktop,
     kagi.png(assets/icon/icon_512x512.png), usr/bin/kagi}`
   - `appimagetool` が PATH or `$APPIMAGETOOL` にあれば `--appimage-extract-and-run` 前提で実行して
     `target/dist/Kagi-<arch>.AppImage` を生成。なければ AppDir 生成までで正常終了(メッセージ出力)
   - zip 化: `target/dist/kagi_Linux-AppImage_<arch>.zip` = AppImage + `kagi.png` +
     `scripts/install_linux_desktop.sh`(`zip` コマンド or Rust 標準で。外部 crate 追加禁止)
2. **`scripts/install_linux_desktop.sh`**(新規): CANViewer の
   install_linux_desktop.sh と同パターン(`~/.local/bin/Kagi.AppImage` 配置、hicolor icon、
   `com.tomixrm.kagi.desktop`、update-desktop-database / gtk-update-icon-cache best-effort)。
   APP_NAME=Kagi / Comment="Safety-first Git GUI client" / Categories=Development;
   curl 等のネットワークアクセス禁止(オフライン完結)
3. **`.github/workflows/release.yml`**:
   - matrix に `{os: ubuntu-24.04-arm, target: linux-arm64, kind: linux, arch: aarch64}` を追加。
     この leg のみ `continue-on-error: true`(private repo では runner が無い可能性)
   - linux leg: appimagetool を公式 release(AppImage/appimagetool, continuous)から arch 別に
     ダウンロード → `bundle-appimage` 実行 → zip も artifact upload に追加
   - SHA256SUMS の対象に zip を追加。release job は現行の単一 draft 方式を維持
4. README(en/ja)の Linux 行を更新: tar.gz に加えて AppImage zip + 同梱スクリプトの使い方
   (`unzip → bash install_linux_desktop.sh`)を1〜2行で

## 触ってよいファイル

- `xtask/src/*` / `scripts/install_linux_desktop.sh`(新規)/ `.github/workflows/release.yml` /
  `README.md`・`README.ja.md`(Linux インストール行のみ)/ 本チケット

## 検証

- macOS ローカル: `bundle-appimage` が AppDir レイアウトと zip(AppImage 抜き or ダミーバイナリ)を
  正しく組むこと、`cargo test --workspace` 全パス + own-code warning 0
- release.yml は YAML 構文チェックまで(実走は PM がタグで行う)
- install スクリプトは shellcheck 相当の目視 + `bash -n`

## 共通規約

- fixture / tempdir のみ。chars() 切り詰め。Cargo.toml 依存追加禁止
- 完了時: 実装メモ + Status: done、worktree branch に commit(push/merge しない)

## 実装メモ(2026-06-13)

- **xtask `bundle-appimage`**(`xtask/src/appimage.rs` 新規、`main.rs`/`util.rs` 配線):
  - `Kagi.AppDir/{AppRun, kagi.desktop, kagi.png, usr/bin/kagi}` を組む。AppRun は
    `readlink -f` で AppDir を解決して `usr/bin/kagi` を exec する POSIX sh スクリプト
    (シンボリックリンクではなく実体スクリプト — macOS でも生成・検査できる)。
    bin と AppRun と同梱スクリプトには 0o755 を付与(Unix のみ)。
  - `appimagetool` は `$APPIMAGETOOL` → `PATH` の順で探索。あれば
    `ARCH=<arch> appimagetool --appimage-extract-and-run <appdir> Kagi-<arch>.AppImage`。
    無ければ AppDir 生成までで正常終了しメッセージ出力(ローカル macOS)。
  - zip は temp stage(`appimage-zip-stage/`)に payload をフラットに置き、システム
    `zip -q -X` で `kagi_Linux-AppImage_<arch>.zip` を生成。中身は AppImage(あれば)+
    `kagi.png` + `scripts/install_linux_desktop.sh`。crate 依存追加なし(stdlib + 外部 cmd)。
  - `--arch` は `x86_64` / `aarch64`(`arm64` も受理)。デフォルトはホスト arch。
    AppImage 規約は `aarch64`(tar/dmg の `arm64` と異なる)なので `util::host_arch_appimage()` を追加。
- **scripts/install_linux_desktop.sh**(新規、git 実行ビット 100755):CANViewer パターンを
  Kagi 用に移植。`~/.local/bin/Kagi.AppImage` 配置、hicolor 512x512 に kagi.png、
  `com.tomixrm.kagi.desktop`(Comment=Safety-first Git GUI client / Categories=Development; /
  Icon=kagi)、update-desktop-database / gtk-update-icon-cache best-effort。curl/wget なし(オフライン)。
- **release.yml**:matrix に `{ubuntu-24.04-arm, linux-arm64, aarch64, allow-failure:true}` 追加。
  `continue-on-error: ${{ matrix.allow-failure || false }}` で arm64 leg のみ落ちても release 続行。
  linux leg に appimagetool(continuous, arch 別)を curl→chmod→`APPIMAGETOOL` env、bundle-linux 後に
  bundle-appimage。SHA256SUMS の glob と artifact upload path に `*.zip` を追加。単一 draft release 方式は維持。
- **README.md / README.ja.md**:Linux 行に AppImage zip + `unzip … && bash install_linux_desktop.sh`
  を追記、package セクションと release workflow 説明も arm64 / AppImage に更新。

### 検証(macOS)

- `bundle-appimage --bin target/debug/kagi --arch x86_64` → AppDir レイアウト正・appimagetool 不在で
  graceful 終了・zip は icon+script の2点。ダミー appimagetool(`$APPIMAGETOOL`)で `--arch aarch64` を
  走らせ AppImage 生成 + zip に3点(AppImage 先頭)同梱を確認。
- `bash -n scripts/install_linux_desktop.sh` OK。`ruby -ryaml` で release.yml パース OK。
- `cargo test --workspace` 全パス(exit 0)、`cargo build -p xtask` / `cargo clippy -p xtask` warning 0。
