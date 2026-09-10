# 627 P4 — 読み経路の測定

> 状態: 実行中。P1 review により A1 の libgit2 timer 境界を修正済み。旧 S/M/L performance 値は timer 境界不正のため撤回した。P0 の fresh copy が作る stale index stat cache と、bare Git が行う index 修復を分離した S / M / 1k / 5k / 20k / L 再測定を記録済み。A0 / A2、process-cold、dirty L、3 OS の backend 選定は未確定。
>
> branch: `exp/627-p4-read-paths`
>
> 初期実装: `b79f9270`

## 測定契約

- `crates/kagi-git/examples/backend_probe/a.rs` の `WorkingTreeStatus::execute` は、先頭で `context.backend()` を `libgit2` と `cli` に分岐する。
- 各実行で `KAGI_LOG_DIR` を設定する。oplog が harness の pristine copy 外へ出ることを防ぐ。
- CLI の production candidate は `git --no-optional-locks status --porcelain=v2 -z --untracked-files=all --renames` とする。
- `bare` CLI は side-effect 比較だけに使う。性能判定値には使わない。
- 3 iteration は backend 分岐・canonicalization・fingerprint の初期確認用。本測定の P0 warm series は warm-up 1 回を破棄した 11 iteration と process-cold を使う。加えて、index stat cache の状態を分離する direct experiment は template copy を一度作り、timer 外の bare Git repair 後に warm-up 2 回と 30 iteration を使う。

## `--no-optional-locks` の判断

Kagi が将来 CLI で status を実装する場合にも `--no-optional-locks` を付ける。

| 根拠 | 観測 / 要件 |
| --- | --- |
| 読み取りの非書き込み | bare `git status` は P0 fingerprint を変え、同一 warm series の 3 回測定を成立させなかった。`--no-optional-locks` は 3 回とも fingerprint を維持した。 |
| watcher 経路 | status は watcher tick ごとに走る。optional index write を許すと repository state を繰り返し変える。 |
| 安全性 | ADR-0146 と同じく、repository state を変えない読み取り経路を優先する。 |

原因は index stat cache と断定しない。§4.6 の「Git が読み取りでも index を書き戻し得る」交絡が実際に現れた、という事実だけを記録する。

## P1 review — timer 境界の修正

旧 runner は libgit2 の各 iteration 内で `Repository::open` していた。CLI は process 起動を iteration 内に含める一方、production の libgit2 read path は `RepoSession` が open 済み repository を再利用する。したがって旧 report の libgit2 timing は production 比較に使えない。

P0 runner は nonmutating warm series に `prepare_series` / `finish_series` lifecycle を追加した。A1 は warm-series copy の materialize 後、timer 前に `Repository::open` を一度だけ行い、`series_setup.repository_open_ns` に tab-open 相当の別コストとして記録する。series 終了時には handle を drop する。

この変更前に記録した S/M/L の 11 iteration 値、および M の CLI / libgit2 比率は性能値として撤回する。raw JSON は下表に保持するが、timing を backend 選定に使ってはならない。

### 修正後の初期確認 — S clean

| backend | candidate | timer 内 wall ms（3 iteration） | series setup | Git process | canonical / fingerprint |
| --- | --- | --- | --- | ---: | --- |
| libgit2 | — | 12.472 / 11.197 / 10.641 | `repository_open_ns = 157,167`（0.157 ms、timer 外） | 0 | `[]` / 不変 |
| CLI | `--no-optional-locks` | 26.042 / 26.367 / 13.851 | — | 1 | `[]` / 不変 |

3 回は lifecycle・backend 分岐・fingerprint の確認だけであり、性能結論に使わない。修正後の S/M/L 11 iteration warm、process-cold を改めて実行する。
### 修正後の clean warm 規模系列

reusable repository を用い、11 回の最初を warm-up として破棄した。S は generator fixture（200 tracked files）、M は Kagi source clone（1,194 tracked files）、L は generator fixture（50,000 tracked files）である。

| size | files | libgit2 open ms（timer 外） | libgit2 median / p95 ms | CLI median / p95 ms | CLI / libgit2 | libgit2 min–max ms | CLI min–max ms |
| --- | ---: | ---: | --- | --- | ---: | --- | --- |
| S | 200 | 0.196 | 19.021 / 20.170 | 24.543 / 26.222 | 1.29 | 11.676–20.170 | 22.525–26.222 |
| M | 1,194 | 0.172 | 228.355 / 249.411 | 215.559 / 229.545 | 0.94 | 225.579–249.411 | 208.234–229.545 |
| L | 50,000 | 0.219 | 4,153.127 / 5,455.873 | 1,317.064 / 1,476.294 | 0.32（stale-index 条件。採否非使用） | 3,793.953–5,455.873 | 1,235.606–1,476.294 |

