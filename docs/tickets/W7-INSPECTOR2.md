# W7-INSPECTOR2: Inspector レイアウト再設計(message スクロール枠 + files 1:1)

- Status: in-progress
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

- [ ] 長文 message fixture(後述)で: message は枠内スクロール、files は初期表示で見える
- [ ] divider ドラッグで比率が滑らかに変わり、clamp が効く(PM 実操作)
- [ ] デフォルト比率 1:1
- [ ] counts 行の集計が正しい(fixture で modified/added/deleted を混在させて確認)
- [ ] Compare モード回帰なし / Path⇄Tree 回帰なし / ファイルクリック→main diff 回帰なし
- [ ] 既存 headless ログ回帰なし(inspector 構成変更でログは変えない)
- [ ] `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記

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
