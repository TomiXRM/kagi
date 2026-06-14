# ADR-0075: AppState / RepoSession / OperationController

- Status: Accepted / Date: 2026-06-14
- Context: v1.0 re-architecture. See `docs/rearch/architecture.md` §2.3, research #1/#3/#7.

## Decision

- **`AppState`**(単一 `gpui::Entity`)が `Workspace`(`Vec<RepoSession>` + active index)とグローバルサービス(settings / theme / i18n locale / command palette)を持つ。
- **`RepoSession`** はタブ1枚 = 完全に自己完結した単位。保持: GitBackend handle + worker、最新 `RepoSnapshot` + 派生 view データ、**`Selection { Wip, Commit(CommitId) }`**、**`RepoMode { Normal, Conflict(ConflictSession) }`**、`DiffService` cache、terminal session、FS watcher、`Freshness { Loading, Fresh, Stale }`。
- タブ切替は **`active = idx` のゼロフレーム swap**。v0.2.0 の「active タブ用トップレベルフィールド vs `tab_cache` の二重定義 + build_tab_view/apply_tab_view 橋渡し」を**廃止**。selection/scroll 等の transient state もタブごとに保持される。
- **`OperationController`** がパイプライン(plan→confirm→preflight→execute→verify→log)を中央で1回だけ強制する。`request(Operation)` → off-thread で plan → confirm 用に plan を返す → confirm 後 preflight(再 snapshot、repo が変わっていたら中断)→ execute → verify → oplog → snapshot 更新。キャンセルは構造化タスクハンドルで管理(v0.2.0 の `busy_op` + 各種 generation counter を置換)。**repo を変更する唯一の経路。**

## なぜ

- **状態の二重管理が最大の州**(research #7): フィールド追加が active 側と cache 側の 4-5 箇所に波及。1 session = 全状態自己完結にすれば、切替は index 変更だけになり、transient state も失われない。
- **selection の脆さ**(research #2): `selected: Option<usize>` は reload で壊れる。`CommitId` ベースにして session が所有する。
- **パイプライン強制の単一化**: UI から散在的に呼ばれていた git 操作を 1 つの controller に集約し、安全パイプラインを構造的に保証する。
- **キャンセルの構造化**: アドホックな generation 比較を、session 寿命(RAII)+ タスクハンドルに置換。

## 代替案

1. 単一 `Entity<AppState>` が全 session を所有(本決定)。
2. タブごとに `Entity<RepoView>` を分け、AppState は薄い親。
3. OpenLogi 風の単一グローバル AppState。

## 捨てた案

- 案3(全部グローバル): 重い per-repo 状態を gpui global に置くと purview が曖昧になりテスト困難。theme/i18n/prefs のような真にグローバルなものだけ global にする。却下。
- 案2 は有力だが、初期は単一 `Entity<AppState>` + 内部 `RepoSession` のほうが状態遷移を追いやすい。**実装時に再評価**(architecture.md §8 open question)。今は案1で確定。

## 将来の負債 / リスク

- 単一 Entity に状態が集まると `AppState` が再び肥大化しうる → `RepoSession` と各 service に責務を押し込み、`AppState` 自体は薄く保つ規律で対処。
- 非 active タブのデータ保持メモリ(多数タブ時)→ 必要なら LRU で snapshot を落として再ロード。
- watcher を per-active にするか per-session にするか未確定(research #7)。MVP は per-active + 切替時 revalidate。

## Consequences

- `tab_cache` / `build_tab_view` / `apply_tab_view` / 各種 generation counter は廃止。
- UI は `controller.request(op)` と session の派生データ読み取りだけを行う。
