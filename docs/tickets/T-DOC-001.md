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
