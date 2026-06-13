# T-BCM-061: Create local tracking branch operation plan を実装する

- Status: done
- Group: Remote
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

ADR-0055: tracking branch 作成 + checkout を 1 plan に。名前衝突= blocker + 入力

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## 実装メモ(Codex / w25-bcm-int)

- `plan_checkout_tracking_branch` / `execute_checkout_tracking_branch` を追加。remote prefix を除いた local 名を default にし、local name collision は blocker。
- UI は tracking branch 作成 + checkout を 1 plan/modal/background op として実行。
- `file://` remote fixture で plan/execute/upstream 設定を検証。
