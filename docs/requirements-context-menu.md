# 要件定義: Commit Context Menu(コミット起点の履歴操作)

- Date: 2026-06-12
- 発端: ユーザー要件(原文は本ファイルとチケットに反映)
- 関連 ADR: 0020〜0026
- 実装担当: codex 5.5 high(worktree lane 方式、PM が merge・検証)

## 目的

Commit Graph を「閲覧画面」から「履歴操作の起点」にする。commit を右クリックすると
選択 commit と repository 状態に応じた Context Menu が出て、そこから安全に操作できる。

**不変条件(既存の安全方針を継承)**:
- repository 状態を変更する操作は必ず OperationPlan(= GitOperationPlan)を生成し、
  確認画面を経由する。Context Menu から直接実行しない
- destructive 操作は二段階確認。force push は絶対に自動提案しない
- 実行後は snapshot / graph / right panel / status bar を refresh し、Operation Log に記録する

## ギャップ分析(2026-06-12 実装時点)

| 操作 | 既に実装済み | 今回やる | later |
|------|--------------|----------|-------|
| 右クリック menu 基盤 | なし(W2-DELETE で hover-group 不安定のため見送った経緯) | **commit row 右クリック → menu overlay、状況別出し分け、disabled 理由表示** | キーボードショートカット |
| Show commit details | selection + Inspector | menu から selection を呼ぶだけ | — |
| Copy SHA | Inspector に full SHA copy あり | **short SHA / commit message copy 追加 + menu 統合** | Copy commit link |
| Create branch here | plan_create_branch + dialog(T-HT-001) | **menu 統合 + 「作成後 checkout」オプション(T-HT-005 を吸収)** | — |
| Create worktree here | なし | **ADR-0025 + plan/execute + path 入力 dialog + Navigator WORKTREES セクション** | — |
| Create tag here | なし | — | v0.2 |
| Cherry-pick onto current | in-memory plan + 実行(T015、conflict 予測 blocker) | **menu 統合 + availability 判定(HEAD 選択時 disabled / merge commit disabled / dirty warning)** | merge commit の parent 選択 |
| Revert this commit | なし | **in-memory revert(cherry-pick と同パターン)+ plan + merge commit は disabled** | merge revert(parent 選択) |
| Apply patch | なし | — | v0.2 |
| Checkout this commit | branch checkout のみ(plan_checkout) | **commit checkout(detached HEAD 警告必須 + branch 作成推奨)/ commit 上の branch/tag があれば branch checkout を提示** | — |
| Compare with HEAD | なし | **commit↔HEAD の changed files + diff(read-only、ADR-0026)** | Compare with selected commit |
| Compare with working tree / local changes | WIP diff は commit panel 経由のみ | **commit↔working tree 比較(local changes なしなら disabled)** | — |
| Reset to this commit | なし | **ADR-0024 + menu に disabled 表示(理由 tooltip)のみ** | soft/mixed 実装(ADR 承認後)。hard は二段階確認 + 喪失明示の設計後 |

## Menu 構成(グループ・上から安全操作順)

ヘッダ: `<short SHA> <commit title(切り詰め)>`

1. **Inspect**: Show commit details / Copy SHA / Copy short SHA / Copy commit message
2. **Create from this commit**: Create branch here… / Create worktree here…(/ tag, patch = later 非表示)
3. **Apply changes**: Cherry-pick onto current branch… / Revert this commit…
4. **Checkout / Move**: Checkout this commit… / Checkout '<branch>'(commit 上に branch/tag label がある場合のみ)
5. **Compare**: Compare with HEAD / Compare with working tree / Show changed files
6. **Advanced / Dangerous**(赤系・警告アイコン): Reset current branch to this commit…(MVP は disabled)

## 状況別 availability(正準表。実装は ADR-0021 の純関数)

文脈フラグ: `is_head` / `is_ancestor_of_head` / `is_merge` / `dirty` / `detached` /
`has_local_changes` / `refs_here`(commit を指す branch/tag)

| 項目 | HEAD 選択 | HEAD より過去 | 別 branch 上 | merge commit | dirty WT | detached HEAD |
|------|-----------|---------------|--------------|--------------|----------|---------------|
| Show details / Copy 系 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Create branch here | ✓ | ✓ | ✓ | ✓ | ✓ | ✓(推奨表示) |
| Create worktree here | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Cherry-pick onto current | ✗ disabled「HEAD と同一」 | ✗ disabled「既に到達可能」 | ✓ | ✗ disabled「merge commit は MVP 対象外」 | ✓+警告 | △ target branch なし → disabled「detached HEAD」 |
| Revert this commit | ✓(到達可能なら) | ✓ | ✗ disabled「現在 branch に含まれない」 | ✗ disabled(MVP) | ✓+警告 | ✗ disabled |
| Checkout this commit | ✗ disabled「既に HEAD」 | ✓+detached 警告 | ✓+detached 警告 | ✓+警告 | ✓+強警告 | ✓ |
| Checkout branch/tag here | —(現 branch) | refs_here あれば ✓ | ✓ | refs_here 依存 | ✓+警告 | ✓ |
| Compare with HEAD | ✗ disabled「同一」 | ✓ | ✓ | ✓ | ✓ | ✓ |
| Compare with working tree | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| (local changes なし) | Compare with working tree → disabled「local changes がありません」 | | | | | |
| Reset to this commit | disabled「不要(HEAD と同一)」 | disabled(MVP、ADR-0024) | disabled(MVP) | disabled(MVP) | disabled(MVP) | disabled「現在 branch がありません」 |

- disabled 項目は **理由を tooltip で表示**(非表示にするのは later 機能のみ)
- cherry-pick の「既に到達可能」判定: `graph_descendant_of(HEAD, target) || HEAD == target`

## GitOperationPlan 統合(ADR-0022)

既存 `OperationPlan` を拡張して使う(新規 struct は作らない):

| 要求フィールド | 対応 |
|----------------|------|
| operation kind | `title`(既存)+ oplog の op 名 |
| selected commit SHA / current HEAD / branch / upstream / dirty | `current`(StateSummary)+ plan 本文に明記(既存パターン) |
| expected result | `predicted`(既存) |
| warnings / blockers | 既存 |
| destructive flag | **新規 `destructive: bool`**(true → 確認ボタン赤 + 二段階確認) |
| rollback hint | `recovery`(既存) |
| commands or API calls | plan モーダルの説明文に明記(既存パターン) |
| affected files | `preview_files`(既存) |

**handler 一元化**: `CommitAction` enum + `dispatch_commit_action(action, commit_id)` を導入し、
Context Menu と Inspector の Actions セクションが**同じ handler** を呼ぶ(二重実装禁止、T-CM-062)。

## 完了条件(原文対応)

- [ ] commit 右クリックで Context Menu(ヘッダ = short SHA + title)
- [ ] 状況別出し分け + disabled 理由表示(availability は unit test 付き)
- [ ] Copy SHA / short SHA / commit message が動作
- [ ] Create branch here が plan 経由 + checkout オプション付きで動作
- [ ] Create worktree here が plan 経由で動作し Navigator に WORKTREES が出る
- [ ] Cherry-pick / Revert が plan 経由で動作(merge commit は disabled)
- [ ] Checkout this commit(detached 警告 + branch 作成推奨)が plan 経由で動作
- [ ] Compare with HEAD / working tree が read-only で動作
- [ ] Reset は Advanced/Dangerous 配下に disabled で表示(ADR-0024 済み)
- [ ] Context Menu と Inspector Actions が同一 handler
- [ ] 操作後 refresh + Operation Log 記録
