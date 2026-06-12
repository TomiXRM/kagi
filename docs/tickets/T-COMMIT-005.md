# T-COMMIT-005: Commit Checklist — secret / .env 検出(warn, override 可)

- Status: todo
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

- [ ] `.env` を stage → warn。`.env.example` → warn しない
- [ ] PRIVATE KEY ヘッダを含むファイル → warn
- [ ] 通常コードファイル → warn しない
- [ ] unit test: ファイル名ヒット / 除外 / 内容ヒット / 非ヒット、計 4+
- [ ] `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記

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
