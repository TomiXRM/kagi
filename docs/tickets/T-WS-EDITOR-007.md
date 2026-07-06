# T-WS-EDITOR-007: file tree 右クリックメニュー(標準ファイル操作 + git-aware 操作)

- Status: done (PM accepted 2026-07-07 — tests+headless verified; menu GUI checklist pending user)
- Group: workspace framework / エディタモード
- 発端: ユーザー要望「Editor Workspace の file tree に、普通のエディタが持つ標準の
  ファイル操作 + kagi 独自の git-aware 項目を、右クリックの context menu から
  一式使えるようにしてほしい」。依存: T-WS-EDITOR-006(エディタタブ)。

## スコープ(実装済み)

右クリック対象は3種: ファイル行 / ディレクトリ行 / ツリー下の空白(=リポジトリ
ルート)。`EditorWorkspaceView` に `tree_menu: Option<(TreeMenuTarget, Point<Pixels>)>`
を追加し、`ConflictView::file_menu` と全く同じ「entity は状態だけ持つ・overlay は
`render.rs` で `KagiApp` 直下にトップレベル描画・on_select は entity をリースせず
`KagiApp` メソッドを直接呼ぶ」パターンで実装(`editor_tree_menu.rs`)。

### メニュー内容

| 項目 | file | dir | root(空白) | 備考 |
|---|---|---|---|---|
| New File… / New Folder… | - | ✓ | ✓ | 空ファイルを作成しエディタで即オープン(New File) |
| Rename… | ✓ | ✓ | - | テキスト入力モーダル、`std::fs::rename` |
| Delete…(Trash) | ✓ | ✓ | - | macOS のみ表示。`~/.Trash` へ move(同一ボリューム限定) |
| Copy Path / Copy Relative Path | ✓ | ✓ | - | クリップボードへ |
| Reveal in Finder | ✓ | ✓ | - | `open -R <path>`(選択状態で Finder 表示) |
| History | ✓ | - | - | 既存 File History へ遷移(`open_file_history`) |
| Stage / Unstage | ✓ | - | - | 該当する方だけ表示(unstaged→Stage, staged→Unstage) |
| Discard Changes… | ✓ | - | - | tracked かつ変更ありのみ。既存 `DiscardModal` を path 起点で再利用 |
| Add to .gitignore | ✓ | - | - | untracked のみ。`.gitignore` に1行 append(冪等) |

## 設計判断

