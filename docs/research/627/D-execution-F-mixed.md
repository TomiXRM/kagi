# #627 P5 — 実行経路と mixed backend

## 実施条件

- macOS arm64
- release binary: `target/release/examples/p5_driver`
- fixture: synthetic、commits `3`、depth `3`、seed `627`
- tracked files: `1,000` / `5,000` / `20,000` / `50,000`
- backend ごとに `KAGI_LOG_DIR` を別 temporary directory に設定
- hardened CLI `git status` は index stat cache を書き戻すため、両 backend を iteration ごとの pristine copy へ揃えた
- `--iterations 12`。先頭 1 回を warm-up として破棄し、残り 11 回の median を採用

## D1 — `working_tree_status`、file 数傾き

| tracked files | libgit2 median (ms) | CLI median (ms) |
|---:|---:|---:|
| 1,000 | `55.536` | `39.238` |
| 5,000 | `278.294` | `115.417` |
| 20,000 | `1,113.757` | `423.117` |
| 50,000 | `2,906.703` | `3,078.617` |

libgit2 の 1k→50k median は `+2,851.167 ms`、傾きは約 `58.2 ms / 1k files` である。D1 の `0.5 ms / 1k files` 基準を大きく超え、未変更 tracked file を読み直す `stash_save2` 型の疑いとして D2 の対象にする。

L（50k）では libgit2 は CLI より `171.913 ms` 速い。CLI 採用基準である「libgit2 が 1.5 倍以上遅く、絶対差 150 ms 以上」を満たさないため、`working_tree_status` を CLI 採用候補にはしない。

CLI の 50k median は 3 秒台までばらついた。両 backend とも fresh copy を使う process-warm 条件であり、D1 の傾きは検出できたが cache / filesystem の交絡を含む。D2 は macOS `fs_usage` で open/read 原因を調べる。
