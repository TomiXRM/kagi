# W29-I18N-WAVE2: i18n 漏れ掃除(validation + 残 UI 文言)

- Status: in-progress / 担当: Opus lane
- 発端: 日本語モードで「branch name must not be empty」等が英語のまま(ユーザー報告)
- 仕様: ADR-0048(ours/theirs と同じく Msg 方式、ドメインワード・branch 名は英語維持)

## 問題の所在

ユーザーが見た文言は **src/git/ops.rs の validation / blocker 文字列**(git 層)。
例: `validate_branch_rename`(ops.rs:169「Branch name is required.」等)、
create-branch plan の blocker(ops.rs:699「Branch name must not be empty.」/ :707 invalid /
:725 already exists)、worktree path(ops.rs:842/856)。
git 層は **テストが英語文言を pin** しているため、UI から直接 Msg 化できない。

## 設計(keyed error 方式)

1. git 層の **ユーザー向け検証理由を enum 化**(例 `BranchNameError { Empty, SameName, Exists(String),
   Invalid(String) }`、worktree path も同様)。`Display` は**現行の英語文言を維持**(既存テストの
   pin を壊さない)。`validate_branch_rename` 等は String でなく enum を返すよう変更し、呼び出し側
   (UI)で enum→`Msg`(en/ja)に写像して表示
2. create-branch / worktree plan の **blocker も同じ enum を OperationPlan に添える**経路を用意
   (既存の `blockers: Vec<String>` は英語のまま温存しテスト維持。UI が enum を持つ場合は
   そちらを優先表示)。過剰設計を避け、**今回は branch-name と worktree-path の検証のみ**対象
3. src/ui/*.rs に残るユーザー向け英語リテラル(非ドメイン語)を Msg 化(branch_menu の
   "No upstream set" は Msg 済みか確認、inspector/sidebar/commands/tabs の取りこぼし)

## スコープ外(wave 3 として明記)

- merge/checkout/delete/pull/push/amend/discard 等の **OperationPlan blocker/warning/recovery
  全般**の翻訳(多数・テスト pin・keyed 化の横展開が必要)。本チケットでは触らない

## 触ってよいファイル

- src/git/ops.rs(検証 enum 化、Display=英語維持)/ src/ui/i18n.rs(Msg 追加)/
  src/ui/{mod.rs(validation 表示箇所のみ),branch_menu.rs,sidebar.rs,commands.rs,inspector.rs,tabs.rs}/
  tests/(enum 化に伴う最小修正)/ 本チケット
- **src/ui/conflict_view.rs と Conflict Mode 配線には触れない**(W30 が担当)

## 規約

- chars() のみ・バイトスライス禁止。own-code warning 0。`cargo test --workspace` green。
  fixture のみ。完了時メモ + Status: done。worktree branch に commit(push/merge しない)