- **Trash 実装**: AppleScript(`osascript`)は遅いので不採用。`std::fs::rename` で
  `~/.Trash/<name>` へ move、衝突時は `name 2`/`name 2.ext` のように連番付与
  (`editor_fs_ops::trash_collision_name`、拡張子2つ以上のファイルや dotfile も
  ケア)。同一ボリューム限定 — cross-volume で `rename` が失敗したらエラー
  モーダルを出して**何もしない**(絶対に permanent delete にフォールバックしない
  — CLAUDE.md invariant #3)。非 macOS はメニュー項目自体を非表示
  (`editor_fs_ops::TRASH_SUPPORTED = cfg!(target_os = "macos")`)。
- **削除の確認は単発クリック**: 既存 `DiscardModal` は「2段階アーム」
  (1クリック目で armed、2クリック目で実行)だが、Trash move は `~/.Trash` から
  復元可能なので同じ重さのゲートは不要と判断(ponytail: リスクの実態に
  安全機構を合わせる)。`EditorDeleteConfirmModal` は単一の確認ボタンのみ。
- **Discard の path 起点対応**: 既存 `open_discard_modal_for_index` は commit
  panel の unstaged index 前提。tree からは index を共有できないので
  `open_discard_modal_for_path`(`operations/discard.rs`)を新設 — plan 生成
  ロジックは複製せず、`plan_discard(&[path])` を直接呼ぶだけ。
- **Stage/Unstage の path 起点対応**: 同様に `do_stage_file_by_path` /
  `do_unstage_file_by_path`(`operations/commit.rs`)を新設。`git add`/`reset` は
  index のみでファイルシステムに触れないため、既存の commit panel 用メソッド
  同様に commit panel と editor workspace tree の両方を明示的に再読込
  (fs watcher は絶対にこの変更を検知できない)。
- **fs 変更後の tree 更新**: Rename/New File/New Folder/Delete/.gitignore
  追加は全部 `std::fs` を実際に触るので、既存の watcher →
  `on_worktree_changed` が自動でツリーを更新する(手動 `start_load` 呼び出し
  なし)。entity 側で watcher が推測できない後始末だけ明示的に処理:
  - Rename: `remap_renamed_path(old, new)` — アクティブバッファ/タブ/
    `tab_cache` のキーを付け替え。ファイルもディレクトリ(prefix match)も
    同じロジックでカバー。
  - Delete: `close_paths_under(path)` — 削除対象(またはその配下)を開いている
    タブを閉じる。
  - New File: `open_tab(path)` — 空ファイルを即座にエディタで開く。
- **一つの `EditorFsPromptModal`**: Rename/New File/New Folder は名前入力+
  検証+`std::fs`呼び出しという同じ形なので `EditorFsPromptKind` で分岐する
  単一モーダル(`OperationPlan` は持たない — Git write ではない、ADR-0120 §4
  のスコープ通り)。`ActiveModal` の通常ルール(variant + accessors +
  open_/cancel_/confirm_ + `confirm_active_modal`/`cancel_active_modal` 登録 +
  `sync_modal_inputs` での `InputState` 生成)に完全準拠。
- **`.git` 保護**: `editor_fs_ops::path_touches_git_dir` (pure, 単体テスト済み)
  で Rename/New/Delete の対象パスが `.git` 配下に触れないかを常にチェック。
  名前検証 `validate_fs_name` はパス区切り文字・`.`/`..`/`.git` を拒否
  (`.chars()` ベースで日本語ファイル名にも安全)。
- **ディレクトリの絶対パス再構成**: `TreeRow::Dir` は圧縮された「親からの
  相対名」しか持たない(`file_tree.rs` の single-child compression)。
  `dir_path_for_tree_index`(pure, 単体テスト済み)が深さ優先の行配置を
  逆算してフルパスを組み立てる。

## klog 契約行(新規)

`editor-ws: fs-created <path>` / `fs-renamed <old> -> <new>` / `fs-trashed <path>` /
`gitignore-added <path>` / `stage <path>` / `unstage <path>`(いずれも
`editor-ws:` プレフィックス付き)。既存の `editor-ws:` 系行は無改修。

## headless 検証

`KAGI_EDITOR_WS_NEWFILE=<name>`: Editor Workspace を開き(未オープンなら開く)、
`open_editor_fs_prompt` + `confirm_editor_fs_prompt` という実際の confirm 経路
経由で `<name>` をリポジトリルートに作成する。fixture repo で実行し、
`editor-ws: fs-created <name>` の klog 行 + ディスク上のファイル存在を確認済み
(下記テスト方法参照)。Stage/Unstage/Discard-by-path は headless hook が
やや大掛かりになるため見送り、`operations/commit.rs`・`operations/discard.rs`
の実装は既存の by-index 版との対比レビュー + `cargo test --workspace` で
担保(該当パスの純粋ロジックは merge_working_tree_files の
untracked/staged/unstaged フラグ単体テストでカバー)。

## 完了条件

- [x] メニュー構成(上表)を `editor_tree_menu.rs` に実装、i18n(EN/JA)完備
- [x] Rename/New File/New Folder: 単一 `EditorFsPromptModal`、`.git` 保護 +
      名前検証(pure, 単体テスト)
- [x] Delete: `~/.Trash` move、衝突連番、cross-volume failure でエラー表示のみ
      (permanent delete へのフォールバックなし)、非 macOS では項目非表示
- [x] Stage/Unstage/Discard: 既存の plan→confirm→execute / index-only staging
      パイプラインを path 起点に拡張(新規ロジック複製なし)
- [x] Add to .gitignore: 冪等な追記(単体テスト)
- [x] Rename が開いているタブ/アクティブバッファを新パスへ付け替え、Delete が
      該当タブを close
- [x] `cargo build` / `cargo test --workspace` 全パス(新規単体テスト含む)、
      `cargo fmt --check` clean、clippy 警告 baseline(39 → 27、新規0)、
      `git2::`/`Repository::open` in `src/ui` gate = 0
- [x] headless: `KAGI_EDITOR_WS_NEWFILE=<name>` で `editor-ws: fs-created` 確認、
      既存 `editor-ws:` 契約行の変更なし、panic なし
- [ ] GUI 目視は PM: 下記チェックリスト

## PM GUI チェックリスト

1. ファイル行 / ディレクトリ行 / ツリー下空白のそれぞれで右クリック →
   メニュー内容が上表どおりに出るか(disabled/hidden の出し分け含む)。
2. New File… / New Folder…(ルート・ディレクトリ両方) → 名前入力モーダル →
   作成 → New File はエディタに即開く、ツリーにも(watcher 経由で)反映。
3. Rename…: 開いているファイルを rename → タブのパス/ヘッダが新パスに追従
   (dirty 状態のまま保持されるか)。ディレクトリ rename → 配下ファイルの
   タブも追従。
4. Delete…: ファイル/ディレクトリを削除 → `~/.Trash` に実際に着地しているか
   (Finder で確認)。同名衝突時に `name 2` になるか。削除したファイルが
   開いていたタブが閉じるか。
5. Copy Path / Copy Relative Path / Reveal in Finder の実際の動作。
6. Stage / Unstage: commit panel 側の staged/unstaged リストと整合するか。
7. Discard Changes…: 既存の discard モーダルと同じ見た目・実行結果か。
8. Add to .gitignore: `.gitignore` に追記後、対象ファイルがツリーから消える
   (watcher 反映後)か。

## テスト方法

`cargo test --workspace`(`editor_fs_ops::tests::*` / `editor_workspace::tests::*`
の新規テスト群を含む)。headless:

```
cd /tmp/some-fixture-repo && git init && git commit --allow-empty -m init
KAGI_NO_RESTORE=1 KAGI_OPEN_REPO=/tmp/some-fixture-repo \
  KAGI_EDITOR_WS_NEWFILE=hello.txt ./target/debug/kagi
```

stderr に `editor-ws: open` → `editor-ws: fs-created hello.txt` →
`editor-ws: file hello.txt` が(この順で)出て、`hello.txt` がディスク上に
作成されていることを確認。
