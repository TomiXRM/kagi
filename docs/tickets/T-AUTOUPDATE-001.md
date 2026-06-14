# T-AUTOUPDATE-001: In-app auto-update (Zed-style, signing-aware)

- Status: todo
- Group: distribution / app lifecycle
- 仕様の正: ADR-0082 (auto-update), ADR-0047 (GitHub Releases distribution),
  ADR-0038 (signing/notarization), ADR-0080 (Settings window).

## 背景
Kagi は GitHub Releases 配布だが、アプリ内に更新導線がない。Zed と同じ GPUI なので
**Zed 方式の薄い updater を自前実装**する。方針は ADR-0082。HTTP は既存 `ureq`、
バックグラウンドは `cx.background_spawn`、設定は `~/.kagi/settings.json`(手書きJSON)、
バナーは header slot を再利用。**チェックは opt-in・サイレント失敗、インストールは必ず
確認モーダル＋チェックサム検証**(Kagi の plan→confirm→execute 思想)。

## スコープ（段階導入）

### Phase 0 — 通知のみ(署名不要・先行リリース可)
- [ ] `kagi-domain`: `Version` 解析 + 比較(手書き semver、`vX.Y.Z`)、`ReleaseInfo`、
      `pick_asset(os, arch, assets)`、`UpdatePlan`。純粋・ユニットテスト付き。
- [ ] `src/update/check.rs`: `ureq` で `releases/latest` 取得(User-Agent 必須)、
      `tag_name`/`body`/`assets` をパース。失敗は `Result`/None で握り潰す。
- [ ] 起動時バックグラウンドチェック(throttle: `update.last_checked`)＋
      メニュー「Check for updates」。
- [ ] header に「vX.Y.Z available」chip → クリックで GitHub リリースページを開く。
- [ ] Settings に `update.auto_check` トグル＋`update.skipped_version`(Skip this version)。
- [ ] domain のユニットテスト(version 比較、asset 選択、skip 判定)。

### Phase 1 — アプリ内更新(未署名)
- [ ] `src/update/download.rs`: アセットを temp に DL(進捗)。
- [ ] `src/update/verify.rs`: `SHA256SUMS-*.txt` と照合。不一致は中止(現インストール無傷)。
- [ ] `src/update/install_*.rs`: OS 別スワップ(`#[cfg]`):
      - linux: 実行中バイナリ/AppImage を rename-in-place → `exec`。
      - macos: `.dmg` を `hdiutil attach` → `Kagi.app` を temp 経由でアトミック差し替え → detach → `open`。
      - windows: `MoveFileEx` で実行中 exe をリネーム → 新 exe 配置 → 再起動。
- [ ] 更新モーダル(現→新、サイズ、検証する旨)→ 確認で実行 → 再起動 → 新バージョン確認 → oplog 記録。
- [ ] temp に書いてから commit、失敗時はロールバック(半端な状態を残さない)。

### Phase 2 — 署名(快適 UX の本丸)
- [ ] release.yml macOS: Developer ID 署名 + `notarytool` notarize + staple。
- [ ] release.yml windows: `signtool` で Authenticode 署名。
- [ ] (任意) checksums への minisign/EdDSA 署名 + 公開鍵埋め込み検証(チャネル認証)。

## 完了条件(受け入れ)
- [ ] Phase 0: 新バージョンがあるとバナー表示→リリースページが開く。なければ何も出ない。
      ネット断・API失敗でクラッシュしない。`cargo test --workspace` 全パス。
- [ ] Phase 1: 確認モーダル経由で DL→検証→入替→再起動→新バージョン起動を実機(または fixture)で確認。
      チェックサム不一致時は現インストール無傷で中止。`grep -rnE 'git2::|Repository::open' src/ui`=0。
- [ ] Phase 2: 署名済みアセットで macOS/Windows が警告なしに更新できる。
- [ ] `src/ui` は network/fs 直叩きしない(全部 `src/update` 経由)。

## 規約 / やってはいけないこと
- サイレント自動インストール禁止(必ず確認)。`reset --hard` 的な破壊操作なし。
- 検証前のバイナリを実行/差し替えしない。失敗時に既存インストールを壊さない。
- view から `ureq`/直 fs を呼ばない。テストは tempdir/fixture のみ(実 `$HOME` を汚さない)。
- 重量級依存(self_update / cargo-dist 切替)は入れない(ADR-0082 の決定)。

## 外部依存(Phase 2 ブロッカー)
- Apple Developer Program 加入(macOS 署名/notarize)。
- Windows コード署名証明書(OV/EV)。
両方とも費用・手配が要るため、Phase 0/1 を先行リリースして価値を出す。
