# 627 P4 — 読み経路の測定

> 状態: 実行中。A1 clean の S/M/L warm 規模系列と M の rename 境界を測定済み。A0 / A2、process-cold、cache 条件、dirty L、backend 選定は未確定。
>
> branch: `exp/627-p4-read-paths`
>
> 初期実装: `b79f9270`

## 測定契約

- `crates/kagi-git/examples/backend_probe/a.rs` の `WorkingTreeStatus::execute` は、先頭で `context.backend()` を `libgit2` と `cli` に分岐する。
- 各実行で `KAGI_LOG_DIR` を設定する。oplog が harness の pristine copy 外へ出ることを防ぐ。
- CLI の production candidate は `git --no-optional-locks status --porcelain=v2 -z --untracked-files=all --renames` とする。
- `bare` CLI は side-effect 比較だけに使う。性能判定値には使わない。
- 3 iteration は backend 分岐・canonicalization・fingerprint の初期確認用。本測定は warm-up 1 回を破棄した 11 iteration と process-cold を使う。

## `--no-optional-locks` の判断

Kagi が将来 CLI で status を実装する場合にも `--no-optional-locks` を付ける。

| 根拠 | 観測 / 要件 |
| --- | --- |
| 読み取りの非書き込み | bare `git status` は P0 fingerprint を変え、同一 warm series の 3 回測定を成立させなかった。`--no-optional-locks` は 3 回とも fingerprint を維持した。 |
| watcher 経路 | status は watcher tick ごとに走る。optional index write を許すと repository state を繰り返し変える。 |
| 安全性 | ADR-0146 と同じく、repository state を変えない読み取り経路を優先する。 |

原因は index stat cache と断定しない。§4.6 の「Git が読み取りでも index を書き戻し得る」交絡が実際に現れた、という事実だけを記録する。

## A1 初期確認 — S

fixture manifest:

| files | commits | depth | filesystem | Git | git2 |
| ---: | ---: | ---: | --- | --- | --- |
| 200 | 50 | 3 | APFS | `2.50.1 (Apple Git-155)` | `0.21.0` |

### clean status

| backend | candidate | wall ms（3 iteration） | canonical | Git process | fingerprint |
| --- | --- | --- | --- | ---: | --- |
| libgit2 | — | 11.469 / 10.896 / 10.782 | `[]` | 0 | 不変 |
| CLI | `bare` | 26.300 の 1 回目後に無効 | `[]` | 1 | 2 回目開始前に不一致 |
| CLI | `--no-optional-locks` | 26.855 / 24.313 / 55.614 | `[]` | 1 | 不変 |

`55.614 ms` は 3 回目の raw 値として残す。3 回では p95 を採用しないため、外れ値処理も性能結論も行わない。

### dirty / rename / untracked status

canonical status は両 backend の全 3 iteration で一致した。

| group | kind | path | rename_from |
| --- | --- | --- | --- |
| staged | renamed | `d00-00/d01-00/renamed-000034.bin` | `d00-00/d01-00/file-000034.bin` |
| unstaged | modified | `d00-00/d01-00/file-000000.bin` | — |
| untracked | untracked | `untracked.txt` | — |

| backend | candidate | wall ms（3 iteration） | Git process | fingerprint |
| --- | --- | --- | ---: | --- |
| libgit2 | — | 11.277 / 10.311 / 10.485 | 0 | 不変 |
| CLI | `--no-optional-locks` | 13.687 / 13.254 / 24.411 | 1 | 不変 |

この値は §5 A1 の性能判断に使わない。S/M/L、warm / process-cold、fsmonitor / untracked cache の有効・無効、情報等価性の全ケースが未完了である。

## A1 warm — M / rename

M fixture は repository source clone を独立 clone し、`Cargo.toml` を unstaged modified、`docs/rearch/architecture.md` を staged rename、`p4-untracked.txt` を untracked とした。registered `backend_probe --operation working-tree-status` により、11 回の最初を warm-up として破棄した。

### 内容不変 R100

| backend | candidate | 破棄値 ms | warm 10 回 median ms | min–max ms | Git process | canonical / fingerprint |
| --- | --- | ---: | ---: | --- | ---: | --- |
| libgit2 | — | 214.924 | 214.093 | 211.327–226.399 | 0 | 3 status と `rename_from` が一致 / 不変 |
| CLI | `--no-optional-locks` | 193.639 | 190.983 | 184.532–197.503 | 1 | 同上 / 不変 |

両 backend は `docs/rearch/architecture-renamed.md` と `docs/rearch/architecture.md` の同じ `rename_from` を返した。

### 内容変更 rename の境界

3 iteration の情報等価性確認では、文書の非空行の約 1/3 を変更した staged rename（Git の `--find-renames` が R63 と表示）でも、両 backend は全回で同じ `renamed` と同じ `rename_from` を返し、fingerprint も不変だった。半数を変更した fixture は Git が rename を検出せず add + delete になり、両 backend も全回で同じ add + delete を返した。

R100、R63、rename 非検出の三状態で同じ canonical status を返したことは、M fixture の rename 境界を跨いだ積極的な情報等価性の証拠である。ただし similarity 50% 近傍または S / L の意味等価性は未確認である。

M の warm median は CLI / libgit2 = 190.983 / 214.093 = 0.89 で、CLI がやや速い。しかし backend 選定の性能閾値 0.7 には届かず、速度を理由に CLI へ寄せる根拠にはならない。process-cold、clean case、fsmonitor / untracked cache 条件、S / L が未完了のため、性能結論は保留する。

## A1 clean warm — S / M / L 規模系列

各 fixture は clean status、CLI は `--no-optional-locks`、11 回の最初を warm-up として破棄した。S は generator fixture（200 tracked files）、M は Kagi source clone（1,194 tracked files）、L は generator fixture（50,000 tracked files）である。

| size | files | libgit2 median ms | CLI median ms | CLI / libgit2 | libgit2 min–max ms | CLI min–max ms |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| S | 200 | 10.402 | 16.068 | 1.54 | 10.044–10.905 | 14.353–26.447 |
| M | 1,194 | 225.314 | 199.974 | 0.89 | 223.697–232.302 | 195.061–210.928 |
| L | 50,000 | 3,321.855 | 1,145.869 | 0.34 | 3,255.121–4,087.412 | 1,087.308–1,198.862 |

S では CLI が遅く、M で逆転し、L では CLI が 0.34 倍だった。従って少なくともこの APFS / Git 2.50.1 / git2 0.21.0 の clean status 系列では、規模増加に対する libgit2 側の傾きが CLI より急である。この条件では L が性能閾値 0.7 を満たすが、dirty status、process-cold、fsmonitor / untracked cache、別 filesystem、A2 の snapshot 情報量が未測定であり、production backend の選定結論にはしない。

## Raw JSON

| case | JSON |
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

## 次の測定
1. clean S / M / L の process-cold、続いて `core.fsmonitor` と `core.untrackedCache` の有効・無効条件。
2. dirty / rename / untracked の L case。
3. A2 の libgit2 `snapshot` と CLI 合成の段別 process / time / 情報欠落。
4. A3 の `1k` / `5k` / `20k` / `50k` fixture における傾き。
5. A0 の GUI watcher scenario。`KAGI_BENCH_READ=1` の raw event、debounce 後 tick、reload、`snapshot` / `working_tree_status` 時間を記録する。
