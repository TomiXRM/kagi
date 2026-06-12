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

- [ ] 全ボタンがアイコン上・ラベル下の Finder 風(PM スクリーンショット確認)
- [ ] Pull/Push の count がアイコン右上チップで出る(ahead/behind fixture で確認)
- [ ] disabled 状態の見た目と理由 footer が機能維持
- [ ] Undo tooltip に対象 commit summary
- [ ] 既存 headless ログ(toolbar/statusbar)回帰なし
- [ ] `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記

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
