# ADR-0120: Workspace pane framework(スロット解決の明示化 + ペイン内容の差し替え可能化)

- Status: Accepted
- Date: 2026-07-06
- Follows: ADR-0117(Entity テンプレート)、ADR-0118(KagiApp decomposition Phase 5.2)、
  ADR-0119(center+right takeover の前例)、ADR-0075/0095(S5 AppState 構想)、
  `docs/rearch/migration/README.md` S5/S6
- Tickets: `T-WS-EDITOR-001`〜`003`(エディタワークスペース)

## Context — 棚卸しの結果

「左ペインに branch list 以外を映すモードを追加したい」「main ペインに別のものを
出したい」(例: 左=file tree / main=エディタ / 右=hunk)という要望に対し、現状の
レイアウト決定機構を棚卸しした。

### 現状のスロット構造(2026-07-06 時点)

レイアウトは `render.rs` → `render_body.rs` にハードコードされた
**sidebar | center | right** の3ペイン + ボトムパネルで、「どのペインに何を映すか」
は `KagiApp` 上に散在する gate フィールドの **if/else の並び順という暗黙の優先順位**
で決まっていた:

| レベル | 優先順位(先勝ち) | gate フィールド |
|---|---|---|
| ウィンドウ全体 | error 画面 > Welcome > 通常シェル | `error` / `tabs.is_empty() && remote_view.is_none()` |
| body 全体 | Conflict Mode(sidebar ごと置換)> 通常 body | `conflict` + `!conflict_merge_pending` |
| center(+right 併呑) | FileHistory > Ecosystem | `file_history` / `ecosystem`(takeover、sidebar は残る) |
| center 単独 | Loading > Diff > CommitList | `loading_tab` / `main_diff` |
| right | CommitPanel > Inspector > なし | `commit_panel_open`(+entity)/ `inspector_visible`(+detail) |
| left | Navigator / 非表示 | `sidebar.visible` |
| bottom | OperationLog / Terminal / Activity | `bottom_panel_open` + `BottomTab` |

新しいペイン内容の追加は「render_body の正しい分岐位置を考古学で探す」作業であり、
takeover 系は early-return、right 系は else-if、と追加パターンもバラバラだった。
一方で、ペイン内容を自己描画する仕組み自体は ADR-0117/0118/0119 の
**`Entity<XView>` テンプレート**(fat entity / thin reflector、`WeakEntity<KagiApp>`
+ deferred 呼び出しで再入回避、`reset_per_repo_ui` で一括破棄)として確立済み。
欠けているのは **スロット割り当ての明示的な枠**だけである。

### 検討した代替案

1. **Zed 流の汎用 docking framework**(任意分割・drag&drop 配置・レイアウト永続化)
   — 却下。要望は「決まった組の切り替え」であり、汎用性の対価(フォーカス管理、
   シリアライズ、divider 動的生成)が大きすぎる。必要になった時に本 ADR の resolver
   の上に載せられる。
2. **S5(ADR-0075 AppState/RepoSession)と同時にやる** — 却下。S5 は状態の持ち方の
   再編で最大の未着手構造課題。本件は **render 側のスロット解決**に限定すれば S5 と
   直交し、S5 が来ても resolver は入力の取り出し元が変わるだけで生き残る。
3. **各ペインを即 Entity 化してから枠を作る** — 却下。Sidebar の Entity 化は
   ADR-0118 で「S5 と同時が筋」と明示的に deferred 済み。枠(スロット解決)と
   中身(Entity 化)は独立に進められる。

## Decision

### 1. スロット解決を純関数 1 箇所に集約する(`src/ui/workspace.rs`)— 実装済み

```rust
pub enum LeftPane   { Navigator, Hidden }
pub enum CenterPane { FileHistory, Ecosystem, Loading, Diff, CommitList }
pub enum RightPane  { CommitPanel, Inspector, Hidden }

pub struct WorkspaceLayout { left, center, right }
pub struct WorkspaceInputs { /* gate フィールドの bool スナップショット */ }
pub fn resolve_workspace(&WorkspaceInputs) -> WorkspaceLayout
```

- 上表の優先順位を `resolve_workspace` が一手に持ち、`render_body` は結果に対する
  **ルーティングだけ**を行う(takeover の early-return も廃止 — resolver が
  `right = Hidden` を返すので自然に流れる)。
- 純 bool 入力なので gpui なしで単体テスト可能(8 ケースを同モジュールに追加)。
  「commit_panel_open だが entity 不在なら Inspector にフォールバックせず右ペインなし」
  等の既存挙動をテストで固定した。
- 置き場所は `kagi-domain` ではなく `src/ui/`: 語彙(Inspector/CommitPanel/Navigator)
  が UI であって Git ドメインではないため。
- Conflict Mode / error / Welcome は body より上のレベル(sidebar・bottom panel ごと
  置換)なので resolver の対象外のまま `render.rs` に残す(本 ADR で文書化のみ)。

### 2. ペイン内容の追加規約

