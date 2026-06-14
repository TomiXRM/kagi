# ADR-0072: Workspace Crate Split and git2 Confinement

- Status: Accepted / Date: 2026-06-14
- Context: v1.0 re-architecture (`re-architecture` branch). See `docs/rearch/architecture.md` §1.

## Decision

単一 `kagi` バイナリを Cargo workspace に分割する:

- `kagi-domain` — pure Rust。gpui にも git2 にも依存しない(models / graph layout / diff model / conflict FSM / plan types / rules / settings model / i18n keys)。
- `kagi-git` — **git2 が存在する唯一のクレート**。`GitBackend` trait + git2 adapter + CLI adapter + snapshot + oplog + worker thread。
- `kagi-app` — `AppState` / `RepoSession` / `OperationController` / async / persistence。gpui には依存するが git2 には依存しない(git2 は `kagi-git` 経由のみ)。
- `kagi-ui` — view-models / views / components / theme / i18n / commands。**git2 にも `kagi-git` にも依存しない。**
- `kagi`(bin) — window/shell/menu/main のみ。
- `kagi-test-fixtures`(dev)— fixture repo 生成。`xtask` — packaging(現状維持)。

依存は下方向のみ(domain ← git / app ← ui ← bin)。

## なぜ

不変条件「UI は `git2::Repository` を直接開かない / `git2::` を直接呼ばない」を**コンパイルエラーとして強制**するため。v0.2.0 では `ui/mod.rs` がこれを ~80 箇所で破っており、安全パイプライン(plan→…→verify)が構造的に保証されていない。`kagi-ui` の `Cargo.toml` から git2/kagi-git を外せば、UI から git2 を呼ぶコードは「そもそもビルドできない」。UI が必要とする `Operation` 要求型は `kagi-domain` に置くので、「何をしたいか」を記述するのに「実際にやる層」へ依存せずに済む。

## 代替案

1. **1 クレート内のモジュール境界 + lint** — 現状維持に近い。`pub(crate)` や命名規約で分離。
2. **2 クレート(core / ui)** — git ロジックと UI だけ分ける中間案。
3. **本決定の 4+2 クレート分割。**

## 捨てた案

- 案1: モジュール境界はリークをコンパイルエラーにできない(同一クレート内なら `git2::` をいつでも import できる)。grep gate 頼みは脆い。却下。
- 案2: domain(純粋・テスト容易)を git backend から分離できず、テストピラミッドの土台(ADR-0077)が作れない。app と ui も同居しがちで god-object が再発する。却下。

## 将来の負債 / リスク

- クレート分割は初期コストが高い(skeleton 化 + 306 テストの re-point)。strangler 移行(architecture.md §7)で段階的に行う。
- crate 間の型移動でビルドが一時的に壊れる窓が出る。各ステップで `cargo test --workspace` green を維持する規律で対処。
- `kagi-app` が将来 git2 を直接使いたくなる誘惑 → worker thread を `kagi-git` 側に置き app は handle のみ持つ設計で回避(ADR-0073)。

## Consequences

- CI に grep gate を追加(`grep -r 'git2::' crates/kagi-ui/src` が 0 件であること)を belt-and-suspenders として置く。
- 新規コードは必ず正しいクレートに帰属させる。迷ったら domain(純粋)に寄せる。
