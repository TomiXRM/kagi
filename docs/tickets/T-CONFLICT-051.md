# T-CONFLICT-051: Open terminal at repo root を実装する

- Status: todo
- Phase: P6 Escape
- 仕様: requirements-conflict-ux.md(v2)+ ADR-0056〜0067

## スコープ

内蔵 terminal を repo root で開く

## 規約

- Plan 経由(ADR-0067)。in-memory 主義(continue まで repo を汚さない)。
- chars() のみ・バイトスライス禁止。theme()・i18n Msg(ADR-0048。ours/theirs は出さない)。
- own-code warning 0。`cargo test --workspace` green。fixture のみ。
