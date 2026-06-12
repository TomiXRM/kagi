# ADR-0007: Bottom Panel Architecture

- Status: Accepted
- Date: 2026-06-12

## Context

Git 操作のログ・terminal・将来の Problems 等を表示する、出し入れ可能な下部パネルが必要
(requirements-shell.md §1)。VSCode / Zed の Bottom Panel と同じ操作モデルにする。

## Decision

1. **AppShell の明示化**: KagiApp の render を「Header Toolbar / Body(Sidebar+Main+RightPanel)/
   Bottom Panel / Status Bar」の縦 flex スロット構造に再編する(T-BP-001)。
   各スロットは関数分割し、Body は既存実装をそのまま移す(挙動変更なし)
2. **状態**: `bottom_panel: { open: bool, height: f32 (クランプ 80〜60%), active_tab: BottomTab }` を
   KagiApp に持つ。高さはセッション内記憶(reload で保持。永続化は later)
3. **タブは trait 化しない**: MVP は `enum BottomTab { OperationLog, Terminal }` の match 描画。
   タブ追加が3つを超えた時点で抽象化を検討(YAGNI)
4. **Operation Log タブ**: T017 の oplog(JSONL)を**メモリ上にもリングバッファ(最大500件)**で保持し、
   タブはそれを新しい順に表示。アプリ起動時に既存 JSONL の末尾 100 件を読み込む
5. **失敗時の自動オープン**: OpOutcome::Failed を記録した時点で `open = true` + active_tab = OperationLog
6. **リサイズ**: 上端に水平ディバイダ(T023/T030 と同じ絶対座標方式: `height = viewport_h - cursor_y - statusbar_h`)
7. **開閉ショートカット**: gpui の KeyBinding(`cmd-j` — VSCode/Zed と同じ)+ Status Bar のアイコン

## Consequences

- render 関数の大規模な構造変更(T-BP-001)が先行リスク → 挙動変更なしの純リファクタとして単独チケット化し、
  スクリーンショット比較で回帰確認する
- Operation Log の二重管理(ファイル + メモリ)は許容(ファイルは永続監査、メモリは表示用)
- Terminal タブの中身は ADR-0008 に従う
