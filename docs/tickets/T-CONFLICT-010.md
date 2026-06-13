# T-CONFLICT-010: 用語ラベルの実装

- Status: backend-done(W26-CONFLICT-CORE。純関数 + direction test 実装。i18n Msg 接続は UI レーン)
- Phase: MVP
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

ADR-0058 の役割+実名ラベル生成(純関数 + direction test)。i18n Msg 追加

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証

## 実装メモ(backend-done)

- `src/git/conflicts.rs::side_labels(op, current_branch) -> SideLabels`(純関数)。
  `SideLabel { role, name }` の役割+実名ペアを current/incoming/base/result の4役割で返す。
- ADR-0058 §2 表に準拠: merge=Current branch / Merging in、cherry-pick=Commit being applied、
  revert=Changes being undone。**rebase は方向反転を翻訳**(libgit2 ours=onto → "New base"、
  theirs=replay 対象 → "Your commit being replayed")。"ours"/"theirs" は role に一切出さない。
- role 文字列は UI レーンで Msg(ADR-0048)に接続する想定の素の英語ラベル。実名(branch/commit)は翻訳しない。
- 検証: lib unit(merge/rebase/cherry-pick/revert の direction・label table、`assert_no_ours_theirs` で
  禁止語混入をガード)。
