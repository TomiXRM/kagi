# ADR-0042: Commit Message Draft Storage

- Status: Accepted / Date: 2026-06-13

## Context

書きかけの commit message を、branch ごとに保存して再起動後も復元し、commit 成功時に clear したい。
保存先・形式・タイミング(debounce)を、当初は oplog / avatar キャッシュの流儀に合わせた。
2026-09-23、#513 の draft 限定 slice で外側 JSON の実装を既存依存の serde へ移行した。
oplog / resolution の JSON 実装は本変更に含めない。

## Decision

### 保存先・形式

- 保存先: **`$KAGI_LOG_DIR/drafts/`(設定時)→ なければ `$HOME/.kagi/drafts/`**。oplog の path 解決
  (`operations.jsonl` と同じ仕組み)を踏襲。headless テストは `KAGI_LOG_DIR` で決定的に。
- 1 draft = 1 ファイル。ファイル名は **`<sha1(repo_path + "\0" + branch_name)>.json`**(repo + branch で一意。
  同名 branch を別 repo で衝突させない)。
- 形式: **serde の struct + derive による JSON**。既存ファイルのキーと型は維持する:
  ```json
  {"repo":"<abs path>","branch":"<name>","message":"<本文>","mode":"plain|template","updated":<unix秒>}
  ```
  - template モードの場合、本文はテンプレ展開後の plain text を `message` に持つ(復元時はそのまま Input に流す。
    構造化フィールドの分解保存は MVP では行わない — 復元の確実性優先)。
  - 保存は借用フィールドの `Serialize` struct を使い、autosave のたびに本文をコピーしない。
    読み込みは既存 `Draft` に `Deserialize` を derive する。Unicode surrogate pair も正しく復元する。
  - `branch` / `message` は必須。省略された `repo` は空文字列、`mode` は `"plain"`、
    `updated` は `0` とし、未知のフィールドは無視する。従来どおり外側は object に限定する。
    壊れた JSON・型違いは draft 無視 = 空から開始し、commit を妨げない。
  - Issue draft の `message` 内に保存する `[title, body]` tuple は既存の serde_json のまま。
    保存先・SHA-1 key・同一ディレクトリの temporary file + rename 境界・queue は変更しない。

### タイミング(debounce)

- Input 変更のたびに即書きせず、**250ms debounce**。既存 `schedule_modal_replan`(generation counter + 250ms
  `gpui::Timer` + 最新世代のみ実行)の機構を参考にした draft 専用のスケジューラを置く。
- 書き込みは **background**(`cx.background_spawn`)で行い UI を塞がない(avatar / oplog と同様)。
- 空 message(trim 後空)になったら draft ファイルを **削除**(空 draft を残さない)。

### ライフサイクル

- **読み込み**: repo open / branch 切替時に該当 draft を読み、message Input に流す(Input が既に非空なら
  上書きしない — ユーザー入力優先)。
- **clear**: `execute_commit` / `execute_amend` 成功時に、その branch の draft ファイルを削除。
  失敗時は残す(再試行できるように)。
- **branch 切替**: 現 branch の draft を保存 → 新 branch の draft を読込(branch ごと独立)。

## Consequences

- oplog / avatar と保存先・env override・background 書き込みの流儀を共有する。
- 既にある serde / serde_json 依存を使い、draft の手書き escape / unescape / field extractor を廃止する。
- repo / branch / linked-worktree の draft 分離は、従来どおり working-tree path をキーに含めて維持する。
