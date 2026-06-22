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

## 実装メモ（T-PERF-RENDER-001 / ADR-0116 Wave 2 実施）

- Status: done（cargo build/test/fmt/clippy green） / **UI 目視検証 pending**
  （subagent は GUI を起動できない）

### render() からの I/O 退避

`src/ui/render.rs` の `render()` から次の3点を撤去（コメントで退避先を明記）:

1. `detect_conflict_mode()` の同期呼び出し
2. reflog seed ブロック（`Backend::open` + `seed_history_from_reflog` +
   `history_seed_attempted` の render 内 mutation）
3. `ensure_auto_fetch_ticker(cx)` の毎フレーム呼び出し

これらは新設の `KagiApp::ensure_startup_repo_io(cx)`（`src/ui/mod.rs`）に集約し、
**reload しない確定タイミング**＝以下の2経路で1回だけ起動するようにした:

- `switch_repo`（タブ切替／`open_repository` 経由のファイル選択・Recent・
  single-instance）— ローカルタブ分岐の末尾、`load_repo_async` の直前。
- `open_main_window` の app init（CLI 起動の初期タブは `switch_repo` を通らず
  main.rs が手組みするため、render が初フレームで担っていた分をここで arm）。

### conflict-detect の非同期化（I/O と状態適用の分離）

`detect_conflict_mode` を「I/O 半分」と「状態適用半分」に分割:

- `detect_conflict_payload(repo_path, prev_selected, prev_editing_file,
  current_branch) -> ConflictDetectOutcome`（`self` 非依存・全 I/O：
  `Backend::open` / `detect_conflict_session` / `ResolutionBuffer::load` /
  residue・status 再計算 / 選択ファイルの zdiff3 マーカー materialize）。返り値は
  `Send`（`ConflictDetectOutcome`、大きい `Detected` は `Box<ConflictDetected>`）。
- `apply_conflict_detect(outcome)`（UI スレッド：`self.conflict` 反映と
  `[kagi] conflict-mode: …` / `conflict-mode: cleared` /
  `conflict-mode: merge resolved — ready to commit` の発行）。

これにより:

- 同期 `detect_conflict_mode()` = `payload` を同期実行 → `apply` で**従来と
  バイト等価**（`reload` / `conflict.rs` ステージ後の sync caller はそのまま）。
- 新設 `detect_conflict_mode_async(cx)` = `payload` を `background_spawn` →
  `cx.spawn` で marshal-back → `apply` + `cx.notify()`。

`OpenFailed` 分岐は元コードの早期 return（`merge_commit_ready` を触らない）を
忠実に再現。`merge_commit_ready=false` の設定順、cleared/merge-ready の klog 順序も
元と一致。

### reflog seed の非同期化

`seed_history_from_reflog`（`&Backend` を取る既存 sync 版＝`reload` 用）は
`apply_reflog_seed(Result<Vec<HistoryEntry>,String>)` 経由に内部リファクタ。新設
`seed_history_from_reflog_async(cx)` が `Backend::open` + reflog 読みを
`background_spawn` で実行し、marshal-back 時に **only-when-empty を再判定**してから
`apply_reflog_seed`（in-flight 中の `record_history` を clobber しない）。

### run-once ガードの扱い（捨てていない）

- conflict: `conflict.detected_for`（repo path 単位）。async 版は **I/O 起動前に**
  guard を arm（元の「set guard, then I/O」順を踏襲）。`reload()` /
  `reset_per_repo_ui`（タブ切替）/ `conflict.rs` ステージ後の `detected_for=None`
  リセット挙動は不変。
- history: `history_seed_attempted`。`ensure_startup_repo_io` 内で立ててから
  async seed を起動。`reset_per_repo_ui` の `=false` リセット（タブ切替で再 seed）も
  不変。
- auto-fetch: `auto_fetch_ticker_alive`（既存の自己ガード）。挙動そのまま、呼出元を
  render → init/tab 確定へ移しただけ（remote-URL 単位グローバル化は本チケット外）。

### ヘッドレス契約への影響

`init_tab` / `run_repo_flow`（`src/headless.rs`）は `reload()` を**同期**で呼び、
そこで `detect_conflict_mode`（sync）/`seed_history_from_reflog`（sync）が従来通り
実行・klog 発行される。CLI 通常起動（`from_snapshot`）は `reload` を通らないため、
従来は render が担っていた分を `open_main_window` の async 経路へ移行（出力文字列・
回数は不変、タイミングのみ毎フレーム→確定時へ）。`tests/`（conflicts_test 等）は
git レイヤ直叩きで render 経路に非依存、`conflict-mode`/`history: seeded` の
直接アサートは存在しないことを確認済み。

### 検証結果

- `cargo build --workspace`: OK
- `cargo test --workspace`: 791 passed / 0 failed（conflicts/undo_redo/undo/amend
  含む全 green）
- `cargo fmt --check`: clean
- `cargo clippy --workspace`: 自分の diff に新規警告なし（baseline 41 → 38、
  既存 `len_zero` を2件解消）

### 注意（並行レーンとの干渉）

作業中、別レーン（Wave 3 `T-SPLIT-PULLPUSH-001`）が `crates/kagi-git/src/ops/`
を分割中で working tree が一時的にビルド不能になることがあった。本チケットの変更は
UI 4 ファイル（`src/ui/mod.rs` / `render.rs` / `tabs.rs` /
`operations/history.rs`）のみで kagi-git には非依存。`push_test` の単発失敗は
当該レーンが `pull_push.rs` を削除中だったことに起因（隔離して再実行で pass を確認）。
