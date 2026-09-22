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

## 描画用 marker 派生状態（#497）

- `FileResolution` の private Result 差替え境界で、その内容の marker 判定と
  `ResolutionBuffer` の集約件数を同時更新する。選択・手編集・hunk 操作・undo/redo
  は同じ境界を通り、変更していない file は再走査しない。別の数値 revision や
  repository の `ConflictRevision` を key にせず、Result と派生状態を一緒に所有する。
- load は判定を再構築し、live index への autosave overlay は該当する draft と
  検証済みの派生状態だけを運ぶ。派生値は JSON に保存しない。
- render は cached marker verdict を読み、dashboard の Continue と理由表示は
  同じ blocker 評価を共有する。未解決・binary・deletion の優先順位は変えない。
- Edit 中の同期は hunk の元データではなく現在の Result と比較する。
  `text_to_lines` と同じ末尾改行の扱いで比較し、同じ手編集を再描画ごとに
  undo stack へ積み直さない。空テキストと空行は区別し、Preview の組立表示は維持する。
- cache は表示専用。`files_with_marker_residue` と Save/Continue の実データに対する
  fresh validation は維持し、cache を書き込みの安全ゲートにしない。
