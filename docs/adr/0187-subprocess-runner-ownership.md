# ADR-0187: subprocess は一つの runner が所有し、待機打切りを終了と混同しない

- 状態: 採用（#507）
- 日付: 2026-09-08
- 関連: [#507](https://github.com/TomiXRM/kagi/issues/507)、
  [ADR-0177](0177-transport-recording-boundary.md)（未証明の結果を確定扱いしない）、
  ADR-0009 §3（network git は system binary 経由）、ADR-0089（remote SSH）、
  ADR-0099（agentic CLI provider）、#294 / #403（run_git の leak と pipe deadlock）、
  [DESIGN §4.3/§5.1](../rearch/app-layer/DESIGN.md)

## 決定

subprocess を起動する経路は `kagi_git::cli::run_child` **一つ**に統合する。
runner が child と stdin/stdout/stderr と deadline を所有し、caller は
program/args/env/cwd だけを組み立てる。

```rust
pub fn run_child(cmd: &mut Command, timeout: Duration, stdin: Option<&[u8]>)
    -> std::io::Result<ProcRun>;

pub struct ProcRun { pub stdout: Vec<u8>, pub stderr: Vec<u8>, pub status: Result<i32, ProcStop> }
pub enum ProcStop { Deadline { secs: u64, reaped: bool }, Wait { error: String, reaped: bool } }
```

型が契約である。

| 値 | 意味 |
|---|---|
| `Err(io::Error)`（spawn 失敗） | **何も走っていない**。副作用なし。`Failed` にしてよい |
| `ProcRun.status = Ok(code)` | process が本当に終了した。exit code を読んでよい |
| `ProcRun.status = Err(ProcStop)` | **待機を打ち切った**だけ。結果は不明。`Failed` にしない |

`ProcStop` は exit code を持たない。したがって「timeout を非ゼロ終了として
読む」コードは**書けない**。これが #507 の実体で、`Option<ExitStatus>` を返す
旧 `wait_or_kill` や `recv_timeout` の `Err` を timeout 値へ潰す実装は、
どちらも呼び出し側の注意力に頼っていた。

- **pipe**: stdout/stderr は常に piped で専用 thread が drain する。`stdin` は
  `Some` のとき piped で第三の thread が書き込み、`None` のとき `null`。
  prompt も出力も pipe buffer（~64 KiB）を超えてよい。
- **deadline**: 期限切れで runner が kill + bounded reap を行い、`reaped` に
  結果を記録する。kill を確認できなかった場合だけ reader thread を join せず
  detach する（所有していない process に caller を無期限に張り付かせない）。
- **`reaped` が指すもの**: 直接の child のみ。子孫 process と、child が既に
  他所（remote host、ネットワーク）で始めた作業は含まない。

### 呼び出し側の写像

| 経路 | `ProcStop` の扱い |
|---|---|
| `cli::run_git` | `GitError::TerminationUnknown`（#582 経由で `Unknown`） |
| `ops/worktree_steps::do_command` | `GitError::TerminationUnknown`、`progress.termination_unknown` は立てたまま |
| `remote::run_ssh` | `RemoteError::TerminationUnknown` → `remote_pull` は `OpOutcome::Unknown`（ADR-0177 の既定） |
| `remote::stash::run_frozen` | `FrozenRunError::Unconfirmed`（従来どおり lease 保持） |
| `message_gen::cli_generate` | `GenError::Http`（read-only 生成なので lease なし。静かに rule-based へ落ちる） |

`RemoteError::Timeout` は `RemoteError::TerminationUnknown(String)` に置き換えた。
名前が「時間切れ」ではなく「停止不明」を指すようにするため、そして wait 自体の
失敗も同じ扱いにするため。

## 問題

`src/remote/mod.rs` は child を `wait_with_output` thread へ渡し、`recv_timeout`
だけで `Timeout` を返していた。呼び出し側は child を kill も reap もできず、
**打ち切った後に remote 操作が完了しうる**。`src/remote/stash.rs` の
`run_frozen` も同型。`message_gen::cli_generate` と `login_shell_path` は
`try_wait` で終了を待ってから stdout を読むため、pipe buffer を超える出力
（大きな回答、banner を出す `.zshrc`）で deadlock しうる。
`run_git`（#294/#403 で修正済み）だけが正しく、同じ規約が三つの runner に
再実装されていなかった。

## 代替案

- **process group / process tree kill**: `setsid` + `killpg` で子孫まで止める。
  OS 差（Unix のみ）と、SSH の場合は結局 remote 側の process を止められない
  ことから今回は入れない。`ProcStop::reaped` の doc と `ponytail:` コメントが
  この天井を明示する。DESIGN §4.3 が #507 に委ねた「process-tree 制限」は
  **記述して残す**という結論。
- **`Option<ExitStatus>` のまま `wait_or_kill` を共有する**: 出力の drain と
  stdin の供給が各 caller に残り、#403 の deadlock が再発する余地が残る。
- **timeout を非ゼロ exit に写像する**: ADR-0177 が禁じた「未証明の結果を確定
  扱いする」そのもの。

## スコープ外

- `gh` 経路（`github.rs` / `github_merge.rs` / `github_fetch.rs`）の
  `Command::output()`。`output()` は両 pipe を並行に読むので deadlock はしないが
  deadline がない。#507 受け入れ条件の「gh が worker を無期限に占有しない」は
  別 PR で `run_child` に載せ替える。
- `gh_available` の OnceLock probe を render caller から外すこと（UI 側の課題）。
- retry 機能（#507 明記のスコープ外）。
