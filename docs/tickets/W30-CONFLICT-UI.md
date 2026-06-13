# W30-CONFLICT-UI: Conflict Mode UI(banner / file list / choice / Result preview)

- Status: in-progress / 担当: Opus lane
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
