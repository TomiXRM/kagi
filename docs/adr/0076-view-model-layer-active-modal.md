# ADR-0076: View-Model Layer and ActiveModal Enum

- Status: Accepted / Date: 2026-06-14
- Context: v1.0 re-architecture. See `docs/rearch/architecture.md` §2.4, research #2/#4/#5/#6.

## Decision

- `kagi-ui` に **view-model 層**を導入する。VM は plain data(`git2` 非依存、gpui 依存最小)で、`RepoSession` のデータから構築され、view は VM を描画し **intent**(`Select(CommitId)` / `RequestOperation(Operation)` 等)を app に返す。代表 VM: `CommitGraphVM` / `InspectorVM` / `DiffVM` / `CommitDraftVM` / `ConflictVM` / `SidebarVM` / `ToolbarVM`。
- view は機能領域ごとに**自己完結したコンポーネント**に分割(graph / inspector+diff / sidebar / commit panel / conflict dashboard + 3-pane editor / terminal / bottom panel / tab strip)。16.7k LOC の単一 god-view を廃止。
- modal は **session ごとに `enum ActiveModal` 1個**(各 modal の lazy `InputState`/focus を保持)で、v0.2.0 の ~25 個の `Option<…Modal>` フィールドを置換。
- `CommitDraftVM` は commit 関連の 10+ フラットフィールド(`commit_input` / `commit_template_*` / `smart_commit` / `pending_smart_msg` / `last_draft_value` / …)を集約し、source-of-truth を plain data(`String`/`TemplateFields`)にして `InputState` をそこから sync する(`pending_smart_msg` ハックを解消)。

## なぜ

- **テスト容易性**: VM が plain data なら window 無しで unit test できる(ADR-0077 のテストピラミッド app/ui 層の前提)。v0.2.0 は VM が `KagiApp` に溶けていて window 無しでは検証不能。
- **god-object/god-file の解消**: ~80 フィールドの `KagiApp` と 16.7k LOC の `mod.rs` をレビュー・変更可能な単位に割る。
- **modal の整理**: 同時に高々1つしか出ない modal を 25 個の `Option` で持つのは無駄でバグの温床。enum なら排他性が型で保証される。

## 代替案

1. view が直接 `RepoSession` を読む(VM 無し)。
2. 本決定の VM 層 + intent。
3. 完全な単方向 (Elm/TEA) アーキテクチャ。

## 捨てた案

- 案1: テスト容易性が出ず、view にロジックが溜まり god-view が再発。却下。
- 案3: TEA の純度は魅力だが gpui の `Entity`/`Context` モデルと噛み合わず、大規模書き換えの割に利得が薄い。intent パターンで「実質単方向」に留める。却下。

## 将来の負債 / リスク

- VM 構築のコスト(毎フレーム再構築は無駄)→ 派生データは reload 時に1回作り、フレーム間で保持。
- `ActiveModal` が各 modal の window-context 依存(`InputState`/focus)をどう持つか実装時に詰める(architecture.md §8)。
- intent 経由の間接化でデバッグが一段増える → intent enum を明示的に保つ。

## Consequences

- 各機能領域は「VM + view component + intents」で完結し、Codex への分割委譲がしやすくなる(1 領域 = 1 タスク)。
- headless テスト(KAGI_*)が不要になる土台(VM 直接テスト)が整う。
