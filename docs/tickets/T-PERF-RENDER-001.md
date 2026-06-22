# T-PERF-RENDER-001: render() の同期 I/O を UI スレッドから退避

- Status: todo
- Group: anti-pattern / perf (refactor-plan Step 3.1 / 3.6)
- 仕様の正: ADR-0116 Wave 2

## スコープ

`render()` が UI スレッド上で同期 Git/ファイル I/O を実行している（GPUI の
「render は純粋・I/O 禁止」規範違反）。

- `src/ui/render.rs:341` `self.detect_conflict_mode()` → `src/ui/mod.rs:2021`
  `Backend::open()` / `:2033` `detect_conflict_session()`（index 読み）
- `src/ui/render.rs:355-362` reflog seed → `src/ui/mod.rs:2061`
  `ResolutionBuffer::load()`（ファイル I/O）
- `src/ui/render.rs:356` `self.history_seed_attempted = true`（render 内 mutation）
- `src/ui/render.rs:438` 付近 `ensure_auto_fetch_ticker`（render から毎フレーム呼出）

対応:
1. conflict-detect / reflog-seed の I/O を render から外し、**reload / タブ切替の
   確定タイミング**で `cx.background_spawn`（snapshot/読み込み）→ `cx.spawn` で
   marshal-back し `this.update(cx, |..| { ...; cx.notify() })` 反映。run-once
   ガード（`history_seed_attempted` / `*_detected_for`）はその async 経路で管理。
2. `ensure_auto_fetch_ticker` の呼び出しを render から除去し、app init / tab 確定で
   armする（refactor-plan Step 3.6 の「render から外す」部分。remote-URL 単位の
   グローバル化までは本チケットでは必須としない）。

参考: 既に存在する `refresh_working_tree_external`（`mod.rs:1826` 付近）の
background_spawn + marshal-back パターンに倣う。

## 完了条件

- [ ] `render()` 経路から `Backend::open` / index 読み / ファイル I/O が消える
- [ ] conflict mode / reflog seed が reload・タブ切替時の非同期経路で初期化される
- [ ] 振る舞い不変（ヘッドレス `KAGI_*` 契約行・conflict 検出・履歴 seed が従来通り）
- [ ] `cargo build` + `cargo test --workspace` green、`cargo fmt --check` clean
- [ ] **UI 目視検証 pending** を明記（subagent は GUI を起動できない）
- [ ] 実装メモを末尾に追記

## 規約

- `[kagi]` 契約行は不変。`cx.background_spawn` + `cx.spawn` marshal-back の既存
  idiom を踏襲し、generation/run-once チェックを捨てない（refactor-plan の注意点）