新しいペイン内容(例: 左の file tree)の追加手順を次に固定する:

1. `workspace.rs` の該当スロット enum に variant を追加し、`resolve_workspace` に
   優先順位を 1 行足す(+ テスト)。
2. 中身は **ADR-0117 テンプレートの `Entity<XView>`** として実装する
   (Backend 駆動なら fat、app 所有の非同期があるなら ADR-0119 の thin reflector)。
   `reset_per_repo_ui`(tabs.rs)への破棄追加を忘れない(repo_path を capture する
   entity はタブ切替で必ず破棄)。
3. `render_body` の match に描画 arm を 1 つ追加。
4. `KagiApp` に `open_X` / `close_X`(既存命名規約)+ 必要なら View メニュー
   (`commands.rs` の `command_state` / メニューツリー)+ i18n EN/JA。

### 3. モード切替の置き場(Stage 1)

ユーザー向けの「組の切り替え」は、gate フィールドを増やすのではなく
`KagiApp.workspace_mode: WorkspaceMode`(最初は `Graph` と `Editor` の 2 値)を
resolver の**最上流の入力**として足す。mode は per-repo の transient UI として
`reset_per_repo_ui` でリセット(entity が repo_path を capture するため)。
1 variant しかない今は敢えて導入しない(YAGNI)— T-WS-EDITOR-001 で
`Editor` variant と同時に入れる。

### 4. エディタワークスペース(例の本命)の設計

要望の「左=file tree / main=エディタ / 右=hunk」は `WorkspaceMode::Editor` として
実装する。棚卸しで判明した再利用可能部品:

| ペイン | 部品 | 状態 |
|---|---|---|
| 左: file tree | `file_tree::build_file_tree` → `TreeRow`(純データモデル) | ✅ 既存。レンダラは各所で個別実装なので共通レンダラを 1 つ書く |
| main: エディタ | gpui-component 0.5.1 `InputState::code_editor(lang)`(行番号・tree-sitter ハイライト・〜50K 行) | ✅ conflict editor の Result ペインで使用実績あり。ただし現状 lang `"text"` 固定 — `diff_view::lang_for_ext` を `set_highlighter` に配線する(conflict editor 側も同じ改善が効く) |
| 右: hunk | `render_helpers::render_diff_list`(KagiApp / FileHistoryView 両対応の汎用レンダラ、幅は親任せ) | ✅ そのまま右ペインに置ける |
| 更新検知 | `watcher.rs`(working tree 全体を監視、`WatchEvent::WorkTree`) | △ 分類・一括 refresh のみ。「開いている特定ファイルのバッファ再読込」の配線は新規 |

段階(チケット):

- **T-WS-EDITOR-001** — `WorkspaceMode` 導入 + `EditorWorkspaceView`(fat entity):
  左 = working-tree の変更ファイル tree(`working_tree_status` → `build_file_tree`、
  データ源が既存なので v1 はこれ)、main = 選択ファイルの **read-only**
  `code_editor`(`lang_for_ext` でハイライト)、右 = そのファイルの WIP diff を
  `render_diff_list` で表示。View メニュー + ショートカットで Graph ⇄ Editor 切替。
- **T-WS-EDITOR-002** — 編集可能化: dirty 管理、Cmd-S 保存(ファイル書き込みは
  Git write ではないので pipeline 対象外だが、保存→watcher→status refresh の一巡を
  設計に含める)、クリーン時の watcher 駆動バッファ再読込、未保存で閉じる際の確認。
- **T-WS-EDITOR-003** — full worktree browse(変更ファイル以外も tree に出す。
  untracked/ignore の扱いは `git status` 系の列挙を使う)+ tree→hunk ジャンプ等の磨き。

エディタからの **stage/discard 等の Git 操作は本 ADR のスコープ外**(必要になれば
既存の pipeline + modal を経由する。invariant 4 を迂回する導線は作らない)。

## Consequences

- ペイン内容の追加が「enum variant + render arm + open/close」の定型作業になり、
  優先順位は 1 つの純関数とそのテストで説明可能になった。
- `render_body` の挙動は不変(全テストグリーン、`[kagi]` コントラクト行に変更なし)。
  headless harness への影響なし。
- resolver は毎 frame 実行されるが bool 比較のみで無視できるコスト。
- S5 が入ったら `WorkspaceInputs` の組み立て元が `RepoSession` になるだけで、
  resolver・enum・render arm はそのまま使える。
- 将来 docking / レイアウト永続化が本当に必要になった場合、`WorkspaceLayout` が
  「そのフレームワークが出力すべき型」として既に定義されている。

## Not done(意図的に)

- `WorkspaceMode` の導入(T-WS-EDITOR-001 で。1 variant の enum は作らない)。
- Conflict Mode / error / Welcome の resolver への取り込み(body より上のレベル)。
- Sidebar / Inspector / CommitList の Entity 化(ADR-0118 の deferred 判断を維持)。
- ボトムパネルの resolver 統合(`BottomTab` で既に明示的)。
