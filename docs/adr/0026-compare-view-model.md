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

### PR Peek の表示と承認 (#1102)

- PR table／sidebar の Peek は、base / head OID から read-only Compare を解決してから
  既存 Graph／Inspector へ表示する。read failure では destination を切り替えない。
- graph の読み込み範囲に PR head がなくても、既存 Inspector の Compare header・
  Path／Tree・changed files 領域を描画する。PR head の row がある場合だけ選択し、
  ない場合は以前の選択を解除して files-only にする。別 commit の header／message／
  actions を PR banner の下に残さず、detail や commit identity を捏造しない。
- 成功時は Inspector を表示し、旧 MainDiff と `commit_panel_open` の表示 gate を閉じる。
  Commit Panel entity と未送信 draft は保持する。ファイル選択は既存 Compare source の
  main diff を開き、固定した base / head OID と path を使う。
- Peek は PR mode の表示 gate だけを閉じ、open tabs と comment drafts を保持する。
  PRs button／PR open は同じ状態を復元する。Graph button による明示的な mode exit は
  従来どおり PR mode を破棄する。非表示の PR mode は Graph の keyboard routing を奪わない。
- dirty Editor は既存の単一 dirty guard と `EditorPendingIntent::PrPeek` を使う。
  `Attachment` と editor／active input entity・open path を凍結し、承認時に全て照合してから
  discard と navigation を行う。別 tab／再訪・editor／input の置換後の古い承認は何も捨てない。
  Cancel は buffer と表示文脈を維持する。解決済み Compare は一度だけ取り出して表示し、
  再描画ごとに payload を複製せず、承認後に refs を読み直して比較対象を変えない。
- Peek は fetch／checkout／repo write／oplog を開始しない。HEAD・index の staged 内容・
  working files・refs・stash は不変である。

## Consequences

- MainDiffSource の enum 拡張に伴い、再読込・復帰経路(close 時の戻り先)の場合分けが増える
- working tree 比較は unstaged + staged + untracked を含む(diff_tree_to_workdir_with_index)。
  local changes が無い場合は menu 側で disabled(ADR-0021)
