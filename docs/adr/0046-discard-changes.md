# ADR-0046: Discard Changes(unstaged の変更を捨てる)

- Status: Accepted(2026-06-13、ユーザー依頼「雑に追加したけど変更をなかったことにする機能」/ 設計は PM 判断)
- Date: 2026-06-13

## Context

unstaged な変更(working tree の編集)を「なかったことにする」操作が欲しい。Commit Panel の
unstaged files に per-file の Discard ボタンと、Discard all changes ボタンを置く。

これは kagi で初めての **working tree の内容を恒久的に失わせうる操作**。git 的には
`git checkout -- <path>`(index の内容で WT を上書き)に相当する。reflog にも oplog の ref 記録にも
残らない = 通常の git では**復元不能**。kagi は「安全な Git GUI」が identity なので、
確認 UI だけに頼らず**捨てる前に必ずバックアップを取る**。

## Decision

### 意味論

- **Discard(file)** = 対象 path の WT 内容を **index の内容**で上書きする(`git checkout -- <path>` 同等)。
  - staged 変更には**一切触れない**(index は不変。staged を消す操作ではない)
  - WT で削除されたファイル(unstaged deletion)も index から復元される(これは安全方向)
- **Discard all changes** = unstaged セクションの対象ファイル全部に同じ操作
- **untracked file は対象外(MVP)**: untracked の「discard」= ファイル削除 = `git clean` 相当であり、
  codebase 規約(`git clean` 実装禁止)に抵触する。ボタンは disabled + tooltip
  「Untracked files are not deleted by kagi」。将来やるなら別 ADR(trash 移動方式等)
- **conflicted file は blocker**(conflict 解決フローで扱う。discard で踏み潰さない)

### 安全機構: backup-then-discard(必須)

1. **plan**: 対象ファイル列挙 + 各ファイルの WT 内容サイズ確認。blockers/warnings を返す
2. **backup**: 実行直前に、対象各ファイルの**現在の WT 内容を `repo.blob()` で ODB に書き込み**、
   `path → blob SHA` のリストを作る(ODB の loose object として残る。kagi は gc を実行しないので
   oplog が参照する限り実質回収されない)
3. **execute**: `checkout_index`(path 指定・force)で WT を index 内容に戻す
4. **verify**: 対象 path が status の unstaged から消えたこと
5. **oplog**: `discard` op として **path→blob SHA の対応を必ず記録**(復元手段。
   `git cat-file -p <sha>` でいつでも取り出せる。将来 oplog UI から restore 可)

backup が 1 ファイルでも失敗したら **discard 自体を中止**(blob 化できないものを捨てない)。

### UI フロー

- per-file: unstaged 行の hover に Discard アイコン(trash / undo 系)。click → **danger 確認 modal**
  (赤系、ADR-0023 の destructive 表示): 対象 path、「This permanently discards your unstaged changes
  to this file. A backup blob is recorded in the oplog.」+ Cancel / Discard
- Discard all: unstaged セクションヘッダにボタン。modal に**対象ファイル一覧(件数 + list)**を表示。
  untracked / conflicted は一覧に「skipped」と明示
- 実行は W15 の async パターン(`start_discard` + blocking core、busy_op="discard"、
  Busy footer + toast)。tree サイズ依存の WT 書き込みのため
- `KAGI_AUTO_CONFIRM` はテスト専用(既存規約)

### headless 検証

- `KAGI_DISCARD=<path>` / `KAGI_DISCARD_ALL=1`(+ `KAGI_AUTO_CONFIRM`)で plan→execute→verify を
  ログ出力(既存 KAGI_* 経路と同形式)

## Consequences

- 「破壊的操作はしない」規約は「**WT discard は backup-then-discard の隔離フローでのみ許可**」へ追記
  (ADR-0024 hard-reset 隔離・ADR-0040 案 C と同型の例外管理。ADR-0023 の表にも destructive として追記)
- ODB に discard backup blob が溜まるが、サイズは捨てた WT 内容ぶんのみで、ユーザーデータ保全の
  対価として妥当。将来「oplog から restore」UI の土台になる
- untracked 削除は引き続き不可能(git clean 非実装の規約を保つ)
