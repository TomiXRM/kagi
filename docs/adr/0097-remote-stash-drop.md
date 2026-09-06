# ADR-0097: Remote stash drop over SSH (ADR-0089 Phase 3 — first remote write)

- Status: Accepted
- Date: 2026-06-17
- Context: ADR-0089 added a **read-only** remote-over-SSH view (snapshot, graph,
  diffs) and explicitly deferred remote *writes* to "Phase 3", noting they must
  keep Kagi's safety thesis (plan → confirm → execute → verify/oplog) and the
  destructive-command ban. A user working against a remote dev box (Tailscale,
  Linux) hit the gap: the stash context menu's **Drop** did nothing in the remote
  view, because every write path early-returns when `repo_path` is `None`.

## Decision（2026-09-07、#484 Stash PR 2 で改訂）

remote stash drop も application 層の
`plan → approve → prepare → run → apply` と単一 lease table を通す。旧 UI 直結の
`remote_stash_drop`、UI 側 oplog 組立、synthetic `<host>:<root>` identity は安全境界として
使用しない。詳細な採用契約は [FAMILY-stash r5](../rearch/app-layer/FAMILY-stash.md#5-remotessh-drop)。

### Layering
1. **Pure contract** (`kagi-domain::remote`): `ssh -G` の security-relevant 値、
   `RemoteConnectionId` / `RemoteRepoId`、versioned fixed NUL frame、completion token、
   outcome matrix を所有する。lease key は凍結 connection と remote physical common-dir の組で、
   selected main/linked root は attachment に分離する。
2. **I/O** (`src/remote/stash.rs`): 明示 identity file を要求し、`ssh -G` を再照合して
   direct profile だけを literal `-F /dev/null` argv に凍結する。known_hosts は digest と
   operation-owned snapshot を使う。一個の foreground `sh -c` が preflight/drop/post-read、
   repo 外 completion token の atomic rename、fixed terminal frame を順に行う。
3. **Application** (`src/app/stash.rs`): `StashJob::Local | Remote` の有限和だけを調停し、
   `std::process` / `std::fs` / shell を持たない。remote も approve 後に
   `WriteScope::Remote(RemoteRepoId)` を予約し、receipt は transport report から一度だけ記録する。
4. **UI** (`src/ui/operations/stash.rs`): remote modal も async plan と共通 confirm/dispatch を通す。
   UI は predicted outcome を記録せず、active attachment だけ footer/refresh を更新する。

### Safety
- Stash drop is **not** a banned destructive command (`push --force` / `reset
  --hard` / `git clean`); it is the same explicitly-confirmed Destructive op as
  the local path (ADR-0087). The confirmation modal, irreversible warning, and
  oplog entry are all preserved for the remote case.
- `BatchMode=yes` / strict host-key / no Proxy・ControlMaster・agent-only により、承認後に
  alias の実経路が変わる profile は write を拒否する。
- terminal frame + process exit が無い channel loss は Unknown。before と同じ state read だけでは
  old writer の停止を証明しない。同じ operation/job/scope/result に束縛した repo 外 completion
  token と state read の組だけが ack を発行し exact lease を解放する。token 不在・不正・読取不能
  では当該 app session 中は解除不能で、自動再送しない。
- remote repo/worktree/common-dir に Kagi marker、lock、oplog、daemon を置かない。

## Consequences
- The remote view is no longer strictly read-only: stash drop is a supported
  write. Other remote writes (pop/apply/checkout/commit/push) remain unimplemented
  and should follow this same pattern when added.
- 実 SSH の停止・token 回復は localhost sshd M で確認する。typed fake G は alias drift、
  fixed frame 全 outcome、slow writer 生存中の ack 不発を決定的に証明する。

## Not done (follow-ups)
- The remote stash menu still shows **Pop / Apply** (working-tree ops that no-op
  on a read-only remote). They should be hidden or surfaced as unavailable in the
  remote view.
- A formal `GitBackend` trait (`LocalBackend` / `RemoteSshBackend`) to replace the
  `remote_view.is_some()` dispatch (ADR-0089 deferred this).
