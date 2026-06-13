# T-CONFLICT-012: operation summary を表示する

- Status: done(W30 banner、Dashboard 版は本フェーズ)
- Phase: P2 Dashboard
- 仕様: requirements-conflict-ux.md(v2)+ ADR-0056〜0067

## スコープ

ADR-0058 の方向文言(Merging X into Y 等)

## 規約

- Plan 経由(ADR-0067)。in-memory 主義(continue まで repo を汚さない)。
- chars() のみ・バイトスライス禁止。theme()・i18n Msg(ADR-0048。ours/theirs は出さない)。
- own-code warning 0。`cargo test --workspace` green。fixture のみ。
