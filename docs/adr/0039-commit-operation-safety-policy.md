# ADR-0039: Commit Operation Safety Policy

- Status: Accepted / Date: 2026-06-13

## Context

Commit スイート(Preview / Checklist / Amend / Smart Message 等)を入れるにあたり、
「何を block(実行不可)し、何を warn(警告つきで続行可)するか」の判定基準を一本化する。
既存 `plan_commit`(staging.rs)は既に一部を block/warn しており、これとの一貫性が要る。
ADR-0023(Dangerous Operations Policy)との責務分担も明確にする。

## Decision

### block と warn の判定基準(commit 系操作の共通則)

- **block(blockers に積む = Execute ボタンを出さない)**: 操作が **データ破損・履歴汚染・確実な事故**を
  招く条件。ユーザーが「分かっていてやる」余地がない、または override させるべきでないもの。
  - 例: staged が空 / message 空 / unresolved conflict file が staged / conflict marker が staged BLOB に存在
- **warn(warnings に積む = 黄色表示で続行可)**: 「意図しているかもしれないが確認したい」条件。
  false positive があり得るもの、ユーザーの正当なユースケースが存在するもの。
  - 例: secret・.env らしきファイル / large binary / leftover(commit に含まれない変更)
- 判定は **純関数**(repo 状態 + staged 情報 → blockers/warnings の Vec)。UI も oplog も持たない。
  ルールの具体は ADR-0043(Checklist Rules)に置き、本 ADR は分類方針のみ固定する。

### override(警告の無視)の可否

- **warn は override 可能**(続行できる)。block は override 不可。
- secret / large binary の warn は誤検知があり得るため、UI で「この警告を承知で commit」を 1 クリックで
  許可する(タイプ入力は求めない)。override したことは oplog の note に残す(誤コミット後の追跡用)。
- override の状態は **その commit 1 回限り**(次の commit に持ち越さない)。

### ADR-0023 との関係

- **commit / amend with new staged = history-additive**(新 commit を足すだけ)。ただし **amend は SHA を
  変えるため history-rewriting** に該当(ADR-0040)。本 ADR の checklist は「その commit を作る前の入力検査」、
  ADR-0023 は「その操作カテゴリの確認段数」を決める。**両者は直交**:
  - 通常 commit: checklist(本 ADR / 0043)を通る → 1段階確認(history-additive)
  - amend: checklist を通る → **2段階確認**(history-rewriting、ADR-0040 / 0023)
- checklist の blocker は ADR-0023 の「確認段数」より前段(plan 生成時)に効く。blocker があれば確認画面に
  進む前に Execute 不可。

## Consequences

- 既存 `plan_commit` の blocker(staged 空 / message 空 / conflict 状態)は本方針に既に整合 → 再分類不要
- 新ルール(conflict marker / secret / large binary)は本方針に従い前2つを block・後1つを warn(ADR-0043)
- checklist を純関数 + 別 module(`checklist.rs`)に切り出すことで、commit と amend の両 plan から再利用できる
