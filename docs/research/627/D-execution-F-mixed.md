# #627 P5 — 実行経路と mixed backend

## 実施条件

- macOS arm64
- P0 登録済みの `backend_probe --operation execution-mixed` のみを実行経路に使う。私設 driver は dispatch / warm-up 契約を重複させるため削除した。
- fixture: synthetic、commits `3`、depth `3`、seed `627`
- tracked files: `1,000` / `5,000` / `20,000` / `50,000`
- backend ごとに `KAGI_LOG_DIR` を別 temporary directory に設定
- D1 は plan 指定どおり tracked file 2 個を dirty にする。hardened CLI の local override が無い clean fixture の値は #649 override cost を含まない lower bound として記録する。
- P0 runner の read-only contract は 1 copy warm series である。ただし index 修復を timer 外に置く hook は P0 に無く、copy ごとの ctime / inode mismatch を避ける warm-index 比較は P0 owner の支援が必要である。

## D1 — `working_tree_status`、file 数傾き


初回の private driver / clean fixture 測定は、P0 登録経路・dirty 2 file・同一 harness contract を満たさない。数値は設計判断に用いず、raw report も引用しない。

### stale-index series（P0 経路で再測定予定）

P0 の materialize は copy ごとに ctime / inode と index stat cache を乖離させる。CLI は `status` で index を自己修復でき、libgit2 は refresh を index に書き戻さないため、pristine copy の初回値はこの非対称性を含む。これは「libgit2 が遅い」の結論ではなく、stale index 修復の必要性を示す分類である。

#### per-iteration copy（historic）

| tracked files | libgit2 median (ms) | CLI median (ms) | 分類 |
|---:|---:|---:|---|
| 1,000 | `55.536` | `39.238` | stale-index |
| 5,000 | `278.294` | `115.417` | stale-index |
| 20,000 | `1,113.757` | `423.117` | stale-index |
| 50,000 | `2,906.703` | `3,078.617` | stale-index、裾が大きい |

50k の 11 sample は libgit2 range `2,605.549–6,572.795 ms` / p95 `6,572.795 ms`、CLI range `1,033.112–5,844.586 ms` / p95 `5,844.586 ms`。21 iteration series は libgit2 median/p95/range `3,025.803/7,891.732/2,661.309–8,526.823 ms`、CLI `1,073.654/3,454.947/1,026.872–3,988.477 ms` だった。いずれも stale-index 条件として保存し、通常 backend 比較には使わない。

各 size は raw iteration order、`wall_ns`、`user_ns`、`sys_ns`、canonical status count を残す。warm-up を除いた sample から min / median / nearest-rank p95 / max を出す。順序を sort した配列だけは記録しない。

### warm-index 比較

P0 runner には `materialize → chmod → plain git status → timer` の timer 外 hook がない。私設 driver を同条件の代替にしてはならない。したがって **P0 registered path での warm-index 比較は未測定** である。下記は既存の direct measurement であり、P0 owner が hook を提供するまで canonical にはしない。

#### direct warm-index（historic private driver）

この series は materialize 後に `chmod -R u+w` と plain `git status` を timer 外で行った。private driver は削除済みであり、P0 registered path の再現値ではないが、測定済みの数値を撤回しない。

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

## D2 — syscall 分類

macOS の `fs_usage -w -f filesys` は root 権限を要求して終了した。syscall 回数・読取 bytes は未測定である。root 不要の次の判別は、file 数固定で total byte 数だけを変える fixture を P0 経路で測り、wall / user / sys の比を比較することにする。


## F — mixed backend の追加照合

`backend_fixture --scenario mixed-stash` は未実装のままだが、PR #659 が `MixedStash` を P0 dispatcher に登録した。共有 fixture は変更していない。

F は timing 実験ではなく divergence 実験である。現行 runner では `execute` が timer 内で呼ばれるため、固定 setup は `MixedStash::execute` 内で行い、`wall_ns` は setup を含む非比較値として扱う。判定は plan 予測、run_recorded の verify/oplog evidence、実行後 status の食い違いだけに基づける。

各 iteration は `mutates_fixture=true` により synthetic pristine copy を materialize してから、次の固定導出を行う。

1. index の先頭 tracked path を選び、元の bytes に `\nP5 staged stash content\n` を付加する。
2. hardened `run_git <repo> add <path>` を実行する。
3. 同じ元の bytes に `\nP5 unstaged stash content\n` を付加する。
4. hardened `run_git <repo> stash push --include-untracked -m p5-mixed-stash` を実行する。

その後に libgit2 の `Backend::plan` と `run_recorded` で `f-apply` / `f-pop` / `f-drop` を実行する。report は manifest、plan blockers/warnings、action error、oplog recovery handle、verified、実行後 status count を記録する。全 write の `plan → confirm → preflight → execute → verify → oplog` は結果に関係なく必須であり、verify の省略・弱化を提案しない。

### 結果

fixture は `/tmp/kagi-627-p4/S`（S、tracked `200` files）。各 candidate は `3`反復で、次の registered command を使った（`f-pop` / `f-drop` は candidate だけを置換）。

```sh
backend_probe --repo /tmp/kagi-627-p4/S --operation mixed-stash \
  --backend libgit2 --candidate f-apply --iterations 3 --format json
```

CLI が作った stash に libgit2 の `Backend::plan` / `run_recorded` を通す混在経路で、探索した範囲の plan 予測と実行後 state の食い違い件数は **`0`** だった。全 3 反復で同値だった。

| candidate | verified | blockers | action error | post-state (staged / unstaged / untracked) |
|---|---|---|---|---|
| `f-apply` | `true` | `[]` | `None` | `0 / 1 / 0` |
| `f-pop` | `true` | `[]` | `None` | `0 / 1 / 0` |
| `f-drop` | `true` | `[]` | `None` | `0 / 0 / 0` |

これは「混在が安全」という結論ではない。未実施範囲は、stash apply が競合する同一 path の別内容、S より大きい fixture、untracked を含む復元の境界である。F が `0` 件だったことは、全 write の `plan → confirm → preflight → execute → verify → oplog` を緩める根拠にならない。
