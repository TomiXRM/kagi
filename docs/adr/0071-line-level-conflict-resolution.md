# ADR-0071: Line-level Conflict Resolution + 採用チェックボックス階層(file / chunk / line)

- Status: Accepted(2026-06-13、ユーザー依頼: GitKraken の file/chunk/line 3 階層チェック。実装は v0.2、
  flow レーン(per-hunk)merge 後)
- 関連: requirements-conflict-ux.md v2 §10 / ADR-0064(editor layout)/ 0069(rendering)/ 0057(buffer)

## 課題

GitKraken は エディタ左に **3 階層のチェックボックス**:
- **file 単位**: そのファイル全体を current/incoming で採用
- **conflict chunk(hunk)単位**: その衝突ブロックを current/incoming で採用
- **line 単位**: 各行を個別に採用/不採用

現状 kagi は hunk 単位まで(flow レーンで per-hunk 化中)。line 単位が未対応。
また A/B pane は CodeEditor InputState(テキスト描画)で、**行ごとの checkbox を内部に差し込めない**。

## Decision

### 1. データモデル(resolution.rs を line 単位へ拡張)

現 `ConflictHunk { current: Vec<String>, incoming: Vec<String>, base, choice: HunkChoice }` を保ちつつ、
**line 単位の選択状態**を追加する:

```rust
pub struct ConflictHunk {
    pub current: Vec<String>,
    pub incoming: Vec<String>,
    pub base: Vec<String>,
    pub choice: HunkChoice,          // 既存: hunk 一括(後方互換)
    pub line_select: Option<LineSelection>,  // None=hunk choice 駆動 / Some=line 単位
}

pub struct LineSelection {
    // current/incoming の各行を採用するか + 出力順序。
    // MVP: 採用フラグ列 + 既定順(current 群→incoming 群、または交互)。
    pub current_taken: Vec<bool>,    // current[i] を含めるか
    pub incoming_taken: Vec<bool>,   // incoming[i] を含めるか
    pub order: LineOrder,            // CurrentFirst / IncomingFirst / Interleaved(将来)
}
```

- `assemble()` は `line_select` があればそれで行を組み、無ければ `choice`(hunk 一括)で組む。
  **後方互換**: 既存の hunk 単位 accept はそのまま動く(line_select=None)。
- chunk チェック = その hunk の `choice` を設定(line_select クリア)。line チェック = `line_select`
  の該当フラグ tr�。file チェック = 全 hunk に同 choice。**tri-state**(全/一部/無し)を上位へ伝播。

### 2. レンダリング戦略(ADR-0069 の見直し)

**A/B pane を「行リスト + 左チェックボックス」に変更**する(InputState CodeEditor では行ごとの
checkbox gutter を差せないため)。各 pane = `uniform_list` の行 row:
```
[☑] 12 │ <code line(monospace, syntax 任意)>
```
- 左に **line checkbox**、その左/ヘッダに **chunk checkbox**、pane ヘッダ/toolbar に **file checkbox**
  (3 階層。tri-state で親子連動)。
- 行番号 + monospace は自前 row で描画(現 InputState の利点=行番号/scrollbar を自前実装で再現)。
- **手編集(Edit result)は引き続き InputState**(Result pane の Edit モードのみ)。A/B は選択専用 row list。
- scrollbar は uniform_list の標準。A/B 縦スクロール同期は ADR-0070(行 row list なら shared
  ScrollHandle で実装しやすい — InputState の制約を回避できる副次効果)。

### 3. UI 階層(GitKraken 風・左チェック群)

```
File:  [☑/—/☐] b.txt   ← toolbar / pane header
 └ Chunk 1: [☑/—/☐] current | [☐] incoming
     ├ line: [☑] top MAIN          (current 由来)
     ├ line: [☐] top T3            (incoming 由来)
     └ ...
 └ Chunk 2: ...
```
- チェックは current 側 row 群と incoming 側 row 群それぞれに付く(両採用は両方 check)。
- 両方 check 時の順序は chunk 単位の order トグル(CurrentFirst/IncomingFirst)で(ADR-0064 既存)。

### 4. 段階実装

| Phase | 内容 |
|-------|------|
| MVP(本 ADR の実装 wave) | line 単位 checkbox + file/chunk/line tri-state + A/B を row-list 化 + Result 反映 |
| v0.2 | 複数行ドラッグ選択 / interleave 順 / syntax highlight を row に |
| v0.3 | Result の inline 自由編集との統合(現 Edit モード)/ symbol 単位 |

## Consequences

- ADR-0069(A/B=InputState CodeEditor)を **A/B は row-list、Result-edit のみ InputState** に改訂。
  行番号/scrollbar は自前 row で再現。InputState の「行 checkbox 差せない」制約を解消。
- assemble() の後方互換で、hunk 単位しか使わない既存テスト/動作は不変。
- A/B scroll 同期(ADR-0070)が row-list 化で実装しやすくなる。
