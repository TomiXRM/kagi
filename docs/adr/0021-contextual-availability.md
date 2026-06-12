# ADR-0021: Contextual Git Operation Availability

- Status: Accepted / Date: 2026-06-12

## Decision

- availability は **UI から分離した純関数**で決める:
  ```rust
  pub struct MenuContext {
      pub is_head: bool,            // 選択 commit == HEAD
      pub is_ancestor_of_head: bool,// HEAD から到達可能(graph_descendant_of || ==)
      pub is_merge: bool,           // parent_ids.len() >= 2
      pub dirty: bool,              // working tree dirty
      pub detached: bool,
      pub has_local_changes: bool,  // staged+unstaged+untracked > 0
      pub refs_here: Vec<RefBadge>, // 選択 commit を指す branch/tag
  }
  pub fn build_commit_menu(ctx: &MenuContext) -> Vec<MenuGroup>
  ```
- 出し分けの正準表は `docs/requirements-context-menu.md` の availability 表。
  実装と表が食い違ったら**表を正**とする
- Disabled には必ず人間が読める理由文字列を持たせる(tooltip 表示、ADR-0020)
- later 機能(tag / patch / commit link / reset soft・mixed 実行)は `Hidden`、
  Reset menu 項目自体は `Disabled(reason)` で見せる(ADR-0024)
- `build_commit_menu` は **unit test 対象**(T-CM-063): 要件3の6状況
  (HEAD 選択 / 過去 commit / 別 branch / merge / dirty / detached)を最低1ケースずつ

## Consequences

- MenuContext の構築は selection 時に既にある情報(details / status_summary / badges /
  commit_row_index)から O(1)〜O(refs) で可能。新規 git 呼び出しは
  `graph_descendant_of` 1回のみ
- 同じ MenuContext を Inspector Actions の出し分けにも使い、二箇所の判定差異を防ぐ
