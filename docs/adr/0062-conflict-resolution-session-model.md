# ADR-0062: Conflict Resolution Session Model

- Status: Accepted(2026-06-13)
- 関連: ADR-0056(Mode state machine)/ 0057(resolution buffer)/ requirements-conflict-ux.md §3.1

## Decision

conflict 発生〜continue/abort/skip までを **1 つの Session** として扱う(W26 の ConflictSession を拡張):

```rust
struct ConflictResolutionSession {
    id: ConflictSessionId,           // sha1(repo + op + started_at)。oplog/resolution log の相関キー
    operation: ConflictOperationKind, // Merge / Rebase{step,total} / CherryPick / Revert
    current_branch: Option<String>,   // 役割: Current branch side
    incoming_ref: Option<String>,     // 役割: Incoming branch / Commit being applied
    started_at: DateTime,             // 注: gpui/test では外部から注入(Date::now 不可の制約)
    files: Vec<ConflictFile>,         // type(ADR-0065)+ status(unresolved/resolved-candidate)
    resolved_count: usize,
    unresolved_count: usize,
    can_continue: bool,               // ADR-0067 のチェック結果
    can_abort: bool,                  // 常に true(安全弁)
    can_skip: bool,                   // sequencer op のみ
}
```

- session は **検出時に再構築**(Repository::state + index conflicts)し、ResolutionBuffer(ADR-0057)を
  `~/.kagi/conflicts/<sha1(repo)>/` から復元 → **中断・再開してもユーザーの解決選択を失わない**
- id は oplog の各エントリ(save/continue/abort/skip/hunk action)に付与し、Resolution Log と
  Operation Log を相関できるようにする(§2.7)
- can_continue/abort/skip は session が毎回計算する派生値で、UI(banner/dashboard)はこれを読むだけ
  (UI から git 状態を直接判定しない)
