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

## 実装メモ(2026-06-13)

### HTTP 手段の決定
- **gpui 内蔵 HTTP は使えない**ことを registry ソースで確認。gpui 0.2.2 は
  `gpui_http_client`(= `gpui::http_client` 再エクスポート)に依存するが、
  これは **trait 定義クレート**で、`App` の既定クライアントは `NullHttpClient`
  (全リクエスト失敗)。実体の reqwest クライアントは Zed 本体の別クレートにあり
  crates.io 版には publish されていない。`with_http_client` で差し込めるが
  実体が無いので 0 依存は不可能。
- **採用: `ureq` 3.3.0(`default-features=false, features=["rustls"]`)**(ADR-0037 根拠)。
  blocking・小依存。rustls 0.23 / ring は既に gpui 系の依存ツリーに存在するため
  TLS スタックを再利用でき、新規コンパイルは最小(rustls の重複バージョンなし=
  `Cargo.lock` の rustls は 1 つのまま)。

### 画像
- 自前デコード依存は **追加せず**、gpui の `img(ImageSource::Image(Arc<gpui::Image>))`
  に委譲(gpui 同梱の `image` crate が png/jpeg/webp/gif を内蔵デコード)。
  `Image::from_bytes(ImageFormat, bytes)` で生成。フォーマットはマジックバイトで判定し、
  未知/破損は `None`→イニシャル円フォールバック(panic なし)。

### 解決順(ADR-0037 準拠)
1. noreply パース(新旧2形式・case-insensitive・id 部 digits-only 判定)→
   `avatars.githubusercontent.com/<user>?s=64`(API 不要)
2. 未解決 email のみ Commits API バッチ(未認証 `per_page=100`、最大3ページ、
   serde 無しの自前スキャナで `commit.author.email → author.avatar_url` を抽出)
3. それ以外はイニシャル円。**email を検索クエリに使う API は不使用**(privacy)

### キャッシュ / background
- ディスク: `$KAGI_LOG_DIR/avatars/` → `~/.kagi/avatars/`、ファイル名は URL の
  FNV-1a 64bit hex(sha1 依存回避;衝突しても最悪「別アバター表示」で crash 無し)。
- メモリ: `KagiApp.avatar_images: HashMap<email, Arc<gpui::Image>>`。
- 取得は `cx.background_spawn`(W3 パターン)→ 完了時 `this.update` で merge + `cx.notify`。
  `avatar_fetch_for: Option<PathBuf>` で repo ごと1回だけ起動(reload では再取得しない)。
- `KAGI_OFFLINE=1` で全ネット停止(ディスクキャッシュからの再表示は offline でも可)。
  リトライは session 中の自然な再 resolve に委ねる(失敗は静かにフォールバック)。

### 表示差し替え(mod.rs / inspector.rs、最小限)
- `src/ui/avatar_fetch.rs`(新規): resolver / parse / cache / HTTP / 背景 resolve。
- `src/ui/mod.rs`: module 宣言、`KagiApp` に2フィールド追加+2 initializer、
  `ensure_avatars()` 追加、`render()` で起動、`render_body()` で `avatar_images` を
  clone して `render_inspector` へ受け渡し、`render_rows()` のシグネチャ+rows アバター円。
  **`render_header_slot`(W10-TOOLBAR)は未変更。**
- `src/ui/inspector.rs`: `render_inspector` に `avatar_images` 引数追加、meta 行アバター円。
- `Cargo.toml`: `ureq` 追加のみ。

### 検証結果
- `cargo test` 全 19 suites パス(avatar_fetch unit 21 + avatar 10 を含む)、own-code warning 0。
- KAGI_OFFLINE=1 + fixture(非 GitHub remote): 既存ログ全回帰なし +
  `[kagi] avatar: resolved=0 pending=1 offline=true`。
- ネット検証: kagi 自身を /tmp に shallow clone して実起動 →
  `[kagi] avatar: resolved=1 pending=0 offline=false`、`~/.kagi/avatars/` に
  64x64 PNG を確認。再起動(KAGI_OFFLINE=1・同 LOG_DIR)で
  `resolved=1 pending=0 offline=true`(ネット無しでキャッシュ表示)。
- Commits API スキャナは公開 repo(octocat/Hello-World)の実 JSON で
  json パーサと同一の (email, avatar_url) ペアを抽出することを確認。
- 実画面のアバター画像目視は PM(スクリーンショット)に委ねる。
