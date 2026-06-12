# W2-STATUS: Status Bar 拡張 + Bottom Panel default 高さ(PM 直接実装)

- Status: done (2026-06-12)
- 担当: PM(main 直接)
- 関連 ADR: 0017

## スコープ

1. `StatusBarSummary.conflict_count`(snapshot の conflicted から)
2. Status Bar チップ追加: `!N` conflict(赤、>0 のみ)/ `⚑N` stash(>0 のみ)/ `→ <upstream名>`
3. headless ログ拡張(prefix 互換): `[kagi] statusbar: <branch> ↑A ↓B staged=N unstaged=M conflicts=K stash=S upstream=<name|->`
4. `FooterStatus::Busy`(⟳ 青)— 操作が非同期化された時の表示先。現状は同期実行のため未構築
   (`#[allow(dead_code)]`、非同期化時に外す)
5. Bottom Panel default 高さ = viewport の 18%(ADR-0017、要件 ≤20%):
   - 構築時は `BOTTOM_PANEL_H_UNSET`(0.0)sentinel
   - 初回 render で `viewport_h * 0.18`(MIN_H=80 下限)に解決し
     `[kagi] bottom-panel: default height=H (18% of viewport V)` を出力
   - KAGI_BOTTOM_PANEL=1 起動ログは解決前なら `height=18%-of-viewport` と表示

## 検証

- `cargo test` 15 suites 全パス、own-code warning 0
- fixture headless: `statusbar: main ↑1 ↓0 staged=0 unstaged=1 conflicts=0 stash=1 upstream=origin/main`
- `bottom-panel: default height=162 (18% of viewport 901)`(1400x900 window)
- スクリーンショットで Operation Log が画面下部 ~18% で表示されることを確認

## 残課題

- 真の background operation 表示は git 操作の非同期化(将来チケット)とセットで Busy を構築する
