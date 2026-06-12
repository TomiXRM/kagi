# ADR-0043: Commit Checklist Rules

- Status: Accepted / Date: 2026-06-13

## Context

Commit Checklist の各ルールの**判定方法**を確定する。ADR-0039 が block/warn の分類方針を決めたので、
本 ADR は具体ルールの検出ロジック・閾値・誤検知時の override を定める。検査対象は **staged 内容のみ**
(unstaged は commit に入らないので検査しない)。

## Decision

検査は純関数 `checklist(repo, staged) -> (blockers, warnings)`(新 module `src/git/checklist.rs`)。
`plan_commit` / `plan_amend` から呼ぶ。各ルール:

### 1. staged なし(block)— 既存

`status.staged.is_empty()` → blocker。既存 `plan_commit` 実装済み。

### 2. message 空(block)— 既存

`message.trim().is_empty()` → blocker。既存 `plan_commit` 実装済み。

### 3. unresolved conflict file が staged(block)

- repo が conflict 状態(`status.conflicted` 非空)→ blocker(既存 `plan_commit` 実装済み)。
- 加えて **conflict だったファイルが解決扱いで staged されたが marker が残る**ケースは次の rule 4 が捕捉。

### 4. conflict marker 検出(block)

- **staged BLOB を走査**(WT ファイルではなく index/staged tree の内容)。staged な各ファイルの BLOB を読み、
  行頭が **`<<<<<<< ` / `=======` / `>>>>>>> `**(7文字 + 続き)に一致する行があれば blocker。
- 対象は **テキスト BLOB のみ**(binary 判定 = NUL バイト含む or git の binary 判定 → skip。binary に marker は無い)。
- 大きい BLOB は **先頭 N(例 1MiB)まで**走査(巨大ファイルで全走査して固まらない)。
- これは block: marker 入りの commit はほぼ確実に事故。override 不可。

### 5. secret / .env 検出(warn)

- **ファイル名ヒューリスティクス**(staged path に対して):
  - `.env`(末尾一致・`.env.*` 含む、ただし `.env.example` / `.env.sample` / `.env.template` は除外)
  - `id_rsa` / `id_ed25519` / `*.pem` / `*.key`(末尾)/ `*.pfx` / `*.p12` / `credentials` / `secrets.*`
- **内容ヒューリスティクス**(staged BLOB の先頭数 KiB をテキスト走査、誤検知を抑えるため控えめに):
  - `-----BEGIN (RSA |EC |OPENSSH |PGP )?PRIVATE KEY-----`
  - `AKIA[0-9A-Z]{16}`(AWS access key)/ 既知 token prefix(`ghp_` / `xoxb-` 等、保守的に少数)
- いずれかヒットで **warn**(false positive あり得るため block にしない)。override 可(ADR-0039)。

### 6. large binary 検出(warn)

- staged な **binary BLOB**(NUL 含む or git binary 判定)のサイズが **閾値超**で warn。
- 閾値: **既定 5 MiB**(`KAGI_LARGE_BLOB_BYTES` で override 可、テスト用に小さくできる)。
- テキストの大ファイルは warn しない(diff として正当なことが多い)。binary のみ対象。
- warn 文言にサイズとファイル名を出す。override 可。

### override の扱い

- rule 5 / 6(warn)は ADR-0039 のとおり 1 クリック override 可。override したら oplog note に
  `overrode: secret(<file>) / large_binary(<file>, <bytes>)` を残す。
- rule 1〜4(block)は override 不可。

## Consequences

- 検査は staged BLOB ベース(WT を読まない)→ 「staged したものだけが commit される」原則と一致し、
  unstaged のノイズを拾わない
- conflict marker / secret 走査は **テキスト BLOB の先頭一定量のみ**で性能を担保
- 閾値・除外パターンは env / 定数で調整可。カスタムルール(ユーザー定義)は MVP 外
- checklist を独立 module にすることで commit / amend の両 plan から再利用し、unit test を集中させられる
