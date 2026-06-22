# 06 — Diff Viewer / Changed-Files Tree / Syntax Highlighting (re-architecture research)

> NOTE (2026-06-22): `src/git` was extracted to `crates/kagi-git` in ADR-0115; paths below describe the pre-extraction layout.

Research sub-agent #6. RESEARCH ONLY — no source modified.

Scope: diff viewer, changed-files tree, syntax highlighting, large-file handling,
diffstat, compare view.

Related ADRs: 0016 (changed-files & diff), 0015 (commit inspector), 0026 (compare
view-model). Reference: `docs/research/gpui-component-audit.md`.

---

## 1. Kagi 現状

### 1.1 ドメイン層は既にかなり純粋(良い土台)

`src/git/diff.rs` と `src/git/diffstat.rs` は **UI 非依存の pure data model** を既に
提供している。これは再アーキでもほぼそのまま流用できる資産:

- `DiffLineKind` { Context, Added, Removed }、`DiffLine`、`Hunk`、`FileDiff`
  (`old_path`/`new_path`/`change`/`hunks`/`is_binary`)。
- `FileDiffStat` { path, change, additions, deletions, is_binary } と
  `bar_segments(additions, deletions, max)` — 純関数。ユニットテスト同梱。
- `src/ui/file_tree.rs` の `build_file_tree(&[FileStatus]) -> Vec<TreeRow>`
  — flat path list → 深さ優先・ソート・single-child compression(VSCode 式)。
  純粋・テスト充実(8 ケース、日本語パス含む)。**位置だけが UI レイヤなのが問題**
  (純ロジックなのに `src/ui/` に住み、`gpui::SharedString` に依存している)。

git-backend の関数群(`commit_changed_files`, `commit_file_diff`, `compare_commits`,
`compare_file_diff`, `compare_commit_to_workdir(_file_diff)`, `commit_diffstat`/
`staged_diffstat`/`unstaged_diffstat`)は libgit2 を内包し `Result<_, GitError>` を
返す。レイヤ分離はおおむね正しい。

### 1.2 三種の diff view が KagiApp に絡み合っている(主要な負債)

`src/ui/mod.rs`(16,775 行)に view-model・状態・git2 呼び出し・描画が同居:

- view-model: `DiffRow`(HunkHeader / Line{kind,text,old/new lineno,highlights} /
  Binary)、`FileDiffView`、`MainDiffView`{title,stats,rows,source}、
  `MainDiffSource`(Commit / Compare / Unstaged / Staged の 4 経路)、
  `CompareView`{base,target,files,title}、`CompareTarget`{Head,WorkingTree}。
- KagiApp 状態: `diff_cache: HashMap<usize, Option<Vec<FileStatus>>>`、
  `diffstat_cache: HashMap<usize, Vec<FileDiffStat>>`、`main_diff`, `compare_view`,
  `main_diff_scroll_handle`。
- **UI が git2 を直接叩いている(レイヤ違反)**: `open_main_diff_commit` /
  `open_main_diff_compare` / `open_main_diff_wip` は `git2::Repository::open(...)`
  を呼んでから git-backend 関数を実行する。INVARIANT「UI never calls git2 directly」
  に反する。さらに 3 関数で added/removed カウント・stats 文字列生成・
  `highlight_diff_rows` 呼び出し・`eprintln!` headless ログがほぼ重複コピペ。
- diff の取得経路が `open_main_diff_inspector_file`(compare 中か否かで分岐)→
  `open_main_diff_commit`/`compare` に散在し、re-load/back 復帰の場合分け
  (`MainDiffSource` ごと)が `main_diff_step` に集中して肥大化。

### 1.3 キャッシュは「行 index キー」

`diff_cache` / `diffstat_cache` は **コミットリストの row index** をキーにする
(`HashMap<usize, …>`)。snapshot を入れ替えると毎回 `HashMap::new()` でクリア
(`reload` / 初期化各所)。問題点:

- キーが「選択行の序数」であり commit identity(`CommitId`)でない。リストの
  並び替え・フィルタ・再読込で容易に無効化され、コミット単位の再利用が効かない。
- compare / WIP 経路はこのキャッシュに乗らない(WIP は毎回 git2 再実行)。
- 真理の所在が曖昧: changed-files は行 index、main_diff は単一インスタンス、
  compare_view も単一。所有者が KagiApp に一極集中。

