# W33-CONFLICT-DASHBOARD: Right Panel Dashboard + Skip + Escape Hatch(Phase 2/5/6)

- Status: in-progress / 担当: Opus lane
- 仕様: requirements-conflict-ux.md v2 §2.3/2.6/3.5 + ADR-0063(dashboard)/ 0067(continue/skip)/ 0060(外部)/ 0058
- チケット: T-CONFLICT-011〜015, 042, 043, 050〜052

## スコープ

1. `src/ui/conflict_view.rs` を **Conflict Dashboard** に拡張(W30 の banner+list を発展):
   - ヘッダ `Merge conflicts detected`(op 別)+ operation summary(方向文言、ADR-0058)
   - current / incoming の **役割+実名 badge**(ours/theirs 非表示、tooltip 補足)
   - conflicted count / resolved count
   - **Conflicted Files / Resolved Files の 2 セクション分離** + type badge(ADR-0065 の ConflictKind)
   - Path / Tree toggle(**MVP は Path のみ機能**、Tree は表示だけ or disabled)
   - Abort / Continue / Skip / external tool ボタン(下記)
   - Mark resolved 系: `Mark selected file resolved` / `Mark all clean files resolved`(marker無し&index
     resolved のみ)。`Mark all resolved` は Advanced(本 MVP では出さない or disabled)
2. Skip(T-042): `src/git/conflicts.rs`(or ops.rs)に sequencer skip plan/execute を追加
   (rebase/cherry-pick/revert のみ。merge は非表示)。Plan 経由(ADR-0067)
3. Continue 前 checklist 拡張(T-043): unresolved/marker(済)に加え index resolved / binary 残 /
   required file 削除 / commit message 非空 / checklist blocker を加味して can_continue を厳密化
4. Escape Hatch(T-050〜052):
   - Open external merge tool(settings の mergetool、$LOCAL/$BASE/$REMOTE/$MERGED 置換。
     未設定なら設定方法を案内)
   - Open terminal at repo root(内蔵 terminal)
   - Copy conflict file path / Copy git command(`git <op> --continue|--abort|--skip`)

## 触ってよい/いけない
- 触ってよい: `src/ui/conflict_view.rs` / `src/git/conflicts.rs`(skip + checklist 強化)/
  `src/ui/mod.rs`(dashboard render + escape/skip 配線のみ)/ `src/ui/i18n.rs`(Msg)/ ops.rs(skip plan 必要時)/
  tests/ / 本チケット
- 触らない: `src/ui/conflict_editor.rs` と resolution.rs の hunk 拡張(W32)/ Cargo.toml / vendor。
  file クリックで editor を開く配線は W32 が入れるので、Dashboard 側は「ファイル選択状態」を更新するだけ

## 規約
- Plan 経由(ADR-0067)。chars() のみ・バイトスライス禁止。theme()・i18n Msg(ours/theirs 非表示)。
  own-code warning 0。`cargo test --workspace` green。fixture のみ。完了時メモ + Status: done。
  worktree に commit(push/merge しない)