p95 は warm-up を除いた n=10 の nearest-rank 値であり、n=10 では最大値と同じである。`Repository::open` は全規模で 0.219 ms 以下の timer 外コストだった。S/M はこの series 内では相対的に安定し、S では CLI が遅く、M はほぼ同等だった。

L の既存値は消去しないが、P0 の fresh copy を使い、`--no-optional-locks` と `core.fsmonitor=` を使う **stale index stat cache 条件**の観測として分類する。P0 は read-only operation に一 copy の warm series を使うが、`run_probe` は invocation ごとに template を `materialize_pristine` する。copy は index の stat data と file の inode / ctime を不一致にし得る。さらにこの CLI candidate と libgit2 は L の timed series 中に index mtime を更新しなかった。そのため既存 L の `0.32` は backend 固有の採否・傾き・性能閾値の根拠に使わない。

### L 50k — index stat cache を正規化した direct 再測定

元の L template から新しい copy を一度だけ作り `chmod -R u+w` を実行した。timer 外の bare `git status --porcelain=v2 -z` を 1 回実行して index mtime が `1788932046792.2166` から `1789033150211.511` へ変化したことを確認した。続く exact P4 candidate の計時中は mtime 不変である。各 backend は 2 warm-up 後に 30 回測定し、順序を反転した。

| 実行順 | libgit2 median / p95 / min–max ms | CLI median / p95 / min–max ms | CLI / libgit2 |
| --- | --- | --- | ---: |
| libgit2 → CLI | 152.776 / 196.493 / 140.216–219.712 | 95.228 / 101.297 / 90.043–104.390 | 0.62 |
| CLI → libgit2 | 209.579 / 438.934 / 156.303–2,020.167 | 107.250 / 364.164 / 89.828–395.101 | 0.51 |

同じ direct L の stale-index 30 sample は libgit2 median `3,049–3,133 ms`、CLI median `963–1,071 ms` だった。bare Git repair 後は双方とも桁違いに短縮したため、stale index と warm index を混ぜた backend 比は無効である。一方、warm index でも順序ごとの median と p95 / range は動く。50k は単一 median の backend 選定値ではなく、index state と tail を併記する観測に留める。

### 20k — 同じ index stat cache 効果の確認

L と同じ generator parameter（seed 627 / 2,000 commits / depth 6）で 20,000 tracked files の template を作り、新しい writable copy を使った。stale series 中は index mtime 不変、timer 外 bare Git repair で mtime が変化し、その後の timed series は再び不変だった。

| index state | libgit2 median / p95 / min–max ms | CLI median / p95 / min–max ms | CLI / libgit2 |
| --- | --- | --- | ---: |
| stale | 1,179.807 / 1,652.032 / 1,101.294–2,024.240 | 423.823 / 1,186.089 / 372.762–1,590.953 | 0.36 |
| bare Git repair 後 | 77.523 / 124.601 / 70.487–148.353 | 61.176 / 102.617 / 52.239–143.143 | 0.79 |

この効果は 50k 専用ではない。P5 の既存 20k series は P5 owner が同じ state-normalized procedure で判断し直す。P4 は P5 report を変更しない。以下では P4 の S/M を同じ手順で測り直し、さらに同一 generator の A3 scale series だけから傾きを再計算する。

### S / M — state-normalized direct 再測定

既存 S（200 files）と M（Kagi source clone、1,194 files）も、新しい writable copy を bare Git repair 後に各 backend 2 warm-up + 30 iteration、両順序で測った。両 fixture は generator parameter が異なるため、この表は S/M の比較観測であり傾きの入力には使わない。

| fixture | libgit2 → CLI: libgit2 / CLI median ms | CLI → libgit2: libgit2 / CLI median ms | CLI / libgit2 range |
| --- | --- | --- | ---: |
| S / 200 | 1.060 / 9.570 | 1.008 / 10.008 | 9.03–9.92 |
| M / 1,194 | 4.172 / 11.135 | 4.977 / 11.393 | 2.29–2.67 |

各 timed series は index mtime 不変である。p95 と min–max は raw JSON に残す。

### A3 — state-normalized scale decision table

