# T-CONFLICT-024: Result Preview viewer を実装する

- Status: todo
- Phase: P3 Editor
- 仕様: requirements-conflict-ux.md(v2)+ ADR-0056〜0067

## スコープ

下段 Result/Output、由来表示、未解決明示

## 規約

- Plan 経由(ADR-0067)。in-memory 主義(continue まで repo を汚さない)。
- chars() のみ・バイトスライス禁止。theme()・i18n Msg(ADR-0048。ours/theirs は出さない)。
- own-code warning 0。`cargo test --workspace` green。fixture のみ。
