# ADR-0073: GitBackend Trait, Unified Operation, Worker Thread

- Status: Accepted / Date: 2026-06-14
- Context: v1.0 re-architecture. See `docs/rearch/architecture.md` §2.2, `docs/rearch/research/03-git-backend.md`.

## Decision

- `kagi-git` に **`trait GitBackend: Send`** を定義し、app から見える git の唯一の窓口にする。読み取り(`snapshot` / `diff_commit` / `diff_workdir`)とパイプライン基本操作を持つ。
- v0.2.0 の ~30 個の `plan_X` / `preflight_X` / `execute_X` フリー関数(`ops.rs` 6557 LOC)を、単一の **`enum Operation`** とそれに対する `plan` / `preflight` / `execute` / `verify` に統合する。op ごとのロジックは `ops/checkout.rs` 等のモジュールに分割。共通処理(dirty-WT formatter、標準 blocker セット)は一度だけ書く。
- adapter は2つ: **git2 adapter**(default、in-memory dry-run を行える唯一の実装)と **CLI adapter**(network: fetch/pull/push を `run_git` で、prompts-off + timeout)。
- 各 `RepoSession` は **専用 worker thread を1本**持ち、その上に `git2::Repository` を置いて操作を channel で直列化する(git2::Repository は `Send` だが `!Sync`)。

## なぜ

- **安全パイプラインの単一化**: plan→confirm→preflight→execute→verify→log を関数ごとのコピペではなく一箇所で強制する。`Operation` enum なら exhaustive match で「全 op がパイプラインを通る」ことを型で担保できる。
- **git2 を1クレートに封じる**(ADR-0072)ための具体機構。
- **worker thread**: v0.2.0 は背景クロージャごとに repo を開き直し(80×)、`cx.spawn` がアドホックに散在。1 session = 1 worker = 1 Repository にすると、開き直しコスト・競合・キャンセル管理が構造化される。
- **dry-run は git2 を残す理由そのもの**: `cherrypick_commit`/`merge_trees` の in-memory merge で working tree を触らず conflict を予測する。CLI では不可能なのでここだけは libgit2 必須。

## 代替案

1. `trait GitBackend` のメソッドを op ごとに生やす(`checkout()`, `cherry_pick()`, …)。
2. 本決定の単一 `enum Operation` + 汎用 `plan/execute`。
3. Zed 型の `GitRepository: Send+Sync` + async BoxFuture(CLI only)。

## 捨てた案

- 案1: メソッド爆発し、「全 op が plan を通る」不変条件を型で守れない。trait が肥大化。却下(ただし読み取り系は素直にメソッドで持つ)。
- 案3: Zed は CLI only で dry-run を持たない。Kagi の予測安全性(in-memory merge)を失うので不採用。設計の参考に留める(GPL の都合もありコードは流用不可)。
- gitbutler / jj のコード流用: ライセンス(FSL)・依存(gix/protobuf)が重い。**概念のみ採用**(snapshot atomicity、op-DAG/undo metadata)。

## 将来の負債 / リスク

- 6.5k LOC の挙動保存移行 — 「まず verbatim 移動、refactor は後」。306 テストを安全網にする。
- 単一 worker の直列化が network op(長い)で他操作をブロックしうる → network は CLI adapter で別扱い、必要なら fetch を別レーンに。
- per-keystroke の再 plan latency(v0.2.0 既知)→ plan とは別に純粋 validator の fast path を domain に置く。
- enum vs trait の最終確定は実装時(architecture.md §8 open question)。

## Consequences

- oplog はパイプライン末尾で**必ず**1回書かれる(v0.2.0 は呼び出し側任意だった)。形式は ADR-0074。
- UI は `OperationController::request(Operation)`(ADR-0075)しか触れない。
