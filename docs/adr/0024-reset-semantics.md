# ADR-0024: Reset Semantics and Safety

- Status: Accepted(実装は別途承認後)/ Date: 2026-06-12

## Decision

- **MVP では reset を実行可能にしない**。Context Menu の Advanced/Dangerous に
  `Reset current branch to this commit…` を **Disabled(理由: "安全設計 ADR-0024 の
  実装承認待ち")** で表示するのみ(T-CM-051)
- 実装する場合の意味論(soft → mixed → hard の順に段階導入):
  - **soft**: ref 移動のみ(`repo.reference(refs/heads/<branch>, target)`)。
    WT / index 無傷。既存 undo_commit の一般化。destructive=true(履歴書き換えのため)
  - **mixed**: ref 移動 + index を target に reset。WT 無傷。destructive=true
  - **hard**: WT も書き換える = **データ喪失があり得る唯一の reset**。
    実装条件: (1) 2段階確認(ADR-0023)、(2) 失われる変更の**ファイル一覧を plan の
    preview_files に列挙**、(3) dirty なら「先に stash する」ボタンを第一候補として提示、
    (4) `git2 checkout_tree(force)` は**この文脈でのみ**使用可(コードベース禁止規約の
    唯一の例外として、関数名 `execute_hard_reset` 内に隔離 + テストは fixture のみ)
- 共通ガード:
  - reset 前に現在 HEAD SHA を oplog に記録し、recovery に `git reset --soft <旧SHA>` 相当
    (kagi 上では「Undo reset」導線)を明記
  - **push 済み commit を捨てる場合は警告**: `graph_descendant_of(upstream, HEAD)` で
    判定し「remote に存在する履歴です。force push は提案しません」を warnings に
  - detached HEAD では disabled(current branch が存在しない)
- デフォルト選択は **soft**

## Consequences

- hard reset の例外規約により「force/reset --hard/clean をコードに書かない」既存規約を
  「`execute_hard_reset` に隔離された checkout_tree(force) のみ例外」に改訂(実装時)
- T-CM-052/053(soft/mixed)は本 ADR の承認後に着手、T-CM-054(hard)はさらに後段
