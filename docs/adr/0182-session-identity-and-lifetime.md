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

## identity の再照合（凍結値と実解決値）

`RemovePlan` / `StashPlan` に、その plan が **実際に解決した** 管理 worktree の
`worktree: WorktreeId` を追加した。owner 判定は 3 点照合になる。

1. plan 採用時（`apply_plan`）— 凍結 `Attachment.worktree` と `plan.worktree` を比較。
2. 承認直前（`approve`）— locator を **その場で再解決** して凍結値と比較。
3. 実行時 — 既存の `RepoId` / admin fingerprint 照合（ADR-0175）。

不一致・identity 取得不能はどちらも `AdmissionError::Identity`（plan 側は `PlanState::Error`）で
拒否し、再 open を要求する。execute 時の `RepoId` 照合だけでは attach→plan の drift を防げない:
同一 repository 内で sibling worktree への symlink に差し替えると `RepoId` は一致したまま
HEAD/index が変わる。preview・stash index・OID がその時点で別 worktree のものになるため、
「表示する前に」拒否する必要がある。

## 配送範囲 — 共有 refs と worktree 限定変更

`Planned::changes_shared_refs()` が段階1 の配送契約。

| 変更 | 範囲 |
|---|---|
| remove-worktree（refs/admin） | 管理 worktree ＋対象 worktree ＋**同 RepoId の開いている全 sibling** |
| stash push / pop / drop（`refs/stash` 書換） | 対象 worktree ＋同 RepoId の開いている全 sibling |
| stash apply（index/WT のみ） | 対象 worktree のみ |

`Sessions::siblings_of(&RepoId)` が開いている worktree を返し、`apply()` が既配送分を除いて
`Delivery::Invalidate` を追加し `stale` に入れる。段階2 の read 移管ではなく段階1 の配送契約。

## 離脱 revision — 提案は保存せず再観測する

`TabSession.visit` を追加した。tab を**離れる**たびに increment する（incarnation は不変、
tab は開いたまま）。`Attachment` は plan 時の `visit` を運ぶ。

- 離脱で `stash_followups` は即破棄する。元の conflict は既に解決済みで、再証明する手段がない。
- 離脱後に着地した completion は payload を **作らない**（`s.visit(session) == attachment.visit` 判定）。
- conflict payload 自体は残るが **evidence にすぎず**、`visit` が古い間は
  `stash_conflict()` も `continue_stash_conflict()` も `take_stash_followup()` も何も返さない。
- 復帰時の再 detect が実 repository の conflict identity で `observe_stash_conflict` を呼び、
  identity が一致したときだけ `visit` を現在値へ再束縛する。つまり提案の OID は常に
  「今そこにある conflict」に由来し、保存された提案が復活することはない。
- 離れている間に conflict が解決されていれば identity は空になり、payload は破棄される。

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
- **alias 統合**: `Sessions::attach` は解決済み `WorktreeId` が既存 session と一致すれば
  その session を返す。`/repo` と `/repo/.git`、symlink、相対 path はすべて同じ tab になる
  （path 比較では見抜けない）。配送は `sessions_for()` で **一致する全 session** へ行い、
  `HashMap` の任意 1 件を選ばない。
- `switch_repo` と `enter_remote_view` は離れる tab に対して `Sessions::depart` を呼ぶ。
  #562 の同一 tab 再選択は early return するので depart しない。背景 close も呼ばない。
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
