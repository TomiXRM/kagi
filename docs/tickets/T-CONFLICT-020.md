# T-CONFLICT-020: conflicted file selection を実装する

- Status: todo
- Phase: P3 Editor
- 仕様: requirements-conflict-ux.md(v2)+ ADR-0056〜0067

## スコープ

file クリックで Conflict Editor 起動(both modified 優先、ADR-0065 で分岐)

## 規約

- Plan 経由(ADR-0067)。in-memory 主義(continue まで repo を汚さない)。
- chars() のみ・バイトスライス禁止。theme()・i18n Msg(ADR-0048。ours/theirs は出さない)。
- own-code warning 0。`cargo test --workspace` green。fixture のみ。
