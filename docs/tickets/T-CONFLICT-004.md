# T-CONFLICT-004: conflict marker 残存検出を実装する

- Status: done(W26)
- Phase: P1 State
- 仕様: requirements-conflict-ux.md(v2)+ ADR-0056〜0067

## スコープ

checklist(ADR-0043)の marker 検出を再利用

## 規約

- Plan 経由(ADR-0067)。in-memory 主義(continue まで repo を汚さない)。
- chars() のみ・バイトスライス禁止。theme()・i18n Msg(ADR-0048。ours/theirs は出さない)。
- own-code warning 0。`cargo test --workspace` green。fixture のみ。
