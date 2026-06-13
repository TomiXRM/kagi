# T-CONFLICT-021: A/B side model を作る

- Status: todo
- Phase: P3 Editor
- 仕様: requirements-conflict-ux.md(v2)+ ADR-0056〜0067

## スコープ

Current/Incoming の hunk 単位データ(zdiff3 由来、ADR-0057)

## 規約

- Plan 経由(ADR-0067)。in-memory 主義(continue まで repo を汚さない)。
- chars() のみ・バイトスライス禁止。theme()・i18n Msg(ADR-0048。ours/theirs は出さない)。
- own-code warning 0。`cargo test --workspace` green。fixture のみ。