### 1.4 syntax highlight は gpui-component(tree-sitter)を流用済み

`highlight_diff_rows(rows, path)`(mod.rs:834)が
`gpui_component::highlighter::{SyntaxHighlighter, HighlightTheme}` と
`gpui_component::Rope` を使用。拡張子 → 言語名マップ `lang_for_ext`(~25 言語)。
**「new 側全行を 1 本のテキストに連結 → 一括ハイライト → byte range を各行へ再分配」**
方式。sigil(`+`/`-`/` `)は byte 0 を除外、表示は保持。描画は `render_main_diff_rows`
で `StyledText` + 範囲バリデーション(char boundary チェックで panic 回避)。
highlight theme は `theme().dark` で default_dark/light を選択(ADR-0036 連動、
gpui-component の `Theme.highlight_theme` とは別系統 — audit が独立維持で良いと明記)。

問題点:
- **diff には不正確**: removed 行も含めた連結ではなく new 側だけを連結している前提だが、
  実際は全 `DiffRow::Line` を連結しているため、removed 行が混ざると tree-sitter が
  「new ファイル」として見るソースが壊れ、ハイライトがずれうる。
- open 時に同期実行(大ファイルでメインスレッドをブロックしうる)。

### 1.5 large-file / binary ガード

- **binary**: `commit_file_diff` 等が `Patch::from_diff` の `None` または
  `is_binary()` フラグで `FileDiff { is_binary: true, hunks: [] }` を返し、
  `DiffRow::Binary` プレースホルダを描画。diffstat 側も `BIN` 表示。これは健全。
- **large blob ガードは diff 経路に存在しない**。`KAGI_LARGE_BLOB_BYTES`
  (default 5 MiB)は `src/git/checklist.rs`(commit checklist ドメイン)専用で、
  diff viewer には適用されていない。巨大テキストファイルの diff は丸ごとロード・
  全行 view-model 化・全行ハイライトされる。
- **大 diff の fold は未実装**: ADR-0016 は「行数 > 2000 で hunk 単位 fold(先頭 N
  hunk 展開 + Show more)」を決定済みだが、現状コードに fold 状態・分岐は無い。
  `MAX_FILES = 100`(changed-files 件数上限)はあるが、1 ファイル内の行上限は無い。
- 描画は `uniform_list`(仮想スクロール)なので「描画」コストは抑えられているが、
  **view-model 構築 + ハイライトは全行 eager**。

---

## 2. 参考プロジェクトの実装方針

### 2.1 Zed(editor / language)

- Tree-sitter を full-tree + **incremental** で深く統合。10 万行でも単桁 ms。
  編集ごとに **バックグラウンドスレッド**へ buffer snapshot を渡して再パース
  (snapshot は full copy 不要)→ keystroke-to-render が sub-ms。
- レイヤ構造: `Rope`(immutable, SumTree, 固定チャンク)→ `text::Buffer`(CRDT)→
  `language::Buffer`(syntax tree + diagnostics)→ `MultiBuffer`(excerpt 集約)。
  diff/コードレビューは MultiBuffer の excerpt として表示され、行ハイライトは
  通常エディタと同じ syntax 機構を再利用。
