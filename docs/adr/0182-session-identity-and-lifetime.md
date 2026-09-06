# ADR-0182: session identity と寿命を既存 Sessions へ集約（#482 段階1）

状態: 採用・実装済み（#482 段階1）
日付: 2026-09-07
関連: [#482](https://github.com/TomiXRM/kagi/issues/482)、
[DESIGN §2.1/§2.2](../rearch/app-layer/DESIGN.md)、
[ADR-0175](0175-app-remove-boundary.md)、[ADR-0176](0176-app-stash-local-boundary.md)、
[ADR-0095](0095-active-view-single-source.md)、[ADR-0121](0121-zed-informed-modularization.md)

## 決定

application family の owner を **path でも tab index でもなく session** で表す。
新 registry / actor / Workspace / crate / worker は作らず、既存 `src/app::Sessions` を拡張する。

- `TabId(u64)` は表示 slot、`SessionId { tab, incarnation }` はその slot が repository に
  attach された世代。`incarnation` は process 全体で単調な `next_id()` から発行するので
  `SessionId` の等値がそのまま owner 判定になる。
- `Sessions::attach(path) -> SessionId` が slot を開き、`kagi_git::Backend::write_worktree_id()`
  で解決した `WorktreeId` を **その時点で凍結**する。`reattach` は同じ `TabId` で incarnation
  だけを更新する（remote 再 snapshot）。`detach(session)` が slot を閉じる。
- `Attachment` は `{ session, path, worktree }`。`path` は plan 入力の locator に限り、
  同一性判定には使わない。`generation: u64` は削除した。
- `Sessions` の path-keyed map を廃止した。`stale: HashSet<WorktreeId>`、
  `stash_conflicts` / `stash_followups: HashMap<SessionId, StashConflict>`。
- `Delivery::Invalidate` / `RemovedTarget` は `InvalidTarget { worktree, path }` を運び、
  宛先 tab は `Sessions::session_for(&WorktreeId)` で引く。`path` は段階2 まで残る
  path-keyed `tab_cache` の locator としてのみ使う。
- 単一 plan slot に `plan_owner: Option<SessionId>` を持たせ、owner の `detach` で失効させる。
  `approve()` も owner が attach 済みでなければ `StaleApproval` を返す。

## `RepoId` / `WorktreeId`

`Backend::write_worktree_id()` を追加した。`write_repo_id()`（canonical common Git dir）＋
canonical な per-worktree Git dir。main worktree と linked worktree は `repo` を共有し
`git_dir` で分かれる。`RemovePlan.worktree_id` が凍結する削除対象の identity は
その worktree を開いた tab が attach 時に解決する値と一致する（`app_remove_test` で固定）。

RepoId の導入は **独立 repo の並行 write 解禁ではない**。保守的な global busy は維持する。

## close は実行取消ではない

`detach` が捨てるのは表示に属するものだけ — conflict / follow-up payload と、
その session が所有していた plan slot。`operations` / `leases` / `settled` / `reconcile`
には触れない。停止不明の lease は tab を閉じても保持され、window close / Quit は
引き続き保留される。completion は必ず `app::apply` を通り、closed owner には
表示 guard（footer / reload）だけが落ちて receipt・oplog・終端化は落ちない。
重複 `OperationId` は `settled` で no-op のまま。

## UI 側の帰結

- `RepoTab` が `session: SessionId` を持つ。`KagiApp::active_session()` が唯一の owner 判定で、
  `self.repo_path == Some(owner.path) && self.switch_generation == owner.generation` は
  app family から消えた。close→同 path reopen は別 session なので旧結果が届かない。
- `EditorPendingIntent::CloseRepoTab` と remove の `RemovedTarget` は session / `WorktreeId`
  経由で対象 tab を閉じる。`close_tab_by_path` は削除した。
- `close_tab` の `clear_stash_conflict(&path)` は `detach(session)` に置き換えた。
- `switch_generation` は read load の世代 guard として残す（#489 / 段階2 の RequestSlot で置換）。
- `#488`/`#562` の同一 tab 再選択 no-op と背景 close `Keep` は不変。背景 close は
  `RepoTab` を消すだけで active tab の `SessionId` を変えないので、選択・undo 履歴・
  進行中 operation の宛先はそのまま残る。

## 対象外（段階2 / 段階3）

snapshot・`active_view` / `tab_cache`・read revision の単一 owner 化（段階2、#489）、
selection/scroll/pane/menu などの `TabView` 移管と `reset_per_repo_ui` の撤去（段階3）。
本 ADR は二重書込を追加せず、段階1 では snapshot 側に一切触れていない。

これは [ADR-0175](0175-app-remove-boundary.md) の
「No worker, TabId, session incarnation, global registry」のうち
**TabId と session incarnation の部分だけ**を supersede する。worker と global registry は
引き続き対象外。

## 検証

- G: `cargo test --workspace`。`app_remove_test` に main/linked の `RepoId` 共有・
  `WorktreeId` 分離、plan の凍結 target identity 一致、close→同 path reopen への旧結果不適用、
  重複 completion no-op、detach 後も停止不明 lease 保持、detach による plan slot 失効を追加。
  `app_writer_admission_test` に detach しても reservation を手放さないことを実 writer 5 種で追加。
  `app_stash_test` は conflict/follow-up が session 所有であること（detach で消え、
  reopen が継承しない）へ更新。
- E: `remove_public_boundary`、`stash_conflict_close_reopen`、`stash_conflict_followup` を
  session 判定へ拡張。GUI runner の実行は PM / primary session が行う。
- `uv run --project ci check-all`（`app-layering` gate: `src/app` は `Backend` 経由のみ）。
