# W11-AVATAR: GitHub アバター取得(ユーザー要望)

- Status: queued(W9-THEME merge 後に着手。W10-TOOLBAR と並列可)
- 担当: worktree agent(Opus、調査込み)
- 関連 ADR: 0037

## スコープ(方式は ADR-0037 が正)

1. **調査**: gpui 0.2.2 の http client / `img()` / `ImageSource` の実態を registry ソースで確認し、
   HTTP 手段を決定(gpui 内蔵 → 依存ゼロ / 不可 → `ureq` 追加。**この lane のみ Cargo.toml 変更可**)
2. `src/ui/avatar.rs` 拡張(+ 必要なら `src/ui/avatar_fetch.rs` 新規):
   - GitHub repo 判定(remote URL parse)
   - noreply email → username パース(新旧 2 形式、unit test 必須)
   - Commits API バッチ(per_page=100、未認証、~3 ページ上限)→ email→avatar_url マップ
   - ディスクキャッシュ `~/.kagi/avatars/`(KAGI_LOG_DIR 対応)+ メモリキャッシュ
   - background 取得 → 完了時 cx.notify で差し替え(W3 の background_spawn パターン)
3. 表示: commit row と Inspector メタ行のアバター円を「画像があれば img(rounded_full)、
   なければ現行イニシャル円」に
4. `KAGI_OFFLINE=1` で取得無効(headless テストは offline 前提で決定的に)。
   失敗は静かにフォールバック、リトライ 1 回まで
5. headless: `[kagi] avatar: resolved=<n> pending=<n> offline=<bool>` ログ(起動時 1 行)

## 完了条件

- [ ] GitHub repo(例: kagi 自身)で実アバターが rows + Inspector に出る(PM 実機確認)
- [ ] 非 GitHub repo / オフラインでイニシャル円のまま(回帰なし)
- [ ] noreply パースの unit test(新旧形式 + 不一致ケース)
- [ ] 再起動でネット取得なしにキャッシュから表示(ログで確認可能に)
- [ ] KAGI_OFFLINE=1 で既存 headless 全ログ回帰なし
- [ ] `cargo test` 全パス + own-code warning 0
- [ ] 実装メモ(HTTP 手段の決定根拠含む)を本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/avatar.rs` / `src/ui/avatar_fetch.rs`(新規可)/ `src/ui/mod.rs`・`inspector.rs`(表示差し替えの最小限)
- `Cargo.toml`(ureq が必要な場合のみ、ADR-0037 根拠)
- `src/main.rs`(KAGI_OFFLINE)/ `docs/tickets/W11-AVATAR.md`

## 触ってはいけないファイル

- `src/git/`(remote URL は既存 snapshot/refs から取れるはず。無理なら read-only 関数追加のみ可)
- `vendor/` / `tests/*`(avatar の unit test は src 内 #[cfg(test)])/ `scripts/*`

## テスト方法

1. `cargo test`(exit code 直接確認)
2. ネット検証は **kagi 自身の repo(github.com/TomiXRM/kagi)を read-only で開く**のは可
   (clone した一時コピーを使うこと。ユーザーの作業 repo そのものは開かない)。
   API 呼び出しは数回に留める(rate limit)
3. headless は KAGI_OFFLINE=1 で fixture

## リスク

- rate limit(未認証 60/h)— キャッシュ徹底、起動毎の再取得をしない(ディスクキャッシュ優先)
- 画像デコード失敗・壊れキャッシュ → 静かにイニシャル円へ(panic 禁止)
- W9-THEME merge 後の着手(色は theme() 経由)。W10 と並列のため mod.rs の変更は
  rows/inspector のアバター描画箇所に限定し、完了報告で全列挙
- 文字列処理は chars() ベース / force 系コード追加禁止(全体規約)
