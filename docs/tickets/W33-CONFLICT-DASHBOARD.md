# W33-CONFLICT-DASHBOARD: Right Panel Dashboard + Skip + Escape Hatch (Phase 2/5/6)

- Status: **done** / 担当: Opus lane
- 仕様: requirements-conflict-ux.md v2 §2.3/2.6/3.5 + ADR-0063(dashboard)/0067(continue/skip)/0060(外部)/0058/0065
- チケット: T-CONFLICT-011〜015, 042, 043/044, 050〜052

## スコープ

1. `src/ui/conflict_view.rs` を **Conflict Dashboard**(右パネル）に拡張(W30 の banner+list を発展)
2. Skip(T-042): `src/git/conflicts.rs` に sequencer skip plan/execute を追加(rebase/cherry-pick/revert のみ)
3. Continue 前 checklist 拡張(T-043/044): `continue_blockers()` で構造化
4. Escape Hatch(T-050〜052): 外部 merge tool / 内蔵 terminal / copy path / copy git command

## 実装メモ(MVP vs v0.2)

### MVP(本フェーズ完了)
- Dashboard を **右パネル**として描画(`render_dashboard`)。中央は W30 の per-file choose+preview を維持
  (W32 の Conflict Editor lane が `ConflictMode.editing_file` を読んで中央を置換する)
- ヘッダ `Merge conflicts detected` + operation summary(方向文言 Merging/Rebasing/Cherry-picking/Reverting
  X onto Y、ADR-0058、ours/theirs 非表示)
- Current / Incoming の **役割+実名 badge**(tooltip に内部 git ステージ補足)
- conflicted / resolved count + prev/next 未解決ナビ
- **Path / Tree toggle**: Path 機能、Tree は disabled プレースホルダ(tooltip で v0.2 案内)
- **Conflicted Files / Resolved Files の 2 セクション** + type badge(ConflictKind、ADR-0065)
- Conflicted 行クリックで `editing_file` をセット(W32 へのハンドオフ。editor 本体は触らない)
- アクション: Abort(常時・**二段確認**、保存済み resolution 消失の警告表示)/ Continue(ゲート)/
  Skip(sequencer のみ・merge は非表示)/ external tool
- Mark resolved: `Mark selected file resolved` / `Mark all clean files resolved`
  (marker 無し & resolution draft あり のみ)。`Mark all resolved` は **MVP 非提供**(Advanced、ADR-0063)
- Continue ゲート強化(`kagi::git::continue_blockers`): unresolved / marker 残 / binary 未解決 /
  required-file 削除未決 / index untracked unmerged / merge message 空。**具体的な blocker 理由を UI に表示**
  (ダッシュボードの理由行 + Refused 時のローカライズ toast)
- Skip: `plan_conflict_skip` / `execute_conflict_skip`(plan 経由・oplog 記録・現 step を安全に破棄、
  force/reset --hard/clean 不使用、buffer は ADR-0057 で退避)
- Escape hatch: external tool は settings.json `mergetool`($LOCAL/$BASE/$REMOTE/$MERGED 置換、未設定なら
  設定方法を案内・**既定ツールは捏造しない**)/ 内蔵 terminal を repo root で開く / conflict path コピー /
  git command コピー(`git <op> --continue|--abort|--skip`)
- i18n: 全 prose を Msg(en+ja)。ours/theirs は一切表示しない

### v0.2 以降(本フェーズ対象外)
- **Folder/Tree grouping + search**(T-CONFLICT-DASH-021 で非機能 Path/Tree toggle を撤去済み。
  conflicted file の folder grouping / tree view / path search は将来 dedicated ticket で再導入予定)
- Tree view 実機能 / external tool の 3-side($LOCAL/$BASE/$REMOTE を個別ファイル materialize)/
  watcher での外部解決取り込み / Advanced "Mark all resolved" / Skip の multi-step sequencer drive
  (現状は現 step 破棄まで。次 pick への前進は dedicated sequence executor 待ち、continue の `Staged` 同様)

## 触らなかったもの
- `src/ui/conflict_editor.rs`(W32 が新規作成)/ resolution.rs の hunk 拡張 / Cargo.toml / vendor
