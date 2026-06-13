# T-CONFLICT-030: Accept current を実装する

- Status: todo
- Phase: P4 Actions
- 仕様: requirements-conflict-ux.md(v2)+ ADR-0056〜0067

## スコープ

hunk に current 側採用 → buffer 反映 → Result 即更新

## 規約

- Plan 経由(ADR-0067)。in-memory 主義(continue まで repo を汚さない)。
- chars() のみ・バイトスライス禁止。theme()・i18n Msg(ADR-0048。ours/theirs は出さない)。
- own-code warning 0。`cargo test --workspace` green。fixture のみ。
