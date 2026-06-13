# T-BCM-024: upstream 未設定 branch の menu 出し分けを実装する

- Status: done
- Group: Sync
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

「No upstream set」を menu 内に表示。Pull disabled / Push→Push and set upstream(ADR-0050)

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## 実装メモ

- Local branches without upstream show a disabled `No upstream set` row.
- Pull is disabled, regular Push is hidden, and `Push and create upstream` is enabled.