1k / 5k / 20k / 50k は同じ synthetic generator（seed 627 / 2,000 commits / depth 6）で作った。各点は `copy → chmod -R u+w → timer 外 bare Git repair → 2 warm-up → 30 measured`、両順序である。表の median は `libgit2 → CLI / CLI → libgit2` の順で併記し、単一比率を採否値にしない。

| files | libgit2 median ms | CLI median ms | CLI / libgit2 range |
| ---: | --- | --- | ---: |
| 1,000 | 22.858 / 21.122 | 30.968 / 31.215 | 1.35–1.48 |
| 5,000 | 33.807 / 31.557 | 37.349 / 37.787 | 1.11–1.20 |
| 20,000 | 77.523 / 68.150 | 61.176 / 58.336 | 0.79–0.86 |
| 50,000 | 152.776 / 209.579 | 95.228 / 107.250 | 0.51–0.62 |

順序別 OLS（files を 1k 単位）は、libgit2 が `2.651–3.875 ms / 1k`（$R^2 = 0.980–0.999$）、CLI が `1.304–1.547 ms / 1k`（$R^2 = 0.993–0.999$）だった。傾き比は libgit2 が `2.03–2.51×`。これは stale index を含む旧傾きではなく、repaired index 条件の本 host に限る観測である。

本表だけで backend を切り替えない。P5 との host 差、50k の order-dependent tail、A2 の情報等価性、3 OS gate を満たすまで採否は未確定とする。少なくとも「旧 20k 傾きを決定根拠にする」ことは取り消す。

### `--git-executable` と fsmonitor 条件

- CLI probe は `GitCliOptions` を通じて `ProbeContext::git_executable()` を実行 binary に渡す。`--git-executable /usr/bin/git` の 1 iteration は report の `git_executable` に `/usr/bin/git`、canonical output に `git_processes = 1` を記録した。
- repository config の fsmonitor executable は信頼しない。probe の `no-optional-locks-fsmonitor` candidate は `-c core.fsmonitor=true` に固定して Git built-in のみを有効化する。default production candidate は `-c core.fsmonitor=` のままである。
- M fixture の `core.fsmonitor=true` / `core.untrackedCache=true` で built-in fsmonitor candidate を 3 iteration 実行すると、1 回目後に fingerprint が変わり 2 回目開始前の P0 check が失敗した。1 iteration の canonical status は libgit2 と一致するが、watcher read としては採用できない。
- 同じ fixture で fsmonitor を probe 側から disabled にし、untracked cache は config `true` のままにすると、libgit2 / CLI とも 3 iteration の canonical status と fingerprint が一致した。CLI output は `fsmonitor = "disabled"`、libgit2 の timer 外 open は 0.256 ms だった。

### rename の情報等価性

M fixture の R100、Git が R63 と報告する内容変更 rename、rename 非検出の add + delete で、両 backend は同じ canonical status を返した。この意味等価性の観測は timer 境界の影響を受けない。ただし S / L の rename と similarity 50% 近傍は未確認である。

## Raw JSON

