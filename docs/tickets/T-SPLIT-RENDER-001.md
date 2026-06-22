# T-SPLIT-RENDER-001: render.rs を機能境界で分割

- Status: todo
- Group: god-file split (CLAUDE.md ≤800 LOC/file, ≤80 LOC/fn)
- 仕様の正: ADR-0116 Wave 3

## スコープ

`src/ui/render.rs`（3521行）と、その中の巨大関数を機能境界で分割する。

- `render`（`render.rs:303`、882行）→ header / body / sidebar / bottom-panel /
  overlay の各サブメソッド呼び出しを薄く合成する形にする。
- `render_header_slot`（`render.rs:1265`、584行）、`render_body`
  （`render.rs:2020`、507行）等の巨大メソッドを、責務単位のサブメソッド or
  sibling モジュール（例 `src/ui/render/header.rs` 等、または `render_header.rs`）へ移す。

方針:
- **純粋な `mod`/可視性移動**。振る舞い・出力 DOM・`[kagi]` 契約行は不変。
- 既存の公開パス（他モジュールから呼ばれる `render_*`）は再エクスポート or
  `impl KagiApp` の inherent method として維持し、呼び出し側を壊さない。
- 1ファイル ≤800 LOC を目標。`render` 本体は合成のみで ≤80 LOC を目指す。

**前提**: T-PERF-RENDER-001 / 002 が render.rs を触るため、それらの**後**に着手する
（同一ファイル競合回避）。

## 完了条件

- [ ] render.rs が ≤800 LOC（または明確な機能サブモジュール群に分割）
- [ ] `render` 本体が合成中心の小さなメソッドになる
- [ ] 振る舞い不変（`cargo test --workspace` green、ヘッドレス契約行不変）
- [ ] `cargo fmt --check` clean、自分の diff に新規 clippy 警告を足さない
- [ ] **UI 目視検証 pending** を明記
- [ ] 実装メモを末尾に追記

## 規約

- 機能境界で割る（CLAUDE.md）。ロジック変更を混ぜない（移動のみ）
