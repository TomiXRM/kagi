# T-MAINSLIM-001: Slim down src/main.rs — extract the KAGI_* headless harness

- Status: todo
- Group: アーキテクチャ / bin shell
- 仕様の正: ADR-0077 (retire/relocate KAGI_*), architecture.md §2.5 (thin bin shell).

## 背景(調査済み)
- `src/main.rs` = **1457 LOC**。うち大半が **KAGI_* headless テストハーネス**(146 KAGI_ 参照、
  `record_headless_op` / `run_headless_discard` / `init_tab` + ~47 の env-var 駆動フロー)。
- OpenLogi の `main.rs` は 512 LOC(薄い shell)。
- ADR-0077 は KAGI_* の退役/縮小を方針化済み。本チケットは**第一歩=ハーネスを別モジュールへ抽出**し、
  `main.rs` を「window 作成 + 実アプリ起動 + 必要最小の bootstrap」に薄くする(挙動は不変)。

## スコープ(純粋な移設、挙動不変)
- 新規 `src/headless.rs`(または `src/ui/headless.rs`)に headless 関連を移動:
  `record_headless_op`、`run_headless_discard`、`init_tab`、および main() 内の全 `KAGI_*` 分岐ロジック
  (KAGI_OPEN_REPO / MENU_DUMP / SELECT_FIRST / JUMP / CONTEXT_MENU / COMPARE_* / OPEN_FIRST_FILE /
  PULL / PUSH / CHECKOUT / COMMIT / AMEND / DISCARD / STASH_* / CHERRY_PICK / REVERT / UNDO / POP /
  CREATE_BRANCH / DELETE_BRANCH / TERMINAL / BOTTOM_PANEL / COMMIT_PANEL / PLAN_* など)。
- `main()` からは **単一の呼び出し**(例 `headless::run_if_requested(&mut app_state, ...) -> bool`)で
  headless 経路へ委譲。theme/lang の初期化や window 起動など「実アプリにも必要」な部分は main に残す。
- **stderr のログ文字列(`[kagi] ...`)を一字一句変えない**(既存の env ハーネステストが grep している)。
- env var の優先順位・挙動を変えない。`KAGI_AUTO_CONFIRM` 等の意味も不変。

## 完了条件(受け入れ条件)
- [ ] `src/main.rs` が大幅に縮小(目標 < 400 LOC、OpenLogi 水準)。headless ロジックは新モジュールへ。
- [ ] `main()` は薄い:env 初期化 → headless 委譲(1 呼び出し)→ window/app 起動。
- [ ] すべての `KAGI_*` 経路が以前と同一挙動・同一 stderr ログを出す(回帰なし)。
- [ ] 通常起動(env なし)が以前どおり動く(GUI が出る)。
- [ ] `cargo test --workspace` 全パス + `grep -rE 'git2::|Repository::open' src/ui` = 0(main.rs の git2 は対象外だが新モジュールに git ロジックを増やさない)。

## 規約
- 純粋な移設。ロジック・文字列・順序を変えない。新機能を足さない。
- fixture/tempdir のみで検証。

## やってはいけないこと
ログ文字列の変更 / env 挙動の変更 / headless 経路の削除(retire は別チケット)/ 移設ついでの挙動変更。

## Implementation memo
(担当 agent が完了時に追記)
