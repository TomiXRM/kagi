# W7-INSPECTOR2: Inspector レイアウト再設計(message スクロール枠 + files 1:1)

- Status: done
- 担当: worktree agent(Opus)
- 関連 ADR: 0015(旧構成)— 本チケットで構成を改訂する

## 背景

ユーザー報告: 長文 commit message(実 repo の a432f0f4)だと Metadata 内に message が
インライン展開され、Changed Files に到達するのに延々スクロールが必要。

## 新レイアウト(ユーザー指定)

```
[title(commit summary、2行まで wrap + truncate)]
[[avatar][author name][date][short hash chip]]   ← 1行のメタ行
[actions(小型ボタン1行: Create branch / Cherry-pick / Copy SHA)]
[message(独立スクロール枠)]                       ← ↕ リサイズ可
──────── divider(ドラッグで比率変更) ────────
[counts 行: N modified · N added · N deleted · N renamed]
[changed files(Path⇄Tree トグル、スクロール)]
```

- **message 枠と files 枠の縦比はデフォルト 1:1**、divider ドラッグで変更可(0.2〜0.8 に clamp)
- message 枠は `overflow_y_scroll`(`.id` 必須)+ `flex` + `min_h(px(0.))`
- 旧 Metadata の縦積み(Author/Committed/Parents/SHA/Message ラベル列)は廃止。
  - author+date は メタ行に集約(committed 日付を使用)
  - short hash chip: tooltip で full SHA、クリックで Copy SHA(dispatch 経由)
  - Parents / full SHA の常時表示は廃止(tooltip と Copy で代替。情報は context menu でも取れる)
- counts 行は ChangeKind 集計(modified/added/deleted/renamed、0 のものは省略)
- Compare モード(`Comparing: … ↔ …` ヘッダ + files)は files 枠側で従来どおり動作すること

## divider 実装

- 既存 `DividerKind` に `InspectorSplit` を追加(絶対座標方式 — on_drag の値は登録時固定、
  delta 方式は使わない。T023 の教訓)
- 比率 state: `KagiApp.inspector_split: f32`(default 0.5)
- 座標→比率: inspector 領域の top は タブストリップ高 + ヘッダ高(定数)、bottom は
  viewport_h − status bar(22) − bottom panel(open 時 height)。BottomPanel divider の
  viewport 計算(mod.rs の DividerKind::BottomPanel 分岐)を参照

## 完了条件

- [x] 長文 message fixture(後述)で: message は枠内スクロール、files は初期表示で見える(headless 起動・描画確認;枠内スクロールの体感は PM 実機)
- [ ] divider ドラッグで比率が滑らかに変わり、clamp が効く(PM 実操作)
- [x] デフォルト比率 1:1(`inspector_split = 0.5`)
- [x] counts 行の集計が正しい(fixture で modified+added 混在を確認)
- [x] Compare モード回帰なし / Path⇄Tree 回帰なし / ファイルクリック→main diff 回帰なし(配線・ハンドラ温存、files_box 側に CompareView 表示)
- [x] 既存 headless ログ回帰なし(inspector は gpui 描画層のみ変更、ログ出力は不変)
- [x] `cargo test` 全パス + own-code warning 0
- [x] 実装メモを本ファイル末尾に追記

## テスト用 fixture

`bash scripts/make_fixture.sh` の repo に、tempdir 内で**20行以上の長文 message commit**
(日本語含む)を追加して検証する(scripts/ は変更しない。検証手順で git commit -m "$(printf ...)" を使う)。
ユーザーの実 repo には触らない。

## 触ってよいファイル

- `src/ui/inspector.rs`(主戦場)/ `src/ui/mod.rs`(DividerKind/state/render_body 配線の最小限)
- `docs/tickets/W7-INSPECTOR2.md`

## 触ってはいけないファイル

- `src/git/` / `tests/*` / `scripts/*` / `Cargo.toml` / 他 docs

