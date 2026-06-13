# T-CONFLICT-001: ConflictState model を定義する

- Status: done(W26 で基礎・拡張は本フェーズ)
- Phase: P1 State
- 仕様: requirements-conflict-ux.md(v2)+ ADR-0056〜0067

## スコープ

ConflictResolutionSession(ADR-0062)+ ConflictFile + 拡張 ConflictKind(ADR-0065)。W26 の conflicts.rs を拡張

## 規約

- Plan 経由(ADR-0067)。in-memory 主義(continue まで repo を汚さない)。
- chars() のみ・バイトスライス禁止。theme()・i18n Msg(ADR-0048。ours/theirs は出さない)。
- own-code warning 0。`cargo test --workspace` green。fixture のみ。
