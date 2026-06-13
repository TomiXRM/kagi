# W32-CONFLICT-EDITOR: hunk 単位 Conflict Editor(Phase 3 + Phase 4 hunk actions + log)

- Status: done / 担当: Opus lane
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

## 完了メモ(実装結果)

### MVP として実装したもの
- **hunk model**(`resolution.rs`): `Region::{Passthrough(Vec<String>) | Hunk(ConflictHunk)}` の
  順序付きリスト。`ConflictHunk { current, incoming, base, choice: HunkChoice }`。
  `HunkChoice = AcceptCurrent | AcceptIncoming | BothCurrentFirst | BothIncomingFirst |
  Manual(String) | Unresolved`。`LineOrigin` に `Context`(passthrough 行)を追加。
  `HunkModel::from_marker_text`(zdiff3 `<<<`/`|||`/`===`/`>>>` を chars()-safe な `\n` split で
  パース)/ `set_choice` / `assemble`(行ごと provenance 付き)/ `assembled_text`。
  未解決 hunk は assemble 時に marker を再出力 → marker gate が確実にトリップ。
- `ResolutionBuffer` に **in-memory hunk 編集 API**: `ensure_hunks`(idempotent, materialization から
  構築)/ `apply_hunk_choice`(再アセンブル → file Result に commit + checkpoint で file-level undo
  互換)/ `reset_hunk` / `hunk_count` / `hunks_all_resolved` / `hunk_model`。既存 file 単位 API は不変。
- **Conflict Editor**(`conflict_editor.rs`): ADR-0064 レイアウト。Top Toolbar(back/path/`conflict n/m`/
  prev/next/Open external tool/Reset all/Save)、Upper split A|B(uniform_list 仮想化、hunk ごとに
  6 つの **文言ボタン**)、Lower Result/Output(行ごと由来タグ + 未解決数表示、選択で即更新)。
  全 prose は Msg(en+ja)、scaled_px、ours/theirs 非表示。
- **mod.rs 配線**: `conflict_editing: Option<PathBuf>` + `conflict_editing_before_text`。
  content conflict を activate → `conflict_open_editor`(repo から materialize → ensure_hunks)。
  render 分岐(conflict Some かつ editing Some → editor、それ以外は従来通り)。
  `conflict_editor_apply_hunk` / `reset_all` / `nav_hunk` / `open_external`(導線のみ + toast)/ `save`。
- **Save(T-034/T-035)**: marker 検査は **warning**(Continue が hard gate)。buffer autosave +
  file を resolved candidate に。oplog(既存 `append_oplog` 再利用、新ログファイルは作らない)へ
  `conflict-save:<op>` を記録(session id + hunk action slug + before/after の FNV ハッシュ)。

### v0.2 送り(MVP 外)
- 外部マージツールの実起動(W33)/ Open external tool は導線 + toast のみ。
- in-app テキストエディタによる本格的な手編集。現状 "Edit result" は current 側を seed にした
  Manual 編集を commit(provenance 経路と文言は満たすが、自由入力 UI は未)。
- prev/next は MVP では Dashboard の unresolved-file ナビを再利用して隣接ファイルへ。
  hunk 内スクロールナビ(同一ファイル内 hunk へのジャンプ)は v0.2。
- hunk ごとの undo(現状は file-level undo を共有)/ syntax highlight / minimap / semantic 区切り。
- hunk model 自体は in-memory(restart 後は materialization から再構築。assembled result は
  buffer.json に永続化されるが per-hunk choice は再構築されない)。

### 検証
- `resolution.rs` unit: 19 tests green(multi-hunk split / 各 accept / both 両順 / manual / reset 再 marker /
  全解決 marker-free / buffer 経由 apply+undo / ensure_hunks idempotent)。
- `tests/conflicts_test.rs`: 実 repo の 2-hunk merge conflict を zdiff3 materialize → 2 hunk に分割 →
  hunk0=current / hunk1=incoming → Result が marker-free・provenance に Current/Incoming/Context、
  reset で marker 残存 → `plan_conflict_continue` が blocker を返すことを確認。
- `cargo test --workspace` 全 36 バイナリ green、own-code clippy/warning 0。
