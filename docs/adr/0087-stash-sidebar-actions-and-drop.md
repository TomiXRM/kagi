# ADR-0087: Stash サイドバー操作の刷新 + standalone drop

- Status: Accepted(2026-06-16、ユーザー報告「stash を pop したら消えると思っていたのに、branch list の stash list から永遠に消えない」「stash を直接選択して消したい」)
- Date: 2026-06-16
- Amends: ADR-0009(stash pop = apply + drop、`stash_drop` は pop 専用の private に限定)

## Context

サイドバー(Repository Navigator)の stash 行は **左クリック = stash apply** だった。
apply は ADR-0004/0009 の設計どおり **stash を残す**ので、ユーザーが「クリックしたら
消費される」と期待して操作すると stash がいつまでも消えない、という混乱が起きていた
(pop/drop のコア実装自体は正しく、ユニットテストで確認済み)。

さらに:

- 任意の stash を **pop** する手段がサイドバーに無かった(toolbar の Pop は常に
  `stash@{0}` 固定)。
- stash を **drop(適用せず削除)** する手段が UI に無かった。ADR-0009 は
  「apply せずに drop すると stash を失う」footgun を避けるため `stash_drop` を
  private にしていた。

## Decision

### サイドバー操作(ユーザー選択: 左クリック=Pop、右クリックで Apply/Drop)

- stash 行の **左クリック = Pop**(apply + remove)。「クリックしたら消える」期待に一致。
- stash 行の **右クリック = コンテキストメニュー**:
  - Restore グループ: **Pop**(apply and remove) / **Apply**(keep stash)
  - Danger グループ: **⚠ Drop**(delete stash)
- メニューは `commit_menu` / `branch_menu` と同じ overlay パターン
  (`src/ui/stash_menu.rs`: `StashMenuState` / `StashAction` / `render_stash_menu_overlay`)。
  `KagiApp.stash_menu` に状態を持ち、`dispatch_stash_action` で各 open_*_modal へ委譲。

### standalone drop(ADR-0009 を amend)

- `ops::plan_stash_drop` / `ops::execute_stash_drop` を **public** 化。drop は
  **明示的にユーザーが起動する Destructive op**(discard / reset --hard と同クラス)
  として再導入する。
- drop は **working tree を一切触らない**。唯一の blocker は index 範囲外のみ。
- UI は **danger 確認モーダル**(`StashDropModal` → `render_plan_modal_card`、
  confirm ラベル "Drop")。実行は背景スレッド(`start_stash_drop` → busy snackbar)。
- **oplog 記録**: `execute_stash_drop` は削除前に stash commit OID を控え、それを返す。
  plan の recovery と after-summary に OID を載せ、`git stash store -m "…" <oid>` で
  復元可能であることを明示する(stash reflog 上、gc までは到達可能)。
- ADR-0009 の「`stash_drop` は private、pop の第2段でのみ呼ぶ」は、pop 内部
  (`stash_drop_internal`)については維持。standalone drop は **danger 確認 +
  WT 非変更 + OID 記録** という別フローとして例外的に許可する。

## Consequences

- 「クリックしても stash が消えない」混乱が解消(左クリックが pop に)。任意 index の
  pop / apply / drop が右クリックメニューから可能。
- drop は破壊的だが、(1) danger 確認モーダル、(2) WT 非変更、(3) OID を oplog/recovery に
  記録、の3点で ADR-0004 の「復元不能な破壊を作らない」方針を満たす。
- i18n: `BusyStashDrop`(en/ja)を追加。busy_label に `stash-drop`。
- テスト: `tests/stash_pop_test.rs::test_stash_drop_removes_entry_without_touching_working_tree`
  (drop 後 entry が1件減り WT は clean のまま、残るのは古い側)。
- 関連: 進行中表示は ADR-0086 の統一 busy snackbar に乗る([[0086-busy-snackbar-sync-icon]])。
