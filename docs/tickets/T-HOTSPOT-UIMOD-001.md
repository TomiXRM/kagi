# T-HOTSPOT-UIMOD-001: src/ui/mod.rs をさらに分割する(S6 続き・Analyze hotspot #1)

- Status: done
- Group: アーキテクチャ / S6(view split)
- 仕様の正: docs/rearch/migration/README.md S6、ADR-0075/0093/0095、CLAUDE.md「≤800 LOC/file」

## 背景(調査済み)

Analyze hotspot(all time)で `src/ui/mod.rs` が risk **0.6863** — 2位の実コード
(i18n.rs 0.0354)の **19倍** で断トツの単独ホットスポット。S6 で 16,775 → **5,829 LOC**
まで削減済みだが、依然 111 fn / KagiApp 110+ fields / impl ブロック2つが残る。
coupling でも main.rs(47回)・render.rs(42回)・tabs.rs(39回)と最多共変更。

現在 mod.rs に残っていて、行き先が明確な塊(調査済み、行番号は 2026-07-05 時点):

| 塊 | 概算 LOC | 行き先 |
|----|---------|--------|
| `sync_modal_inputs`(209 LOC) | ~210 | `src/ui/operations/modal_state.rs` |
| main-diff/compare 系(`open_main_diff_wip` 98 / `main_diff_step` 92 / `open_main_diff_compare` 87 / `set_commit_main_diff` 91 ほか) | ~400 | `src/ui/diff_view.rs`(既存) |
| conflict 検出系(`ConflictDetected` struct / `detect_conflict_payload` 83 / `apply_conflict_detect` 87) | ~200 | `src/ui/operations/conflict.rs` |
| `dispatch_branch_action`(148)/ `dispatch_commit_action`(110) | ~260 | それぞれ `operations/branch.rs` / `operations/commit.rs` |
| `render_platform_menu_dropdown`(155) | ~160 | 新規 `src/ui/platform_menu.rs` または `render_header.rs` |

## スコープ(純粋な移設、挙動不変)

- 上記 5 塊を各行き先モジュールへ `impl KagiApp` ブロックごと移設(1 塊 = 1 コミット)。
- 目標: `src/ui/mod.rs` < **4,500 LOC**(このチケットの分)。800 LOC 到達は後続チケット。
- 移設のみ。ロジック・シグネチャ・`klog!` 文字列を一切変えない。
- `TabViewState` / `active_modal` の規約(CLAUDE.md State-update rules)を厳守。

## 触ってよいファイル

- `src/ui/mod.rs`、上記行き先モジュール、`src/ui/mod.rs` の `mod` 宣言。

## 触ってはいけないファイル

- `crates/kagi-domain/`、`crates/kagi-git/`、`src/headless.rs`、テスト、CI。

## 完了条件

- [ ] `src/ui/mod.rs` が 4,500 LOC 未満。
- [ ] 5 塊が各行き先へ移動、`pub` 面(呼び出し側)無変更。
- [ ] `cargo test --workspace` 全パス、`grep -rE 'git2::|Repository::open' src/ui` = 0。
- [ ] `cargo fmt --check` clean、clippy 新規警告なし。
- [ ] `[kagi]` ログ出力が一字一句同一(headless テストが grep する契約)。

## テスト方法

`cargo test --workspace`(移設は既存 635 テストでカバー)。GUI 目視は人間が最終確認。

## リスク

- KagiApp のフィールド参照が濃い fn は移設先で `use` 調整のみで済むか要確認。
  済まない(private helper が芋づる)場合はその helper ごと移すか、その塊をスキップして報告。

## やってはいけないこと

移設ついでのリファクタ / rename / ログ文字列変更 / 800 LOC を狙って無理な分割。
