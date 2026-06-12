# ADR-0040: Amend Semantics

- Status: **Accepted**(2026-06-13、ユーザー決定: 通常モード=案B、Advanced=案C を将来実装候補として採用)
- Date: 2026-06-13(改訂)

## Context

直前の commit を作り直す Amend を入れる。3 モード: **message only / staged を取り込む / 両方**。
amend は SHA を変える(= history-rewriting、ADR-0023)。pushed commit の amend をどう扱うか、
git2 でどう実装するか(in-memory 主義・ref-order 規則に整合)を決める。

完全禁止は「個人 feature branch で push 後に typo 修正・commit 整理をしたい」UX を損なう。
一方、amend だけ許すと **GUI から push できず詰む**(kagi は force push を出さない)。
→ pushed amend を許すなら **force-with-lease push までを一連の Advanced operation** として扱う。

## Decision

### 3 モードと意味

| モード | 入力 | 結果 |
|--------|------|------|
| message only | 新 message | tree は HEAD と同一、message だけ差し替えた新 commit。SHA は変わる |
| staged | staged 変更 | 直前 commit の tree に staged を畳み込んだ新 commit(message は据え置き) |
| both | 新 message + staged | 上記両方 |

- いずれも **新しい commit を作り、branch ref を新 commit に付け替える**(古い commit は到達不能になるが
  reflog/oplog から復元可能)。**SHA が変わることを plan に明示**(`旧 <short> → 新 <short>` を予測表示)。

### git2 実装方式(in-memory + ref-order 規則)

- `commit.amend(...)` は使わず、**明示的に new commit + ref 移動**で行う:
  1. 親 = 旧 HEAD commit の **親**(amend は HEAD を置換するので parent は据え置き)
  2. tree: message only なら旧 HEAD の tree をそのまま。staged を含むなら `index.write_tree_to(repo)`
     で in-memory tree を得る(WT には触れない)
  3. `repo.commit(None, ...)` で **ref を更新せず** commit object を作る
  4. blocker 無し確認後に `repo.reference(...)` で **ref を最後に動かす**(ref-order 規則)
- author は旧 commit の author を**保持**(committer は現在のユーザー/時刻)。git の amend 既定に一致。
- **merge commit の amend は blocker**。detached/unborn の扱いは plan に明示。
- checklist(ADR-0039 / 0043)を通常 commit と同様に通す。
- 実行前に **旧 HEAD SHA を oplog に必ず記録**(before/after HEAD)。

### pushed commit の amend — 3 案と採否(ユーザー決定済み)

- **案 A(強警告で amend のみ許可、push は CLI 任せ)**: **採用しない**。
  amend 後に GUI から push できない「詰み」状態を作るため。
- **案 B(pushed amend は blocker)**: **通常モードで採用(MVP)**。
  「push 済みは履歴改変になるため amend 不可。新しい commit で修正してください」。
- **案 C(Advanced force-with-lease flow)**: **将来実装候補として採用**。
  Advanced mode で **amend + `git push --force-with-lease` までを安全確認付きの一連の操作**として提供する。

### 案 C: Advanced force-with-lease flow の設計

**ハードルール(禁止事項)**:
- `git push --force` は実装しない。**使うのは `--force-with-lease` のみ**
- **protected branch では禁止**: `main` / `master` / `develop` / `release` 系(`release/*`, `release-*` 等)
- **remote branch が最後に fetch した状態から進んでいる場合は禁止**
- force-with-lease 実行前に **fetch または remote state 確認を必ず行う**
- 確認なしの pushed amend 禁止 / amend だけ許して push 手段を用意しない実装は禁止

**UI 確認フロー(2回クリックではなく、確認内容を明確にした段階的確認)**:

1. **Amend 確認**
   - この commit は upstream に push 済み
   - amend すると SHA が変わる
   - remote branch と履歴が分岐する
2. **Force-with-lease 確認**
   - 通常 push では反映できない
   - kagi は `--force` ではなく `--force-with-lease` のみ使う
   - remote が他人によって進んでいた場合は失敗する(= lease の意味)
3. **入力確認**
   - 対象 remote branch 名を表示
   - **before remote HEAD / before local HEAD / after local HEAD** を表示
   - 続行には **branch 名または `force-with-lease` の明示入力**を要求する

**ユーザーが「通常の push ではなく履歴書き換えを行う」ことを理解できる UI** にする
(Advanced/Dangerous の赤系表示、ADR-0023 の history-rewriting カテゴリ)。

### 実装優先度

| フェーズ | 内容 |
|----------|------|
| **MVP** | pushed amend は blocker(案B)。**未 push commit の amend のみ実装** |
| **v0.2** | 案 C(Advanced force-with-lease flow)の詳細 ADR 追補と UI 設計 |
| **v0.3 以降** | 条件付き(protected 除外・lease 検査・段階的確認)で pushed amend + force-with-lease を実装 |

## Consequences

- 「force push 禁止」の全体規約は「**`--force-with-lease` のみ、案 C の隔離されたフロー内に限り許可**」へ
  将来改訂される(ADR-0024 の hard-reset 隔離例外と同型の扱い。実装時に ADR-0023 の表にも追記)
- `commit.amend` を避け new commit + ref 移動にすることで、cherry-pick / revert と同じ規則に乗る
- T-COMMIT-010/011(amend 実装)は **unblocked**: MVP スコープ = 未 push のみ、pushed は blocker
- 案 C の実装チケットは v0.2 設計時に起票(T-COMMIT-019 として予約)
