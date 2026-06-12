# T-COMMIT-009: Message Template — type/scope/summary/body/test/risk + plain⇄template 切替

- Status: todo
- 依存: 既存 Commit Panel(T025〜T027)/ T-COMMIT-007(draft 保存形式の整合)
- 関連: lane W14-TEMPLATE、ADR-0042

## 背景

メッセージ作成の負担軽減。構造化テンプレ(type / scope / summary / body / test / risk)と plain text の
相互切替を提供する。ADR は新設せず Commit Panel UI + 純関数の組み立てで実現。

## スコープ

- template モード: type(選択 or 自由)/ scope / summary / body / test / risk の入力欄。
- **純関数で 1 本の commit message に組み立て**(例: `type(scope): summary\n\n<body>\n\nTest: ...\nRisk: ...`)。
  Conventional Commits 寄りだが厳格強制はしない(空欄は省く)。
- **plain ⇄ template 切替**: template → plain は組み立て結果を Input に流す。plain → template は best-effort
  parse(1行目から `type(scope): summary` を抽出、無理なら summary に全文)。
- 現在の mode は draft(ADR-0042)の `mode` フィールドに保存。復元時はその mode で開く。

## 完了条件

- [ ] template 入力 → 期待する 1 本の message に組み立てられる(空欄は省略)
- [ ] plain ⇄ template 切替で内容が失われない(往復で実用上保持)
- [ ] mode が draft に保存・復元される
- [ ] 組み立て純関数に unit test(複数パターン)
- [ ] `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/commit_panel.rs`(UI)/ 組み立て純関数を置くなら `src/git/message_template.rs`(新規・UI 非依存)
- `tests/message_template_test.rs`(純関数のテスト、新規)
- `docs/tickets/T-COMMIT-009.md`

## 触ってはいけないファイル

- `src/git/drafts.rs`(mode フィールドを使うだけ)/ 他チケットのファイル / `Cargo.toml`

## テスト方法

1. `cargo test`(組み立て/parse 純関数)
2. UI は PM がスクリーンショット確認

## リスク・規約

- 切替で入力を失わない設計(往復ロスを最小化)。完全可逆でなくてよいが summary/body は保持
- 文字列処理は `chars()` ベース
