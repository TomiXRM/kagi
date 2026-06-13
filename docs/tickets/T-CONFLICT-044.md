# T-CONFLICT-044: unresolved/marker blocker を実装する

- Status: done(W30、項目拡張は本フェーズ)
- Phase: P5 CAS
- 仕様: requirements-conflict-ux.md(v2)+ ADR-0056〜0067

## スコープ

Continue を disabled にする blocker 判定 + 理由表示

## 規約

- Plan 経由(ADR-0067)。in-memory 主義(continue まで repo を汚さない)。
- chars() のみ・バイトスライス禁止。theme()・i18n Msg(ADR-0048。ours/theirs は出さない)。
- own-code warning 0。`cargo test --workspace` green。fixture のみ。
