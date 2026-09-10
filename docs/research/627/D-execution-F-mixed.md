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

| tracked files | libgit2 median (ms) | CLI median (ms) | 判定 |
|---:|---:|---:|---|
| 1,000 | `55.536` | `39.238` | 有効 |
| 5,000 | `278.294` | `115.417` | 有効 |
| 20,000 | `1,113.757` | `423.117` | 有効 |
| 50,000 | `2,906.703` | `3,078.617` | 保留 |

1k→20k の libgit2 median は `+1,058.221 ms`、傾きは約 `55.7 ms / 1k files` である。D1 の `0.5 ms / 1k files` 基準を大きく超え、未変更 tracked file を読み直す `stash_save2` 型の疑いとして D2 の対象にする。

50k の既存 11 sample は安定していない。libgit2 range は `2,605.549–6,572.795 ms`、p95 は `6,572.795 ms`。CLI range は `1,033.112–5,844.586 ms`、p95 は `5,844.586 ms`。median の `171.913 ms` 差は、この range 内で backend 優劣を示さない。

P4 A1 の 50k 値（libgit2 `4,153 ms` / CLI `1,317 ms`）とは初回 11 sample の median が逆だった。どちらかを誤りとは扱わず、両方とも filesystem cache / pristine-copy の状態に支配された測定である可能性を残す。初回 50k の CLI 採用判定は保留し、20k 以下の一貫した傾きだけを D2 の原因調査根拠に使った。


### D1 — 50k 再測定

各 backend で `--iterations 21` を実行し、先頭の warm-up を除く 20 sample を使った。

| backend | median (ms) | p95 (ms) | range (ms) |
|---|---:|---:|---:|
| libgit2 | `3,025.803` | `7,891.732` | `2,661.309–8,526.823` |
| CLI | `1,073.654` | `3,454.947` | `1,026.872–3,988.477` |

再測定 median では libgit2 は CLI の約 `2.82` 倍、絶対差 `1,952.149 ms` であり、D1 の L 規模 CLI 採用候補基準を満たす。一方、両 backend の range と p95 は大きく、filesystem cache / copy の交絡を除外できない。したがってこれは **CLI 採用の確定ではなく D2 と F へ進む候補判定** とする。

先行 11 sample および P4 A1 との食い違いは解消されていない。後続判断では 50k の単一 median ではなく、この 20 sample の median / p95 / range を併記する。
