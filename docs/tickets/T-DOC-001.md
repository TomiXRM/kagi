# T-DOC-001: Reconcile stale `src/git/` docs after ADR-0115

- Status: todo
- Group: docs / truth-first
- 仕様の正: ADR-0116 Wave 1 / ADR-0115

## スコープ

ADR-0115 で git バックエンドは `src/git/` → `crates/kagi-git/` へ移行済みだが、
**現在の指示として読まれるライブ文書**に古い `src/git/` 参照が残り、エージェントを
存在しないパスへ誘導している。これを修正する。

対象（ライブ文書のみ書き換える）:
- `CLAUDE.md` — `src/git/ops.rs` 参照(4箇所: 行23/42/98/114 付近)、レイヤ表の
  「Git backend = `src/git/`」、「god-file mid-split」記述（分割完了済み）、
  modal の「five accessors / `take_X`」記述（`take_X` は削除済み・最大4アクセサ）。
- `AGENTS.md` — 同種の `src/git/` 参照。
- `docs/refactor-plan.md` — Step 3.6 の `src/git/snapshot.rs`、Step 5.5 の
  「`src/git` → `crates/kagi-git`（未来形）」を、ADR-0115 で完了済みと反映。
  `crates/kagi-git/src/...` 表記へ更新。
- `docs/rearch/migration/README.md` / `docs/rearch/architecture.md` — 現在位置と
  crate 構成を実態（`crates/kagi-git`）に一致させる。

歴史的スナップショットは**本文を書き換えない**:
- `docs/rearch/research/*.md`、`docs/rearch/inventory.md` は当時の分析記録。
  誤読を招きうる箇所には本文編集ではなく冒頭に
  `> NOTE (2026-06-22): src/git was extracted to crates/kagi-git in ADR-0115.`
  のような日付つき banner を追加するに留める。

## 完了条件

- [ ] ライブ文書から「存在しない `src/git/` パスを現在の指示として指す」記述が消える
- [ ] `grep -rn "src/git/" CLAUDE.md AGENTS.md docs/refactor-plan.md` が
      実在しないパスを指す行を残さない（research/inventory の banner は可）
- [ ] コード・テストには触れない（doc のみ）。`cargo` 不要
- [ ] 実装メモを末尾に追記

## 規約

- 破壊ではなく調整。歴史的記録は残す（banner 方式）。事実と異なる断定だけ除去
- ADR 番号・日付の参照を正しく保つ

## 実装メモ (2026-06-22)

- `CLAUDE.md` は `AGENTS.md` へのシンボリックリンク（同一実体）。`AGENTS.md` を1回
  編集すれば両方に反映される。両ファイルが従来同一だった理由もこれ。
- **修正したライブ文書**（現在の事実として誤りだった断定を実態に合わせて訂正）:
  - `AGENTS.md`(=`CLAUDE.md`): `src/git/ops.rs` → `crates/kagi-git/src/ops/<feature>.rs`、
    レイヤ表「Git backend = `src/git/`」→ `crates/kagi-git/`、肥大 god-file の主張から
    `src/git/ops.rs` を削除（ops 分割は完了済み・per-feature モジュール化）、
    modal の「five accessors (...`take_X`)」→ 実態の `X`/`set_X`/`clear_X`(+編集系は
    `X_mut`)。`take_X` はコードから削除済み（modal_state.rs に `fn take_` は 0 件）。
    `GitError` の行参照 `src/git/mod.rs:137` → `crates/kagi-git/src/lib.rs:143`。
    feature 追加手順・命名規約・依存理解セクションの `src/git/...` も実態へ。
    依存方向の `git(git2)` はクレート別名（パスではない）のため不変。
  - `docs/refactor-plan.md`: Step 0.1/1.1/1.3/1.4/2.1/2.2/3.6/4.1/4.2/4.3/5.2 の
    touch 対象 `src/git/...` → `crates/kagi-git/src/...`。Step 2.2 の
    `history.rs → file_history.rs` リネームは完了済み（実体は
    `crates/kagi-git/src/file_history.rs`、`history.rs` は消滅）のため strikethrough +
    **Done** 表記。Step 5.5 に「git の `src/git → crates/kagi-git` 抽出は ADR-0115 で
    完了」の Done 注記を追加（残りは `kagi-ui`/`kagi-app`）。リスク表の shim パスも訂正。
  - `docs/rearch/migration/README.md`: S3 を `[~]` → `[x]`（ADR-0115 完了）に更新し、
    sub-step が pre-extraction レイアウトを記す旨の注記を追加。S3b は実態に合わせ
    `[~]`（`Operation` enum と `worker.rs` は存在、`GitBackend` trait は未実装）。
    完了基準表の「`src/git` Backend」→「`crates/kagi-git/` (ADR-0115)」。
  - `docs/rearch/architecture.md`: §2.1 の「today's `src/git/`」→「now `crates/kagi-git/`,
    extracted in ADR-0115; formerly `src/git/`」に訂正（`src/graph/` は現存のため不変、
    target 設計記述である trait/Operation/移行シーケンスは将来像として保持）。
- **banner 方式で残した歴史的スナップショット**（本文は書き換えず、タイトル直後に
  日付つき NOTE を1行追加）: `docs/rearch/inventory.md`、
  `docs/rearch/research/{03-git-backend,04-staging-commit,05-conflict,06-diff-filetree,10-testing-release}.md`。
- **判断基準**: 機械的全置換はせず各出現を読み、「現在の指示・現状説明として誤り」なら
  修正、「当時の分析・dated ログ」なら banner か past-tense 明示で温存。
- ライブ5ファイルの最終 grep `src/git/` 残存は全て (a) strikethrough+Done、
  (b) dated 過去形ログ、(c)「formerly / no longer exists」明示のいずれか。現在の指示
  として存在しないパスを指す行はゼロ。コード(.rs)・テストは未変更、`git add/commit` も未実行。
