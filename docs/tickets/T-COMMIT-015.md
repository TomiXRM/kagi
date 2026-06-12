# T-COMMIT-015: Smart Commit Message — backend(enum dispatch + ollama + rule-based fallback)

- Status: blocked(ADR-0044 の既定バックエンド/model が **Proposed** — ユーザー決定後に着手)/ v0.2
- 依存: ADR-0044 / 0037 / 既存 ureq(avatar_fetch.rs)
- 関連: lane W14-SMART

## 背景

staged diff から commit message を生成。ローカル LLM(ollama)第一、rule-based fallback。外部送信は既定なし
(ADR-0037 の精神)。HTTP は ureq 3 再利用。

## スコープ(ADR-0044 厳守)

- 新 module `src/git/message_gen.rs`:
  ```rust
  pub enum MessageBackend { Ollama { host: String, model: String }, RuleBased }
  pub enum Lang { Ja, En }  pub enum Style { ConventionalCommits, Plain }
  pub struct GenInput { pub diff: String, pub lang: Lang, pub style: Style }
  pub fn generate_message(backend: &MessageBackend, input: &GenInput) -> Result<String, GenError>;
  pub fn collect_staged_diff(repo: &Repository) -> String;   // staged のみ、truncate 付き
  pub fn rule_based(input: &GenInput, files: &[FileStatus]) -> String;  // 純関数、必ず非空
  ```
- **staged diff のみ**収集(unstaged 含めない)。大きければ先頭 ~8KB に truncate + ファイルサマリ添付。
- ollama: `POST http://<host>/api/generate` `{model,prompt,stream:false}`、応答 `response` を手書き JSON parse
  (serde 不要)。ureq 再利用、global timeout(数秒)。`KAGI_OFFLINE=1` で呼ばず rule-based。
- 失敗/タイムアウト/offline → `Err` または rule_based に落ちる(呼び出し側が静かに fallback)。
- **既定はローカルのみ**。external backend は enum に足さない(本チケットは Ollama / RuleBased のみ)。

## 完了条件

- [ ] `collect_staged_diff` が staged のみ返す(unstaged 混入なし、truncate 動作)
- [ ] `rule_based` が常に非空 message を返す(ファイル種別から定型)
- [ ] ollama 呼び出しが offline / 失敗で Err、rule-based に落ちる(モック or `KAGI_OFFLINE`)
- [ ] Conventional Commits / Plain、Ja / En で出し分く(rule-based の語彙)
- [ ] unit test: collect(staged only)/ rule_based 複数 / offline fallback、計 5+
- [ ] `cargo test` 全パス + own-code warning 0、新依存を足していない(ureq 再利用)
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/git/message_gen.rs`(新規)/ `src/git/mod.rs`(re-export)
- `tests/message_gen_test.rs`(新規)
- `docs/tickets/T-COMMIT-015.md`

## 触ってはいけないファイル

- `src/ui/*`(UI は T-COMMIT-016)/ `Cargo.toml`(新依存禁止、ureq は既存)/ 他チケットのファイル

## テスト方法

1. `cargo test`(`KAGI_OFFLINE=1` で決定的に rule-based 経路をテスト)
2. tempdir / fixture のみ。実 ollama に依存するテストは書かない(ネット非依存)
3. staged diff が外部に出ないこと(既定ローカルのみ)を設計で担保

## リスク・規約

- staged diff を外部へ送らない(既定 loopback or ローカル計算)。external backend は本チケット対象外
- ureq 再利用、新依存禁止。手書き JSON(serde 禁止)
- **ADR-0044 Proposed の間は着手しない**(既定 backend / model 選択の決定待ち)
