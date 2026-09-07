# ADR-0190: `git log` の framing を NUL に統一する

## Status

Accepted (2026-09-08) — issue #508

## Context

`git log` の出力を読むパーサは、ASCII の record separator (`\x1e`) / unit
separator (`\x1f`) をレコード・フィールド区切りとして使っていた。「これらは
commit metadata に現れない」という前提を置いていたが、これは**誤り**である。
commit message は任意のバイト列を持てるので、`\x1e` と偽レコードを本文に
埋め込んだ commit を 1 件作るだけで、パーサはそれを 2 レコードとして受理する。

再現 (issue #508, 監査 revision `5a5fd0f0`): 実 git で作った 1 commit に対し
`git rev-list --count HEAD = 1`、パーサ出力 `= 2`。偽の oid
`0000…0000` / author `Eve` / summary `forged row` の行がグラフに現れた。

これは「untrusted なリポジトリを**読むだけ**で、すべての安全判断が乗っている
表示を偽装できる」ことを意味する。影響範囲:

- `crates/kagi-domain/src/remote_snapshot.rs` — remote (SSH) snapshot の commit graph
- `crates/kagi-git/src/file_history.rs` — File History (本文が `--raw`/`--numstat`
  trailer を食い、change type とパスまで偽装できた)
- `crates/kagi-git/src/hotspot.rs` — ecosystem の churn 集計 (本文は含まないが
  author identity / path 由来の `\x1e` で record 境界が壊れうる)
- `crates/kagi-domain/src/remote.rs` — remote repo summary (`%s` が `%D` より前に
  あり、subject 中の `\x1f` で branch 名を偽装できた)

## Decision

**NUL (`%x00`) を `git log` の framing に使う。**

NUL は commit object が持てない唯一のバイトである。検証済み (git 2.50.1):

| 経路 | 結果 |
|---|---|
| `git commit -F <NUL入り>` | `error: a NUL byte in commit log message not allowed.` |
| `git commit-tree -F <NUL入り>` | 同上 |
| `git hash-object -t commit -w` | `object fails fsck: nulInCommit` → `refusing to create malformed object` |

したがって「本文が区切りバイトを含まない」という前提が、NUL に限っては
**git 側で強制された事実**になる。`git log -z` が存在する理由でもある。

1. **`remote_snapshot::LOG_FORMAT`** = `%H%x00%P%x00%an%x00%ae%x00%at%x00%cn%x00%ce%x00%ct%x00%B%x00`。
   フィールド区切りもレコード区切りも NUL。パーサは全体を NUL で split し、
   `chunks_exact(LOG_FIELDS)` で 9 個ずつ固定長で取る。**フィールド数の検証**が
   framing そのものなので、途中で切れた出力は半端な行を作らず捨てられる。
2. **`file_history::LOG_FORMAT`** = 先頭 NUL + 9 フィールド (各 NUL 終端)。
   `--raw`/`--numstat` trailer は 9 番目のフィールドの後に続き、**次レコードの
   先頭 NUL で閉じる**。よって 1 commit = ちょうど 10 chunk となり、
   `chunks_exact(LOG_CHUNKS)` で取れる。`-z` は使わない — `-z` は `--raw` /
   `--numstat` のパス側も NUL 終端に変えてしまい、レコード境界と衝突するため。
3. **`hotspot`** はレコード区切りのみ `%x1e` → `%x00`。author email は header 行の
   末尾フィールドなので `\x1f` 混入でもフィールドはずれない。
4. **`parse_repo_summary`** は format を `%h%x1f%D%x1f%s` に並べ替え、`splitn(3)`
   で読む。自由文の `%s` を最後に置く (`%h` は oid、`%D` は refname で、どちらも
   ASCII 制御文字を含めない)。

`for-each-ref` / `stash list` 系の format は `\x1f` のままにする。フィールドが
refname / oid / git 生成の track 文字列だけで、refname 規則が ASCII 制御文字を
禁止しているため。

## Consequences

- untrusted リポジトリの commit 本文・author 名・subject がどんなバイト列でも、
  レコード数と各フィールドが実履歴と一致する。本文は byte 単位で round-trip する。
- ワイヤ形式が変わるので、format 文字列とパーサは常に同じモジュールに置き、
  片方だけ変えられないようにする (従来通り)。
- regression は 2 段構え: 純粋 unit test (旧区切り・改行・単独 CR を本文に入れる)
  と、実 git fixture に対する integration test
  (`tests/log_framing_test.rs`、`tests/file_history_test.rs`) で、
  `git rev-list --count` と行数・各フィールドを突き合わせる。
- 「区切りバイトは本文に現れない」と書いた旧コメントはすべて撤去した。
