# W29-I18N-WAVE2: i18n 漏れ掃除(validation + 残 UI 文言)

- Status: done / 担当: Opus lane

## 完了メモ(実装結果)

### keyed enum 設計(src/git/ops.rs)
- `BranchNameError`(9 variant、create/rename で文言が違うため call-site 別):
  `EmptyCreate`("Branch name must not be empty.") / `Required`("Branch name is required.") /
  `Whitespace` / `SameName` / `RenameExists(String)` / `RenameInvalid(String)` /
  `CreateInvalidRef(String)` / `CreateLeadingDash(String)` / `CreateExists(String)`。
  `Display` は**現行英語文言を完全維持**(テスト pin 保護)。
- `WorktreePathError { Empty, Exists(String) }`(2 reason のみ keyed)+ `Display`=英語。
- `WorktreeValidationError { Keyed(WorktreePathError), Other(String) }`:
  `validate_worktree_path_keyed` が返す。`validate_worktree_path`(従来 String 版)は
  これの `to_string()` shim に変更(後方互換・テスト維持)。
- `BranchRenameValidation::Invalid(String)` → `Invalid(BranchNameError)` に変更。
  `validate_branch_rename` は keyed を返す。ops.rs 内の plan/execute rename 呼び出し側は
  `reason.to_string()` で英語 blocker に展開(テスト維持)。
- `plan_create_branch` の branch-name blocker は新 free fn `create_branch_name_errors`
  (keyed を返す)から `Display` で英語 blocker に展開。commit-existence blocker は
  非 keyed(英語のまま)。`OperationPlan` の struct には**フィールド追加せず**
  (conflicts.rs 等 30+ literal/W30 領域を触らないため)、UI 側にキーを渡す方式。

### UI 写像(src/ui/i18n.rs / mod.rs)
- i18n.rs: `Msg` に `BranchNameEmpty / BranchNameRequired / BranchNameWhitespace /
  BranchNameSame / WorktreePathEmpty` を追加(en+ja)。引数つきは helper:
  `branch_exists_fmt / branch_rename_exists_fmt / branch_invalid_ref_fmt /
  branch_rename_invalid_fmt / branch_leading_dash_fmt / worktree_exists_fmt`(en+ja)。
  写像 fn `branch_name_error(&BranchNameError)->String` / `worktree_path_error(&WorktreePathError)->String`。
- mod.rs: `CreateBranchModal` / `CreateWorktreeModal` に `localized_blockers: Vec<SharedString>`
  追加。replan で keyed errors を localize し、非 keyed blocker は英語のまま pass-through
  (free fn `localize_plan_blockers`)。render は localized を優先表示。
  rename modal は `render_input_plan_modal` で `branch_name_error` を呼んで localize。
- ja 翻訳が付いた validation messages: name required(空)/ same name / already exists
  (create+rename)/ invalid ref name(create+rename)/ leading dash / worktree path empty /
  worktree path exists。

### task3 残 UI 文言 sweep(非ドメイン prose のみ)
- inspector.rs: "No file changes" / "(diff unavailable)" / "Co-authored by" /
  "… and N more"(helper)を Msg/helper 化。
- tabs.rs: "Ready" / "Loading <name>…"(helper) / Welcome の
  "No repository open. …" を Msg/helper 化。
- branch_menu.rs: "No upstream set" を Msg、"Copied <x>" toast 3 箇所を `copied_fmt`。
- **英語維持(ADR-0048 ドメイン語/操作名ラベル)**: メニュー action ラベル
  (Checkout/Open, Pull ff-only, Create branch from here…, Delete branch… 等)、
  sidebar "filter…"(filter は明示ドメイン語)、"Open Repository…"、列ヘッダ、SHA、branch 名。

### 検証
- cargo build: own-code warning 0(`block v0.1.6` は vendor の future-incompat 警告のみ)。
- cargo test --workspace: green(exit 0)。i18n keyed の新 unit test 2 本追加
  (Display=英語 pin / lang 切替)。conflicts_test の "could not apply … side change" は
  fixture が意図的に作る cherry-pick conflict の git stderr で failure ではない。

### wave 3 へ明示繰り延べ(本チケット対象外)
- merge/checkout/delete/pull/push/amend/discard 等 OperationPlan の
  blocker/warning/recovery 全般の翻訳(多数・テスト pin・keyed 横展開要)。
- worktree-path の残 reason(parent 不在 / repo 内 / not accessible 等)は
  `WorktreeValidationError::Other` で英語のまま。
- create-branch の commit-existence blocker、checkout-after blocker、
  branch-checked-out-elsewhere blocker は英語のまま。

---

- (元メモ) Status: in-progress / 担当: Opus lane
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
