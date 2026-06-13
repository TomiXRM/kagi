# W30-CONFLICT-UI: Conflict Mode UI(banner / file list / choice / Result preview)

- Status: done(MVP)/ 担当: Opus lane
- 依存: W26(backend 済み: src/git/conflicts.rs / resolution.rs — detect_conflict_session /
  ResolutionBuffer / side_labels / plan_conflict_continue / plan_conflict_abort)
- 仕様: requirements-conflict-ux.md + ADR-0056〜0059。tickets T-CONFLICT-002/003/004/006/007 + 008/009 の UI 半分

## スコープ(MVP UI)

1. `RepoMode`(Normal/Conflict)を KagiApp に保持し、起動時 + watcher + 操作後に
   `detect_conflict_session` で出入り(ADR-0056)。CLI 起因も検出
2. **常設バナー**(header 直下): op 名 + 進捗 N/M + Continue(全解決まで disabled)/ Abort /
   Skip(sequencer のみ)。用語は side_labels(ours/theirs を出さない、ADR-0058)
3. **conflict file list**: unresolved/resolved/needs-review、kind(content/rename-delete/
   modify-delete/binary)アイコン、prev/next 未解決ナビ(KDiff3 流)
4. **ファイル単位 choose**(MVP): Keep current(`<branch>`)/ Take incoming(`<branch/commit>`)/
   Keep both(current first)を ResolutionBuffer に適用。binary は choose のみ
5. **Result preview**: 解決後ファイルの diff を continue 前に表示。marker 残検査ゲート
6. **continue / abort ボタン** → 既存 `plan_conflict_continue/abort`(W26)を呼ぶ(plan→oplog)。
   操作 handler の二重実装禁止。3-pane 編集や hunk 単位は v0.2(本 lane では出さない)

## 触ってよいファイル

- `src/ui/conflict_view.rs`(新規・UI 本体)/ `src/ui/mod.rs`(RepoMode 保持 + render 配線 +
   検出フック)/ `src/ui/i18n.rs`(役割ラベル等の Msg 追加)/ `src/ui/commands.rs`(必要なら
   Continue/Abort のメニュー)/ tests/(必要なら)/ 本チケット
- **src/git/conflicts.rs / resolution.rs のロジックは変更しない**(API を使うだけ)。
   W29 が触る git 層 validation や branch_menu には極力触れない

## 規約

- in-memory 主義(continue まで repo を汚さない)。chars() のみ・バイトスライス禁止。
  theme()・i18n Msg。own-code warning 0。`cargo test --workspace` green。fixture のみ。
  完了時メモ + Status: done。worktree branch に commit(push/merge しない)

## 完了メモ(MVP / 2026-06-13)

実装ファイル: `src/ui/conflict_view.rs`(新規・UI 本体 + 単体テスト3)、
`src/ui/mod.rs`(RepoMode 相当の `KagiApp.conflict: Option<ConflictMode>` +
`conflict_detected_for` guard + 検出/操作メソッド + render 配線)、
`src/ui/i18n.rs`(Conflict Mode 用 Msg 追加 en/ja)。git 層・ops.rs・branch_menu は不変更。

**MVP で入れたもの**:
- `KagiApp.conflict`(= ConflictSession + ResolutionBuffer + current_branch + selected_file)。
  検出は `detect_conflict_mode()` を **reload()**(=watcher の reload_external 経由 + 各操作後)
  と **render() 一回限り guard**(起動 / タブ切替の instant-apply 経路)から呼ぶ。CLI 起因も検出。
  ログ `[kagi] conflict-mode: <op> <n> file(s)` / 解消時 `cleared`。
- 常設バナー(header 直下): op 見出し(rebase は "Rebasing <commit> onto <base> — commit s/t")
  + `N/M resolved` + Continue(全解決 AND marker 残ゼロまで disabled)/ Abort / Skip(sequencer のみ)。
  ラベルは `side_labels`(ours/theirs 不使用)。
- conflict file list: unresolved/resolved/needs-review + kind タグ + prev/next 未解決ナビ。
- ファイル単位 choose: Keep current(<current>)/ Take incoming(<incoming>)/ Keep both(current first)。
  `ResolutionBuffer.apply_choice` に適用 → buffer.autosave()(ADR-0057)。binary は choose のみ・preview なし。
- Result preview: 解決後テキストの scroll box(plain text)。marker 残検査は continue ゲートで実施。
- Continue/Abort は **既存 plan→(blocker gate)→execute→record_op(oplog)→reload→再検出** 経路で
  `plan_conflict_continue`/`execute_conflict_continue`・`plan_conflict_abort`/`execute_conflict_abort` を呼ぶ。

**v0.2 に先送り(本 lane では出していない)**:
- 3-pane(Base/Current/Incoming/Result)エディタ・hunk 単位 choose・アプリ内手編集(set_manual_text の UI)。
- blame-of-sides(原因 commit 表示)、undo/redo の UI、rename-delete/modify-delete/binary の専用編集 UI。
- 外部 merge tool / terminal 連携、diff viewer 流用の color preview。
- **Skip の実体**: backend(W26)に skip planner/executor が無く、ops.rs も本 lane 触らない方針のため、
  Skip ボタンは sequencer op で表示するが押下時は「terminal で `git <op> --skip`」を促す toast に留める
  (実 skip は backend 追加後 v0.2)。

検証: `cargo build` own-code warning 0。`cargo test --workspace` 578 passed / 0 failed
(conflict_view 単体3本追加: continue ゲートが未解決でブロック→解決で開く、marker 残でブロック継続、
見出しが ours/theirs を漏らさない)。実バイナリを実 merge conflict の tempdir で起動し
`[kagi] conflict-mode: merge 1 file(s)` を確認(起動時検出)。
