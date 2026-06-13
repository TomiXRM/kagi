# T-BCM-021: Push item に ahead count を表示する

- Status: done
- Group: Sync
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

`Push ↑2`。ahead=0 は no-op 表示。current 以外の branch の push は upstream に対する push plan

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## 実装メモ

- `Push ↑N` / `Push (up to date)` labels are enabled from the pure menu availability function.
- Non-current local branch push uses `plan_push_branch` / `execute_push_branch` against that branch's upstream without checking it out.
