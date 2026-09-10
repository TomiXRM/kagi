# #627 P5 — 実行経路と mixed backend

## 実施条件

- macOS arm64
- release binary: `target/release/examples/p5_driver`
- fixture: synthetic、commits `3`、depth `3`、seed `627`
- tracked files: `1,000` / `5,000` / `20,000` / `50,000`
- backend ごとに `KAGI_LOG_DIR` を別 temporary directory に設定
- 既存の 50k series は、copy 後に index stat cache が stale な状態での測定として分類する。libgit2 は file content hash を走らせる一方、CLI は index を自己修復するため、これを通常 backend 比較には使わない。
- 再測定は `copy → chmod -R u+w → git status --porcelain`（index 修復、timer 外）→ 2 warm-up → 30 measured とする。1 copy の warm series を backend ごとに取り、libgit2 → CLI と CLI → libgit2 の両順序を別 series として記録する。

## D1 — `working_tree_status`、file 数傾き


1k→20k の初回 libgit2 median は `+1,058.221 ms`、傾きは約 `55.7 ms / 1k files` だった。ただしこの series は stale index stat cache を含むため、backend 選択の根拠にしない。20k 以下も同じ warm-index 手順で確認する。

### stale-index series（既存値）

既存値は撤回せず、copy 後の inode / ctime と index stat cache が食い違う状態を測った結果として残す。CLI は初回 `status` で index を更新し、後続の CLI と libgit2 を速くできる。libgit2 は refresh を index に書き戻さないため、stale copy 上で毎回 content hashing に入る。この非対称な自己修復を含むため、同値を「libgit2 が遅い」という比較に用いない。

| tracked files | libgit2 median (ms) | CLI median (ms) | 分類 |
|---:|---:|---:|---|
| 1,000 | `55.536` | `39.238` | stale-index |
| 5,000 | `278.294` | `115.417` | stale-index |
| 20,000 | `1,113.757` | `423.117` | stale-index |
| 50,000 | `2,906.703` | `3,078.617` | stale-index、裾が大きい |

初回 50k の 11 sample は libgit2 range `2,605.549–6,572.795 ms` / p95 `6,572.795 ms`、CLI range `1,033.112–5,844.586 ms` / p95 `5,844.586 ms` だった。21 iteration の stale-index 診断 series も、libgit2 median/p95/range `3,025.803/7,891.732/2,661.309–8,526.823 ms`、CLI `1,073.654/3,454.947/1,026.872–3,988.477 ms` として残す。

### warm-index 再測定

各 series は synthetic fixture（seed `627`、commits `3`、depth `3`）を materialize し、`chmod -R u+w` の後に hardened `run_git status --porcelain=v2 -z` を timer 外で 1 回実行して index stat cache を更新した。最初の backend は 2 warm-up 後に 30 回、次の backend は同じ warm index 上で 2 warm-up 後に 30 回実行した。p95 は nearest-rank（30 sample の 29 番目）である。

| files | order | backend | median (ms) | p95 (ms) | range (ms) |
|---:|---|---|---:|---:|---:|
| 20,000 | libgit2 → CLI | libgit2 | `85.178` | `116.531` | `62.262–117.129` |
| 20,000 | libgit2 → CLI | CLI | `43.138` | `53.195` | `30.356–54.724` |
| 20,000 | CLI → libgit2 | CLI | `41.785` | `43.742` | `38.444–45.181` |
| 20,000 | CLI → libgit2 | libgit2 | `60.867` | `67.325` | `56.293–68.665` |
| 50,000 | libgit2 → CLI | libgit2 | `206.517` | `233.508` | `138.386–250.397` |
| 50,000 | libgit2 → CLI | CLI | `74.243` | `81.608` | `65.805–83.650` |
| 50,000 | CLI → libgit2 | CLI | `95.891` | `112.739` | `71.844–115.844` |
| 50,000 | CLI → libgit2 | libgit2 | `172.969` | `186.133` | `161.855–188.231` |

stale-index series の秒単位の裾は消えた。一方この host の 50k warm-index 比は order により `1.80–2.78x` であり、P4 の `1.31x` は再現していない。差の原因は未確定である。したがって CLI 採用・libgit2 廃止の根拠にはせず、stale index を誰がいつ修復するかを主題として D2 を保留する。

## D2 — syscall 分類

macOS の計画どおりの `fs_usage -w -f filesys` は root 権限を要求して終了した。したがって syscall 回数・読取 bytes は未測定である。warm-index 結果は stale-index artifact を示したが、backend 比の差と順序差の原因は未確定なため、D2 は root 権限を得られる環境まで保留する。

## F — mixed backend の追加照合

計画が指定する `backend_fixture --scenario mixed-stash` は P0 fixture で未登録だった。共有 fixture / root dispatcher は変更せず、pristine synthetic copy に timer 外で固定の stash setup を適用する決定的導出へ切り替える。

setup コマンド列、各 copy の manifest、backend 組合せ、plan 予測と execute 後状態の差を記録する。全 write の `plan → confirm → preflight → execute → verify → oplog` は結果に関係なく必須であり、verify の省略・弱化を提案しない。
