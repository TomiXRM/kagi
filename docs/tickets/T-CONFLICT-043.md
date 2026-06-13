# T-CONFLICT-043: Continue 前 checklist を実装する

- Status: todo(marker+unresolved は W30 済)
- Phase: P5 CAS
- 仕様: requirements-conflict-ux.md(v2)+ ADR-0056〜0067

## スコープ

ADR-0067 の全項目(unresolved/marker/index/binary/required/message/checklist)

## 規約

- Plan 経由(ADR-0067)。in-memory 主義(continue まで repo を汚さない)。
- chars() のみ・バイトスライス禁止。theme()・i18n Msg(ADR-0048。ours/theirs は出さない)。
- own-code warning 0。`cargo test --workspace` green。fixture のみ。
