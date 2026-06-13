# W32-CONFLICT-EDITOR: hunk 単位 Conflict Editor(Phase 3 + Phase 4 hunk actions + log)

- Status: in-progress / 担当: Opus lane
- 仕様: requirements-conflict-ux.md v2 §2.4/2.5 + ADR-0064(layout)/ 0057(buffer)/ 0066(marker)/ 0058(用語)
- チケット: T-CONFLICT-020〜025, 030〜035

## スコープ

1. `src/git/resolution.rs` を **hunk 単位**に拡張: conflicted file を zdiff3 で hunk 列に分割
   (conflict region と非 conflict region)。各 conflict hunk に Current(A)/Incoming(B)/Base の
   行と、選択(AcceptCurrent/AcceptIncoming/BothCurrentFirst/BothIncomingFirst/Manual(text)/Reset)。
   Result は hunk 選択から組み立て、行ごとの provenance(current/incoming/manual)を保持。
   既存 file 単位 API は壊さない(W30 が使用)。unit test(hunk 分割・各 accept・reset・provenance)
2. `src/ui/conflict_editor.rs`(新規): ADR-0064 のレイアウト
   - Top Toolbar: file path / `conflict n of m` / prev / next / Open external tool(導線のみ、実装は W33)/ Reset / Save
   - Upper Split: A=Current branch side / B=Incoming side(uniform_list 仮想化、ズーム対応 scaled_px)
   - Lower: Result/Output preview(由来 side を行ごとに表示、未解決 hunk 明示)
   - hunk ボタン(文言明確): Accept current / Accept incoming / Accept both: current then incoming /
     Accept both: incoming then current / Edit result / Reset this hunk
3. `src/ui/mod.rs`: conflict file クリックで editor を開く状態(例 `conflict_editing: Option<PathBuf>`)+
   render 分岐(editing 中は conflict_editor を表示)。**conflict_view.rs(Dashboard)は触らない**
   (W33 が所有)。Dashboard ↔ Editor の往復は最小の状態フラグで
4. Save(T-034): marker 検査(Save=warning / ADR-0066)+ buffer 永続化 + file を resolved candidate に。
   Resolution action を Operation Log に記録(T-035, session id + hunk action + before/after hash)

## 触ってよい/いけない
- 触ってよい: `src/git/resolution.rs` / `src/ui/conflict_editor.rs`(新規)/ `src/ui/mod.rs`(editor 起動+
  render 分岐+hunk dispatch のみ)/ `src/ui/i18n.rs`(Msg 追加)/ tests/ / 本チケット
- 触らない: `src/ui/conflict_view.rs`(W33)/ `src/git/conflicts.rs` のロジック(読むだけ)/ ops.rs / Cargo.toml / vendor

## 規約
- in-memory(Save=buffer、index 反映は continue 時)。chars() のみ・バイトスライス禁止。
  theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。`cargo test --workspace` green。
  fixture のみ。完了時メモ + Status: done。worktree に commit(push/merge しない)
