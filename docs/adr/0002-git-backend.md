# ADR-0002: Git バックエンドは git2 (libgit2) を主、CLI wrapper を補助とする

- Status: Accepted
- Date: 2026-06-12

## Context

Git 操作の実装方式として git2 (libgit2 binding) と git CLI wrapper を比較する。

| 観点 | git2 / libgit2 | git CLI wrapper |
|------|----------------|-----------------|
| 読み取り性能 | ◎ in-process。revwalk で 10k commits を高速列挙。プロセス起動コストなし | △ 呼び出しごとに prosess spawn。大量呼び出しで遅い |
| 構造化データ取得 | ◎ Oid / Commit / Status を型で取得。パース不要 | △ porcelain v2 等のテキストパースが必要。バージョン差・locale 差のリスク |
| dry-run / preview | ◎ **in-memory merge / cherrypick_commit で working tree に触れず conflict 予測できる**(本プロジェクトの核) | ✗ 相当機能なし。実際に実行して戻すしかない |
| 挙動の Git 互換性 | △ 一部挙動差(merge 戦略の細部、config 解釈など) | ◎ 本物の git そのもの |
| 認証 (push/fetch) | △ credential callback の実装が面倒 | ◎ credential helper / ssh が自動で効く |
| 安全性 | ◎ プログラム的に操作を制限できる(危険 API を呼ばなければよい) | △ 引数組み立てミスで想定外コマンドが走るリスク |
| 依存 | C ライブラリ同梱(vendored でビルド自己完結) | ユーザー環境の git バージョンに依存 |

## Decision

- **読み取り(log, refs, status, diff)と MVP の書き込み(checkout, branch, stash, cherry-pick)は git2** で実装する。
- `GitBackend` trait で抽象化し、**v0.2 の fetch/pull/push は CLI wrapper 実装**を併用できる余地を残す(認証問題の現実解)。
- gitoxide (gix) は書き込み系 API がまだ揃い切っていないため今回は見送り。読み取り高速化が必要になったら再評価。

## Rationale(決め手)

cherry-pick / merge の **dry-run preview** が本プロダクトの中核価値であり、これを working tree を汚さずに実現できるのは libgit2 の in-memory 操作だけ。読み取りの構造化・性能も GUI 用途では CLI パースより圧倒的に堅い。

## Consequences / Risks

- libgit2 と git CLI の挙動差: 結合テストで fixture repo に対し `git` CLI の結果と突き合わせて検証する。
- 認証が必要な操作(v0.2)は CLI 実装側に逃がす設計を維持する。
- `git2` crate のバージョンは Cargo.lock で固定。`vendored` feature でビルドを自己完結させる。
