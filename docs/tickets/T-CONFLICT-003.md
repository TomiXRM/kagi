# T-CONFLICT-003: unmerged index entries から conflicted files を取得する

- Status: done(W26、type 細分は本フェーズ)
- Phase: P1 State
- 仕様: requirements-conflict-ux.md(v2)+ ADR-0056〜0067

## スコープ

Index::conflicts、stage 1/2/3 から type 判定(ADR-0065)

## 規約

- Plan 経由(ADR-0067)。in-memory 主義(continue まで repo を汚さない)。
- chars() のみ・バイトスライス禁止。theme()・i18n Msg(ADR-0048。ours/theirs は出さない)。
- own-code warning 0。`cargo test --workspace` green。fixture のみ。
