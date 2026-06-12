# W10-TOOLBAR: ツールバーを Finder/Keynote 風(アイコン主体 + 下に小ラベル)に

- Status: queued(W9-THEME merge 後に着手 — 同一領域の競合回避)
- 担当: worktree agent(Opus)
- 関連: ADR-0013(toolbar)/ T-UI-001(アイコン)/ ADR-0036(theme — merge 後は theme() を使う)

## 背景

ユーザー要望: ボタンを Apple の Finder / Keynote のツールバー風にしたい。
「アイコンが主で、アイコンの下に小さく文字。アイコンは今より大きめ」。

## スコープ

1. **ヘッダツールバーの全ボタン**(Pull / Push / Branch / Stash / Pop / Undo / Refresh / Terminal)を
   縦積みレイアウトに変更:
   ```
   [ icon (大きめ 20–22px) ]
   [ label (text_xs)       ]
   ```
   - flex_col + items_center、ボタン全体に hover bg + rounded、幅は内容フィット(最小幅で揃える)
   - アイコンサイズは gpui_component::Size::Medium 相当(現状 XSmall)。
     必要なら assets/icons/ に lucide SVG を追加してよい(KagiAssets の ASSETS 表に登録)
2. **count 表示**(Pull ↓N / Push ↑N): アイコン右上の小さな数字チップ(overlay)にする。
   0 のときは非表示。ラベルは "Pull" / "Push" のまま
3. **Undo**: ラベルは "Undo" 固定。対象 commit summary は tooltip に移す(truncate 不要になる)
4. **disabled**: 既存 toolbar_state に従い、アイコン+ラベルとも muted 色 + クリック時の理由 footer は維持
5. **ヘッダ高さの追従**: ボタンが縦積みで高くなる(~52px 想定)。
   - inspector divider の fallback 定数 `INSPECTOR_TOP_OFFSET` を新ヘッダ高に更新
     (実測 bounds 方式が主経路なので影響は fallback のみだが、値は正しく)
   - 左側の repo/branch/upstream 表示と右側 Refresh/Terminal の縦センタリングを崩さない
6. headless: 既存 `[kagi] toolbar:` ログは不変(見た目のみの変更)

## 完了条件

- [x] 全ボタンがアイコン上・ラベル下の Finder 風(PM スクリーンショット確認)
- [x] Pull/Push の count がアイコン右上チップで出る(ahead/behind fixture で確認)
- [x] disabled 状態の見た目と理由 footer が機能維持
- [x] Undo tooltip に対象 commit summary
- [x] 既存 headless ログ(toolbar/statusbar)回帰なし
- [x] `cargo test` 全パス + own-code warning 0
- [x] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/mod.rs`(render_header_slot 周辺)/ `src/ui/assets.rs` / `assets/icons/`(lucide SVG 追加)
- `docs/tickets/W10-TOOLBAR.md`

## 触ってはいけないファイル

- `src/git/` / `vendor/` / `tests/*` / `scripts/*` / `Cargo.toml` / 他 docs

## テスト方法

1. `cargo test`
2. fixture(ahead=1 の main / behind ありの branch)で PM スクリーンショット
3. 検証は fixture / tempdir のみ

## リスク

- W9-THEME merge 後の着手が前提(色は theme() 経由で書く。const 直書き禁止)
- ヘッダ高変更による他レイアウトへの影響(tab strip / body 高さ)— KAGI_WINDOW 小サイズでも確認
- 文字列切り詰めは chars() ベース / force 系コード追加禁止(全体規約)

## 実装メモ(W10-TOOLBAR 完了)

すべて `src/ui/mod.rs` の `render_header_slot` 内で完結。assets/icons・assets.rs は
既存アイコン(ArrowDown/ArrowUp/Plus/Inbox/FolderOpen/Undo2/LoaderCircle/SquareTerminal)
で足り、**新規 SVG 追加なし**。色はすべて `theme()` 経由。

- **make_btn を縦積みに再設計**: `flex_col` + `items_center`、上にアイコン、下に `text_xs`
  ラベル。アイコンは `Size::Size(px(20.0))`(20px、22px のアイコンセル内にセンタリング。
  Size::Medium=size_4()=16px より明示的に大きく、ticket の 20–22px 指定に合わせた)。
  ボタンは `min_w(52px)` で揃え、`rounded_md` + hover で `theme().selected` 背景。
  常時背景は持たせず Finder 風に hover のみ点灯。
- **count チップ**: 引数 `count: usize` を追加。`>0` のときアイコンセル(`.relative()`)の
  右上に `.absolute()` の丸チップ(`top/right = -2px`, `rounded_full`, `bg=color_branch`,
  `fg=bg_base`, 9px bold)。`0` は非表示。99 超は "99+"。Pull は `toolbar.behind`、
  Push は `toolbar.ahead` を渡す。ラベルは "Pull"/"Push" 固定に戻した(数字はチップへ移動)。
- **Undo**: ラベルは "Undo" 固定。`undo_on` かつ summary が非空のとき
  `undo_tooltip_text = Some("Undo: “<summary>”")` を組み、`.when_some(..).tooltip(..)` で
  `Tooltip::new(text)` を表示(sidebar.rs の name_tooltip と同方式)。truncate 廃止。
- **disabled**: 文字・アイコンとも `text_muted`。クリック時の理由 footer 設定ロジックは
  既存のまま温存(ハンドラは enabled/disabled 両方で配線、disabled 経路は理由を footer へ)。
- **ヘッダ高**: 34px → **52px**(`h(px(52.0))`)。左の repo/branch 表示と右の Refresh は
  親 `items_center` で縦センタリング維持。`sep()` は h16px のまま中央寄せ。
- **`INSPECTOR_TOP_OFFSET`**: `30 + 1 + 34` → **`30 + 1 + 52` = 83.0** に更新
  (実測 bounds が主経路、本定数は startup fallback)。

### 検証
- `cargo test` 全パス(test result: ok。failures 0)。own-code warning 0(mod.rs/assets.rs)。
- headless fixture 回帰なし:
  - main(ahead=1): `[kagi] toolbar: pull=off (behind=0) push=on (ahead=1) stash=on pop=on undo=on`
  - feature/two(behind=1): `[kagi] toolbar: pull=on (behind=1) push=off (ahead=0) stash=on pop=on undo=off`
  - sidebar/statusbar ログも不変。
- `KAGI_WINDOW=1000x700` 小窓で panic/error なし、レイアウト破綻なし。
