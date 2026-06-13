# T-CONFLICT-041: Abort operation plan を実装する

- Status: done(W26)
- Phase: P5 CAS
- 仕様: requirements-conflict-ux.md(v2)+ ADR-0056〜0067

## スコープ

cleanup_state + ORIG_HEAD 復帰、buffer 退避、確認ダイアログ

## 規約

- Plan 経由(ADR-0067)。in-memory 主義(continue まで repo を汚さない)。
- chars() のみ・バイトスライス禁止。theme()・i18n Msg(ADR-0048。ours/theirs は出さない)。
- own-code warning 0。`cargo test --workspace` green。fixture のみ。
