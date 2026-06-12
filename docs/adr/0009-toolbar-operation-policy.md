# ADR-0009: Header Toolbar Git Operation Policy(GitOperationPlan and Safe Execution)

- Status: Accepted
- Date: 2026-06-12
- 関連: ADR-0004(破壊的操作ポリシー)を本 ADR で拡張(Amends ADR-0004)

## Context

Header Toolbar に Pull / Push / Branch Create / Stash / Pop / Undo Commit / Refresh を置く。
「単なる git command ボタン」は事故の元であり、本プロダクトの価値(事前明示)に反する。

## Decision

### 1. 全ボタンは既存パイプラインを通る

Toolbar のボタンは **既存の plan → confirm → preflight → execute → verify → oplog パイプライン
(architecture.md §5、T013〜実装済み)への入口にすぎない**。ボタン直結の execute を禁止する
(Refresh のみ読み取りなので例外)。plan を持たない新規操作を追加する場合、必ず ops.rs に
plan_* / execute_* のペアを実装してから UI に出す。

### 2. ADR-0004 の操作分類への追加

| 操作 | クラス | 根拠と扱い |
|------|--------|-----------|
| fetch | Safe | refs/remotes の更新のみ。plan 表示 + 実行 |
| pull (merge) | Guarded | dirty で blocker(checkout と同じ)。conflict 予測は in-memory merge で事前判定し、conflict 予測時は blocker(ADR-0005 踏襲) |
| push | Guarded | **force / force-with-lease を実装しない**(後者は later 検討)。non-fast-forward はエラー表示(自動 force 昇格しない) |
| stash pop | **Destructive(緩和付き)** | 成功時に stash が消える。plan に「apply との違い」を明示。**conflict が予測される場合は blocker とし、apply を提案**(stash を失わない側に倒す) |
| undo commit (soft) | Guarded | ADR-0011。HEAD を1つ戻すが**変更は index/WT に残る**(何も失われない)。push 済み commit は blocker |

### 3. ネットワーク操作(pull/push/fetch)の実装方針

ADR-0002 のとおり認証は CLI に逃がす: pull/push/fetch は **git CLI wrapper**
(`git -C <repo> pull --ff ...` 等を引数配列で組み立て、shell 経由にしない)で実装する。
credential helper / ssh agent がそのまま効く。出力は Operation Log に流す。
preflight / verify は従来どおり git2 の snapshot で行う。

### 4. disabled 規則

ボタンの disabled 状態と理由は常に UI に表示する(T026 の Commit ボタンと同じ「理由が見える」方式)。
detached HEAD では Pull / Push / Undo を disabled(ADR-0010)。

## Consequences

- CLI wrapper の導入で「git バイナリ存在」が pull/push の前提になる(起動時チェック + 無ければボタン disabled)
- pop の conflict 予測 blocker により「pop で stash が消えたのに conflict」という最悪ケースを構造的に排除
- Toolbar 追加操作はすべて ops_test.rs 系の integration test を必須とする
