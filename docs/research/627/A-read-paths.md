# 627 P4 — 読み経路の測定

> 状態: 実行中。A1 の最初の S fixture 確認まで完了。A0 / A2 / A3、S/M/L 本測定、backend 選定は未確定。
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

## Raw JSON

| case | JSON |
| --- | --- |
| S clean / libgit2 / 3 | `/tmp/kagi-627-p4/results/a1-S-libgit2-3.json` |
| S clean / CLI bare / 1 | `/tmp/kagi-627-p4/results/a1-S-cli-bare-1.json` |
| S clean / CLI `--no-optional-locks` / 3 | `/tmp/kagi-627-p4/results/a1-S-cli-no-optional-locks-3.json` |
| S dirty / libgit2 / 3 | `/tmp/kagi-627-p4/results/a1-S-status-libgit2-3.json` |
| S dirty / CLI `--no-optional-locks` / 3 | `/tmp/kagi-627-p4/results/a1-S-status-cli-3.json` |

## 次の測定

1. S / M / L に対する A1 の 11 iteration warm と process-cold。
2. `core.fsmonitor` と `core.untrackedCache` の有効・無効条件。
3. A2 の libgit2 `snapshot` と CLI 合成の段別 process / time / 情報欠落。
4. A3 の `1k` / `5k` / `20k` / `50k` fixture における傾き。
5. A0 の GUI watcher scenario。`KAGI_BENCH_READ=1` の raw event、debounce 後 tick、reload、`snapshot` / `working_tree_status` 時間を記録する。
