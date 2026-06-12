# ADR-0026: Compare View Model

- Status: Accepted / Date: 2026-06-12

## Decision

- **read-only**。repository 状態を一切変更しない(plan 不要、oplog 不要)
- model:
  ```rust
  pub struct CompareView {
      pub base: CommitId,                 // 選択 commit
      pub target: CompareTarget,          // Head | WorkingTree
      pub files: Vec<FileStatus>,         // 変更ファイル一覧
      pub title: SharedString,            // "abc1234 ↔ HEAD" 等
  }
  pub enum CompareTarget { Head, WorkingTree }
  ```
- **git 層に diff 関数を追加**:
  - `compare_commits(repo, a, b) -> Vec<FileStatus>` + `compare_file_diff(repo, a, b, path)`
    (`diff_tree_to_tree`)
  - `compare_commit_to_workdir(repo, a)`(`diff_tree_to_workdir_with_index`)
- **表示は既存部品の再利用**: 
  - changed files 一覧 → Inspector の Changed Files 領域を CompareView モードで描画
    (Path⇄Tree トグル流用)
  - ファイルクリック → **main diff pane**(T-UI-003 の MainDiffView)に表示。
    `MainDiffSource::Compare { base, target, path }` を追加し、Esc/Back で復帰
  - 新しい全画面 Compare View は作らない(部品再利用で UX 一貫性を保つ)
- `Show changed files` menu 項目 = 選択 commit の通常 changed files 表示(既存 selection と同じ)
- Compare 中であることは Inspector ヘッダに `Comparing: <title>`(× で解除)
- headless: `KAGI_COMPARE_HEAD=<row>` / `KAGI_COMPARE_WT=<row>` で
  `[kagi] compare: <base> <-> <target> files=N` ログ

## Consequences

- MainDiffSource の enum 拡張に伴い、再読込・復帰経路(close 時の戻り先)の場合分けが増える
- working tree 比較は unstaged + staged + untracked を含む(diff_tree_to_workdir_with_index)。
  local changes が無い場合は menu 側で disabled(ADR-0021)