| case（P1 前 timing は性能値ではない） | JSON |
| --- | --- |
| S clean / libgit2 / 3 | `/tmp/kagi-627-p4/results/a1-S-libgit2-3.json` |
| S clean / CLI bare / 1 | `/tmp/kagi-627-p4/results/a1-S-cli-bare-1.json` |
| S clean / CLI `--no-optional-locks` / 3 | `/tmp/kagi-627-p4/results/a1-S-cli-no-optional-locks-3.json` |
| S dirty / libgit2 / 3 | `/tmp/kagi-627-p4/results/a1-S-status-libgit2-3.json` |
| S dirty / CLI `--no-optional-locks` / 3 | `/tmp/kagi-627-p4/results/a1-S-status-cli-3.json` |
| M R100 registered / libgit2 / 11 warm | `/tmp/kagi-627-p4/results/a1-M-status-libgit2-11-registered.json` |
| M R100 registered / CLI `--no-optional-locks` / 11 warm | `/tmp/kagi-627-p4/results/a1-M-status-cli-11-registered.json` |
| M R63 registered / libgit2 / 3 | `/tmp/kagi-627-p4/results/a1-M-near-threshold-libgit2-3-registered.json` |
| M R63 registered / CLI `--no-optional-locks` / 3 | `/tmp/kagi-627-p4/results/a1-M-near-threshold-cli-3-registered.json` |
| M nonrename registered / libgit2 / 3 | `/tmp/kagi-627-p4/results/a1-M-threshold-libgit2-3-registered.json` |
| M nonrename registered / CLI `--no-optional-locks` / 3 | `/tmp/kagi-627-p4/results/a1-M-threshold-cli-3-registered.json` |
| S clean registered / libgit2 / 11 warm | `/tmp/kagi-627-p4/results/a1-S-clean-libgit2-11-registered.json` |
| S clean registered / CLI `--no-optional-locks` / 11 warm | `/tmp/kagi-627-p4/results/a1-S-clean-cli-11-registered.json` |
| M clean registered / libgit2 / 11 warm | `/tmp/kagi-627-p4/results/a1-M-clean-libgit2-11-registered.json` |
| M clean registered / CLI `--no-optional-locks` / 11 warm | `/tmp/kagi-627-p4/results/a1-M-clean-cli-11-registered.json` |
| L clean registered / libgit2 / 11 warm | `/tmp/kagi-627-p4/results/a1-L-clean-libgit2-11-registered.json` |
| L clean registered / CLI `--no-optional-locks` / 11 warm | `/tmp/kagi-627-p4/results/a1-L-clean-cli-11-registered.json` |
| S clean reusable repository / libgit2 / 3 | `/tmp/kagi-627-p4/results/a1-S-clean-libgit2-3-reuse.json` |
| S clean reusable repository / CLI / 3 | `/tmp/kagi-627-p4/results/a1-S-clean-cli-3-reuse.json` |
| S CLI selected `/usr/bin/git` / 1 | `/tmp/kagi-627-p4/results/a1-S-cli-selected-git-1-reuse.json` |
| M cache on / libgit2 reusable repository / 3 | `/tmp/kagi-627-p4/results/a1-M-cache-on-libgit2-3-reuse.json` |
| M untracked cache on, fsmonitor disabled / CLI / 3 | `/tmp/kagi-627-p4/results/a1-M-untracked-cache-cli-3-reuse.json` |
| M built-in fsmonitor / CLI / 1 | `/tmp/kagi-627-p4/results/a1-M-cache-on-cli-1-reuse.json` |
| S clean reusable repository / libgit2 / 11 warm | `/tmp/kagi-627-p4/results/a1-S-clean-libgit2-11-reuse.json` |
| S clean reusable repository / CLI / 11 warm | `/tmp/kagi-627-p4/results/a1-S-clean-cli-11-reuse.json` |
| M clean reusable repository / libgit2 / 11 warm | `/tmp/kagi-627-p4/results/a1-M-clean-libgit2-11-reuse.json` |
| M clean reusable repository / CLI / 11 warm | `/tmp/kagi-627-p4/results/a1-M-clean-cli-11-reuse.json` |
| L clean reusable repository / libgit2 / 11 warm | `/tmp/kagi-627-p4/results/a1-L-clean-libgit2-11-reuse.json` |
| L clean reusable repository / CLI / 11 warm | `/tmp/kagi-627-p4/results/a1-L-clean-cli-11-reuse.json` |
| L direct stale index / 2 warm-up / 30 × 2 order | `/tmp/kagi-627-p4/results/a1-L-direct-warm-order-balanced-30.json` |
| L direct warm index / 2 warm-up / 30 × 2 order | `/tmp/kagi-627-p4/results/a1-L-warm-index-order-balanced-30.json` |
| 20k stale / bare Git repair / warm index / 30 × 2 order | `/tmp/kagi-627-p4/results/a1-20k-index-cache-confirmation-30.json` |
| S / M warm index / 30 × 2 order | `/tmp/kagi-627-p4/results/a1-S-M-warm-index-order-balanced-30.json` |
| A3 1k / 5k warm index / 30 × 2 order | `/tmp/kagi-627-p4/results/a3-1k-5k-warm-index-order-balanced-30.json` |
| A3 repaired-index order-separated OLS | `/tmp/kagi-627-p4/results/a3-warm-index-decision-fit.json` |

## 次の測定
1. reusable repository で clean S / M / L の process-cold を測定する。
2. dirty / rename / untracked の L case。
3. fsmonitor disabled / untracked cache enabled 条件を S / L に広げる。built-in fsmonitor は P0 fingerprint を変えたため、watcher candidate から除外する。
4. A2 の libgit2 `snapshot` と CLI 合成の段別 process / time / 情報欠落。
5. A0 の GUI watcher scenario。`KAGI_BENCH_READ=1` の raw event、debounce 後 tick、reload、`snapshot` / `working_tree_status` 時間を記録する。
