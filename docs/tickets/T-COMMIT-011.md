# T-COMMIT-011: Amend — UI(3 モード選択 / SHA 変化表示 / 2段階確認)

- Status: todo(ADR-0040 決定済み: MVP=未push のみ、pushed は blocker。案C は v0.2 設計)
- 依存: T-COMMIT-010 / ADR-0040 / 0023
- 関連: lane W14-AMEND(UI は PM 主体)

## 背景

amend backend を Commit Panel / Header に繋ぐ。3 モード選択、`旧→新 SHA` 表示、history-rewriting の 2段階確認。

## スコープ

- Commit Panel / Header に「Amend last commit」エントリ。mode 選択(message only / staged / both)。
- plan modal に predicted の **SHA 変化**(旧 short → 新 short)を表示。`destructive` なので **2段階確認**
  (ADR-0023: Confirm 赤 → 追確認で「旧 SHA が失われる/reflog 復元可」列挙 → 明示クリック)。
- pushed の場合 ADR-0040 決定どおり(案 A=強警告で続行 / 案 B=disabled + 理由)。
- 実行後 reload + toast(既存 record_op / W3-NOTIFY)。

## 完了条件

- [ ] 3 モードが選べ、plan に SHA 変化が出る
- [ ] 2段階確認を経て amend が実行される
- [ ] pushed の表示が ADR-0040 決定どおり
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/commit_panel.rs` / `src/ui/mod.rs`(plan modal / dispatch)/ `src/ui/commands.rs`(Command Registry 統合)
- `docs/tickets/T-COMMIT-011.md`

## 触ってはいけないファイル

- `src/git/*`(backend は T-COMMIT-010)/ `Cargo.toml`

## テスト方法

1. `cargo test`
2. UI は PM がスクリーンショット + headless ログ

## リスク・規約

- 2段階確認を省略しない(history-rewriting)。タイプ入力は求めない(ADR-0023)

## 実装メモ(2026-06-13、lane W14-AMEND UI plumbing)

- `src/ui/mod.rs`: `AmendPlanModal { plan, error, mode, message, confirm_armed }` を追加。
  AppState に `amend_modal: Option<AmendPlanModal>` フィールド(初期化 2 箇所 + switch_repo リセット + any_modal_open 判定 + render clone + overlay 配線)。
- AppState メソッド: `open_amend_modal(mode, cx)`(commit input から message 取得)/
  `open_amend_modal_with_message(mode, message)`(headless 用・cx 不要)/ `cancel_amend_modal` /
  `confirm_amend`(**2段階確認**: 1回目=arm、2回目=preflight→execute→oplog→reload)。
- 2段階確認(ADR-0023): `render_amend_modal` は専用カード(共用 `render_plan_modal_card` は 2段階非対応のため)。
  1st: 緑「Amend…」/ 2nd(armed): 赤「Rewrite history」+ 「失われるもの(旧 SHA / reflog 復元可)」を列挙。
  **タイプ入力は求めない**(ADR-0023)。blocker ありの時は confirm ボタン非表示。
- plan modal は予測行に `旧 <short> → 新 <new>` を表示(backend の predicted そのまま)、staged 畳み込みファイルも preview。
- 実行後 `record_op("amend", ..)` で toast + oplog + footer、`reload()`。
- **Commit Panel の「Amend」導線は `open_amend_modal(mode, cx)` を呼ぶだけ**。`commit_panel.rs` は
  並行 lane(W14-PREVIEW/TEMPLATE/SPLIT)が編集中のため本 lane では触らず、PM が merge 時に
  ボタン → `open_amend_modal` を配線する(当該メソッドは `#[allow(dead_code)]` で先行提供)。
- 触ったファイル: `src/ui/mod.rs`(本 lane)/ `src/main.rs`(KAGI_AMEND headless)/ `src/git/ops.rs`・`src/git/mod.rs`(backend、T-COMMIT-010)。