- 教訓: (a) ハイライトは描画と分離しバックグラウンドで。(b) immutable rope を
  snapshot して並行処理。(c) diff は「別物」ではなく buffer view の一形態。
  出典: [syntax-aware editing](https://zed.dev/blog/syntax-aware-editing),
  [Rope & SumTree](https://zed.dev/blog/zed-decoded-rope-sumtree),
  [Buffer Architecture (DeepWiki)](https://deepwiki.com/zed-industries/zed).

### 2.2 gpui-component code editor / highlighter(Kagi が既に依存)

- `crates/ui/src/highlighter/` に `SyntaxHighlighter`, `HighlightTheme`,
  `registry.rs`(LanguageRegistry), `languages/`, `diagnostics.rs`。
  `tree-sitter-languages` feature で言語バンドルを供給。`Rope` を入力に取り、
  範囲指定で `(byte_range, HighlightStyle)` を返す API(Kagi の現コードが利用)。
- Input/code-editor コンポーネントは同 highlighter + Rope で行表示。
  audit(`docs/research/gpui-component-audit.md` L53-55, L115)が
  「highlighter は採用済・tree-sitter・highlight theme は独立系統で維持で良い」と確認。
- 教訓: Kagi は highlighter を **再発明しない**。ただし呼び出しは「正しい単一ソース
  (new 側 or old 側)に対して」「バックグラウンドで」行うべき。

### 2.3 VS Code / GitKraken(UX 参照)

- changed-files は flat(path)⇄ tree トグル、status バッジ、件数、diffstat ミニバー
  — Kagi は既に整合(ADR-0016)。
- 大ファイル: VS Code は閾値超で「Large file — diff not shown / open anyway」guard。
  GitKraken も binary/large は明示プレースホルダ。Kagi はこの guard を diff 経路に
  持つべき(現状 binary のみ)。
- side-by-side / inline 切替は両者にあるが Kagi は inline 一本(MVP として妥当)。

---

## 3. 採用すべき設計(TARGET LAYERING に沿う)

### 3.1 domain(pure) — `kagi::diff` / `kagi::filetree`

- `diff.rs` / `diffstat.rs` の model(`FileDiff`, `Hunk`, `DiffLine`, `FileDiffStat`,
  `bar_segments`)はそのまま domain に据える。git2 を触る関数だけを git-backend へ。
- **`file_tree.rs` を `src/ui/` から domain へ移設**。`SharedString` 依存を排し
  `String`/`&str` を返す純モジュール `kagi::filetree::build_file_tree(&[FileStatus])
  -> Vec<TreeRow>` にする。UI は描画時に `SharedString` へ変換。テストは現状のまま流用。
- diffstat 計算・file-tree 組み立て・bar segment 配分は **すべて domain 純関数**。
  → ui/git2 非依存でテスト可能(既存テストが裏付け)。

### 3.2 git-backend — 薄い I/O ラッパに統一

- 既存 `commit_file_diff` / `compare_*` / `*_diffstat` を維持。
- **`Repository::open` を UI から排除**。各 backend 関数は「repo path or 共有
  Repository ハンドル」を受け取り、`Result<FileDiff, GitError>` を返すのみ。
  UI は git2 型(`git2::Repository`, `CommitId`→OID 変換)に一切触れない。
- WIP 経路(staged/unstaged file diff)も同じ backend API に揃える。

### 3.3 app — diff/diffstat キャッシュの所有 + commit-identity キー

- キャッシュキーを **行 index → `DiffKey`** に変更:
  ```
  enum DiffKey {
      Commit(CommitId),                      // changed-files / diffstat
      CommitFile(CommitId, PathBuf),         // single FileDiff
      Compare(CommitId, CompareTarget),      // changed-files
      CompareFile(CommitId, CompareTarget, PathBuf),
      Wip { staged: bool, path: PathBuf },
  }
  ```
  → 並び替え・フィルタ・再読込で無効化されず、コミット単位で再利用可能。
- app 層に `DiffService`(または `DiffStore`)を新設し、
  `changed_files(key)` / `file_diff(key)` / `diffstat(key)` を提供。内部で
  backend を呼び、`HashMap<DiffKey, _>` でメモ化。**KagiApp からキャッシュ所有を剥がす**。
- ハイライトは app/backgroundで非同期に行い、結果(`Vec<(Range, HighlightStyle)>`)を
  view-model へ流す(Zed 方式の縮小版)。最小実装では同期維持で可、ただし large-file
  ガード後に限定。

### 3.4 ui — 再利用可能な diff view component + view-model

- view-model `DiffRow` / `MainDiffView` は維持しつつ、**生成ロジックを 1 箇所に集約**。
  現在 `open_main_diff_commit/compare/wip` に 3 重コピペされている
  「added/removed カウント → stats 文字列 → `FileDiffView::from_file_diff` →
  highlight → `MainDiffView` 構築」を `MainDiffView::build(file_diff, source, theme)`
  という 1 関数へ。3 経路は「どの backend 関数を呼ぶか」だけが違う。
- changed-files 描画(inspector の tree/flat + diffstat_unit + status バッジ +
  active ハイライト)は ADR-0016 通り compare/commit で共通部品化(現状概ね共通だが
  inspector.rs に内包。`changed_files_view(rows, active, on_click)` として抽出)。
- diff 本体は `uniform_list` 仮想スクロールを維持(良い)。

### 3.5 syntax highlighting 戦略

- gpui-component highlighter を継続採用(再発明しない)。
- **連結対象を「new 側のみ(Added + Context)」に限定**して tree-sitter に渡す
  (removed 行を除外)。removed 行は old 側ソースを別途連結して個別ハイライト、
  もしくは removed 行は無ハイライト(赤字のみ)に割り切る。現状の「全 Line 連結」は
  バグの温床。
- highlight theme は dark/light 連動の独立系統を維持(audit 追認)。
- 大ファイルでは **ハイライトをスキップ**(プレーン色フォールバック、既に空 highlights
  パスが存在)。

### 3.6 large-file / binary handling

- binary は現状維持(`is_binary` → `DiffRow::Binary`)。
- **large-file ガードを diff 経路へ導入**: `KAGI_LARGE_BLOB_BYTES`(または diff 専用
  の `KAGI_LARGE_DIFF_*`)を backend の diff 生成前に評価し、閾値超の blob は
  `FileDiff` に `too_large: true`(新フィールド)を立てて hunks を空にし、UI は
  「Large file — N MB, diff not shown / Show anyway」プレースホルダ(binary と同様)。
- ADR-0016 の **大 diff fold**(行数 > 2000 で hunk 単位 fold + Show more)を
  `MainDiffView` の一時状態として実装(永続化しない、ADR 明記)。view-model 構築時に
  「先頭 N hunk のみ rows 化、残りは折りたたみ marker」。

---

## 4. 採用しない設計

- **Zed の MultiBuffer/CRDT/SumTree フルスタック**: Kagi は read-only diff viewer で
  編集 buffer 不要。CRDT・incremental re-parse は過剰。snapshot+background ハイライト
  の「考え方」だけ借りる。
- **side-by-side(2 ペイン)diff**: MVP は inline 一本を維持。複雑度に見合わない。
- **独自 highlighter / 独自 tree-sitter バインディング**: gpui-component の
  highlighter を使う。`Theme.highlight_theme`(gpui-component 本体系統)への乗り換えも
  しない(独立維持を audit が推奨)。
- **行 index キャッシュの温存**: commit-identity キーへ移行する。
- **full-screen 専用 Compare View**: ADR-0026 通り既存部品(inspector changed-files +
  main diff pane)再利用。新画面は作らない。

---

## 5. リスク

- **mod.rs 16,775 行からの抽出規模**: diff/file-tree/compare の view-model と
  open_*/render_* を分離するには広範な機械的移動が必要。headless ログ
  (`[kagi] diff:` / `[kagi] main-diff:` / `[kagi] compare:`)は検証スクリプト依存
  なので **文言・出力タイミングを維持**しないと CI/手動検証が壊れる。
- **キャッシュキー変更の波及**: `diff_cache`/`diffstat_cache` を参照する全箇所
  (selection 変更・reload・`main_diff_step` 再読込・compare 開始)を `DiffKey` 経由へ
  書き換える。`main_diff_step`(prev/next file & commit ナビ)の場合分けが特に密。
- **ハイライト連結バグの修正がスタイルを変える**: new 側のみ連結へ直すと既存の
  見た目(たまたま動いていた箇所)が変わる可能性。fixture/snapshot との差分注意。
- **large-file ガードの閾値**: diff に commit checklist と同じ 5 MiB を使うと、
  大きめのテキスト diff が突然非表示になり退行に見えうる。diff 専用閾値を検討。
- **file_tree の domain 移設**: `SharedString` 剥がしで `src/ui/inspector.rs` の
  呼び出し側(tree_element_rows 構築)に変換コードが必要。

---

## 6. 未解決事項

1. diff キャッシュの **メモリ上限 / 退避ポリシー**(LRU?)。`DiffKey` 化で
   エントリが増えるため上限戦略が要る。
2. large-file ガードの **閾値**(checklist の 5 MiB 共用 or diff 専用 env)と
   「Show anyway」許可後の挙動(都度ロード?キャッシュ?)。
3. ハイライトの **同期/非同期** 判断: 初手は同期 + large-file skip で十分か、
   Zed 式 background snapshot を v1.0 に入れるか。
4. removed 行の **old 側ハイライト** を行うか、赤字無ハイライトで割り切るか。
5. 大 diff **fold の初期展開数 N** と Show more 単位(hunk 単位 / 行数単位)。
6. compare(WorkingTree)diff の **再読込トリガ**: 作業ツリー変更時に compare_view を
   自動無効化するか(現状は手動 close)。
7. `file_tree` を domain(`kagi::filetree`)へ移すか、より広い「presentation domain」
   crate を作るか(他にも純ロジックが ui/ に潜む可能性)。