## リスク

- divider の座標計算(bottom panel open/close で inspector の高さが変わる)— 両状態で確認
- 文字列切り詰めは chars() ベース
- avatar は `src/ui/avatar.rs`(avatar_color / avatar_initial)を流用
- スクロールは `.id` がないと効かない(既知 gotcha)

## 実装メモ(W7-INSPECTOR2)

### レイアウト構造(inspector.rs `render_inspector`)
外側 panel = `flex_col`(`h_full`):
1. `header_region`(`flex_shrink_0`、スクロールしない): title(`line_clamp(2)`)→ meta row → badges row → actions row(1行 `flex_wrap`)
2. `message_box`(`#inspector-message-scroll`、`overflow_y_scroll` + `min_h(0)` + `flex_basis(relative(split))` + `flex_shrink`)
3. `split_divider`(`#inspector-split-divider`、`h(4)` `flex_shrink_0`、`cursor_row_resize`、`on_drag(DividerKind::InspectorSplit)`)
4. `files_box`(`flex_basis(relative(1-split))` + `flex_shrink` + `min_h(0)`): compare banner → counts row → Path/Tree toggle → `#inspector-files-scroll`(`overflow_y_scroll` + `flex_1` + `min_h(0)`)

ratio は flex_basis 比で確保。header は `flex_shrink_0`、message/files は `flex_shrink` 有効なので残余高さを ~1:1 で分割。

### meta row
avatar(18px 円、`avatar_color(email)` 背景 + `avatar_initial(name)`)→ author name(`truncate`、24 chars で省略)→ committed date(`d.committed_date`、`text_xs muted`)→ hash chip。
author name/email は `d.author_line`("name  <email>  date")を `parse_author()` で分解(`  <` と `>` の ASCII マーカーのみで split、byte slice しないので日本語 name 安全)。

### hash chip(旧 Metadata 縦積みの代替)
`#inspector-hash-chip`、tooltip = full SHA(committer が author と異なる場合は `\nCommitter: …` を追加;`HashTooltip` entity が `\n` split 描画)、click = `dispatch_commit_action(CopySha)`(ADR-0022 経由、`copy_sha_click1` 再利用)。Parents / full SHA の常時表示は廃止。

### counts row
`changed_files` を ChangeKind 集計(modified/added/deleted/renamed/type-change、0 は省略、`·` 区切り)。compare モードでは compare files を集計(mod.rs 側で files = compare_view.files を渡す既存配線そのまま)。

### divider 計算式(mod.rs `divider_drag_move` の `DividerKind::InspectorSplit` 分岐)
```
top    = INSPECTOR_TOP_OFFSET(= 30 tab strip + 1 border + 34 toolbar = 65)
bottom = viewport_h - (open ? STATUS_BAR_H(22) + bottom_panel_height + BOTTOM_PANEL_DIVIDER_H(4) : STATUS_BAR_H)
ratio  = clamp((cursor_y - top) / (bottom - top), 0.2, 0.8)
```
絶対座標方式(BottomPanel と同様、delta 不使用)。bottom panel open/close 両方で `bottom_taken` が切り替わる。

### 検証
- fixture: `/tmp/kagi-w7-fix`(`scripts/make_fixture.sh`)+ 22 行日本語 message commit(modified a.txt/b.txt + added longfile.txt/untracked.txt の混在)
- headless(`KAGI_OPEN_REPO` + `KAGI_SELECT_FIRST=1`)で起動・row0 選択・`changed files: 4`・panic なし、既存ログ構造変化なし
- `cargo test` 全 19 binary pass(exit 0)/ own-code warning 0

### 未解決(PM 実機ドラッグ確認)
- divider ドラッグの滑らかさ・clamp(0.2/0.8)の体感、bottom panel open/close 切り替え時の比率追従
- 長文 message での message 枠内スクロール挙動の実機確認(headless ではフレーム描画のみ確認)
