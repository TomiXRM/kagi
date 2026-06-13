# ADR-0077: Layered Test Strategy, Fixture Crate, and CI

- Status: Accepted / Date: 2026-06-14
- Context: v1.0 re-architecture. See `docs/rearch/architecture.md` §5, research #10.

## Decision

クレート分割に対応した4層テストピラミッドを採る:

1. **kagi-domain** — plain `#[test]`(graph layout / diff / diffstat / conflict FSM + assemble / template / checklist / message-gen / settings)。テストの大半をここに集約。fixture も window も不要。
2. **kagi-git** — tempdir fixture repo に対する結合テスト。**`kagi-test-fixtures` クレート**を新設し、29 suite に散らばっていたコピペ `git()` builder と `scripts/make_fixture.sh` のロジックを Rust に統合。既存 ~306 テストはここへ移し backend API に再接続。
3. **kagi-app** — view-model + `OperationController` テスト。VM は plain data なので大半は plain unit test、async/controller フローのみ `gpui::test` / `TestAppContext`。
4. **kagi-ui** — window/描画/入力が本当に要る箇所のみ `gpui::test` + `VisualTestContext`。

加えて:
- **`KAGI_*` headless harness を退役**させる。app/view 層がテスト不能だった代償物。VM と controller が直接テストできるので、`main.rs` 内の 47 var 分岐(plan/execute の重複経路)を削除。必要なら薄い `xtask e2e`(JSON 出力 CLI)に縮小。
- **`ci.yml` を新設**(fmt + clippy + `cargo test --workspace`、macOS + Linux、network pull/push は除外)。v0.2.0 には**テスト CI が存在しない**(release.yml のみ)。

## なぜ

- v0.2.0 のテストは git ロジックに偏り、UI/app は 47-var の脆い env harness(stderr grep 観測、prod binary 常駐、window 必須)でしか検証できない。クレート分割と VM 化で**大半のロジックが window 無しで unit test 可能**になるので、それを前提にピラミッドを組み替える。
- fixture のコピペは保守性を下げる → 専用クレートに一本化。
- テスト CI の不在は re-architecture 中の回帰検出を不能にする → 最優先で導入。

## 代替案

1. KAGI_* harness を維持・拡張。
2. 本決定の4層ピラミッド + harness 退役 + CI 新設。
3. UI も全部 VisualTestContext で E2E。

## 捨てた案

- 案1: prod binary にテストコードが残り、env 結合で脆い。VM 化で不要になる。退役が正。
- 案3: VisualTestContext は遅く Linux headless で不安定なことがある。本当に window が要る所だけに限定し、ロジックは下層で検証。却下。

## 将来の負債 / リスク

- **前提リスク**: VM を gpui Entity から実際に分離できるか(ピラミッド全体の前提)。conflict/commit 周りで `InputState` 依存が残ると app 層テストが window を要求しうる → VM source-of-truth を plain data に保つ(ADR-0076)。
- gpui 0.2.2 の `gpui::test`/`VisualTestContext` の正確な API 面は未検証 → 実装初期に 1-shot PoC で確認してから ui テストを依存させる。
- Linux CI での headless 描画安定性 → ui テストは最小限に。

## Consequences

- `cargo test --workspace` が green であることが完了条件の中核(goal)。CI がそれを門番する。
- `make_fixture.sh` は `kagi-test-fixtures` に吸収され、手動 E2E 用途のみ残す。
