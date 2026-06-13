# T-CONFLICT-010: Conflict Banner を実装する

- Status: done(W30、文言精緻化は本フェーズ)
- Phase: P2 Dashboard
- 仕様: requirements-conflict-ux.md(v2)+ ADR-0056〜0067

## スコープ

上部 banner: 警告 icon + operation summary + count + current/incoming + Continue/Abort/Open Panel

## 規約

- Plan 経由(ADR-0067)。in-memory 主義(continue まで repo を汚さない)。
- chars() のみ・バイトスライス禁止。theme()・i18n Msg(ADR-0048。ours/theirs は出さない)。
- own-code warning 0。`cargo test --workspace` green。fixture のみ。
