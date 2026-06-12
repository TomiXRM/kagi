# ADR-0042: Commit Message Draft Storage

- Status: Accepted / Date: 2026-06-13

## Context

書きかけの commit message を、branch ごとに保存して再起動後も復元し、commit 成功時に clear したい。
保存先・形式・タイミング(debounce)を、既存の oplog / avatar キャッシュ流儀(手書き JSON・serde 禁止・
`~/.kagi/`・env override)に合わせて決める。

## Decision

### 保存先・形式

- 保存先: **`$KAGI_LOG_DIR/drafts/`(設定時)→ なければ `$HOME/.kagi/drafts/`**。oplog の path 解決
  (`operations.jsonl` と同じ仕組み)を踏襲。headless テストは `KAGI_LOG_DIR` で決定的に。
- 1 draft = 1 ファイル。ファイル名は **`<sha1(repo_path + "\0" + branch_name)>.json`**(repo + branch で一意。
  同名 branch を別 repo で衝突させない)。
- 形式: **手書き JSON(serde 禁止、oplog と同方式)**。最小フィールド:
  ```json
  {"repo":"<abs path>","branch":"<name>","message":"<本文>","mode":"plain|template","updated":<unix秒>}
  ```
  - template モードの場合、本文はテンプレ展開後の plain text を `message` に持つ(復元時はそのまま Input に流す。
    構造化フィールドの分解保存は MVP では行わない — 復元の確実性優先)。
  - 文字列のエスケープは oplog の手書き JSON writer を再利用(`"` / `\` / 制御文字)。読みは寛容パーサ
    (壊れていたら draft 無視 = 空から開始。draft 破損で commit を妨げない)。

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

- oplog / avatar と同じ `~/.kagi/`・手書き JSON・env override・background 書き込みの流儀で一貫
- serde を入れない(コードベース方針の維持)。手書き JSON の writer/parser は oplog から流用 or 小ヘルパ共有
- per-worktree の draft 分離(同 branch を複数 worktree で開く)は MVP 外。repo_path をキーに含めることで
  将来 worktree path をキーに拡張可能(YAGNI、今は branch 単位)
