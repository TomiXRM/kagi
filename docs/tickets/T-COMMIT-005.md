# T-COMMIT-005: Commit Checklist — secret / .env 検出(warn, override 可)

- Status: done
- 依存: T-COMMIT-003 / ADR-0043 §rule 5 / ADR-0039(override)
- 関連: lane W14-CHECK

## 背景

`.env` や秘密鍵らしき staged ファイルを warn する。誤検知があり得るため block ではなく warn + override 可。

## スコープ

- `checklist.rs` に secret ルール(warn):
  - **ファイル名**: `.env`(`.env.*` 含む、`.env.example`/`.sample`/`.template` 除外)/ `id_rsa` / `id_ed25519` /
    `*.pem` / `*.key` / `*.pfx` / `*.p12` / `credentials` / `secrets.*`
  - **内容**(staged BLOB 先頭数 KiB、テキストのみ): `-----BEGIN ...PRIVATE KEY-----` / `AKIA[0-9A-Z]{16}` /
    保守的な少数 token prefix(`ghp_` / `xoxb-`)
- いずれかヒットで warning に追加(ファイル名つき)。block にはしない。

## 完了条件

- [x] `.env` を stage → warn。`.env.example` → warn しない
- [x] PRIVATE KEY ヘッダを含むファイル → warn
- [x] 通常コードファイル → warn しない
- [x] unit test: ファイル名ヒット / 除外 / 内容ヒット / 非ヒット、計 4+
- [x] `cargo test` 全パス + own-code warning 0
- [x] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/git/checklist.rs` / `tests/checklist_test.rs`
- `docs/tickets/T-COMMIT-005.md`

## 触ってはいけないファイル

- `src/ui/*` / 他チケットのファイル / `Cargo.toml`

## テスト方法

1. `cargo test`
2. tempdir に各種ファイルを stage
3. override は UI 側(別チケット/PM)。本チケットは warning を返すところまで

## リスク・規約

- パターンは保守的に(false positive で warn 連発しない)。token prefix は少数に絞る
- 内容走査は先頭数 KiB のみ(性能)

## 実装メモ(W14-CHECK / 完了)

- `src/git/checklist.rs` の `checklist()` 内で rule 5(warn)を実装。`plan_commit` から呼ばれ warnings に載る。
- ファイル名(`path_is_secret_name`): `.env` / `.env.*`(`.env.example` / `.env.sample` / `.env.template` 除外、
  小文字比較)、`id_rsa` / `id_ed25519` / `credentials` / `secrets` / `secrets.*`、拡張子 `pem` / `key` / `pfx` / `p12`。
  ファイル名ルールは BLOB 取得前に判定(削除 staged でも名前で warn 可)。
- 内容(`content_has_secret`、先頭 8 KiB = `SECRET_SCAN_BYTES`、テキスト BLOB のみ): `-----BEGIN ...PRIVATE KEY-----`
  ヘッダ / `AKIA` + 16 桁英数字(`contains_aws_access_key`、`chars()` ベースの手書き走査、regex 不使用) /
  `ghp_` / `xoxb-`。いずれも block ではなく warn(override 可、ADR-0039)。
- test: `env_dotfile_warns` / `env_example_excluded` / `private_key_content_warns` /
  `ordinary_code_no_secret_warn` / `pem_key_name_warns` / `plan_commit_surfaces_secret_warning`
  + lib unit test `secret_file_names` / `secret_content`。
