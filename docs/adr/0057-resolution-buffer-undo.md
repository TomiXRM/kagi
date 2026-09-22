# ADR-0057: Resolution Buffer と Undo(repo を汚さない解決)

- Status: Accepted(2026-06-13)

## Decision

- choose(current/incoming/both-ordered)・手編集は **WT/index に触れない解決バッファ**上の操作。
  ファイルごとに Result 草稿 + 操作履歴(undo/redo)。draft(ADR-0042)と同じ
  `~/.kagi/conflicts/<repo-hash>/` へ 250ms debounce 自動保存 → 中断・再開可能
- continue 時にのみ WT へ書き出し+stage。**abort してもバッファは oplog 参照付きで退避**
  (jj の「部分解決を失わせない」の git 互換実装)
- 行ごとの採用元(current/incoming/manual)を保持し UI で出所可視化(BC/KDiff3 流)

## JSON codec（#513 resolution slice）

- autosave の JSON 構文処理は `serde_json` に統一する。保存時は借用する
  `Serialize` view を使い、domain 型への serde 依存や草稿文字列の複製は増やさない。
- 既存の `repo` / `updated` / `files` と各 file の `path` / `binary` /
  `current` / `incoming` / `result` / `raw_result` を維持する。
  `result: null` は未解決、空配列は空テキストへの解決であり、混同しない。
  行の `t` / `o` と選択済み raw の `oid` / `mode` も維持する。
- 読み込みは呼び出し側の repo path をキーとし、欠落・不正な optional field は
  従来の既定値へ戻す。path のない file は読み飛ばし、不正な raw result で
  テキスト草稿を失わせない。JSON 自体が不正なら読み込みは `None` とする。
  surrogate pair は Unicode 文字として復元し、再保存しても脱落させない。
- live index 由来の raw metadata は再検出し、保存済みの選択だけを重ねる。
  undo/redo・hunk model は保存しない。保存先・debounce・書き込み方式、
  Continue/Abort の動作、oplog はこの移行では変更しない。
