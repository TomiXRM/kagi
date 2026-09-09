# 627 P4 — 読み経路の測定

> 状態: 実行中。P1 review により A1 の libgit2 timer 境界を修正済み。旧 S/M/L performance 値は撤回し、reusable repository で再測定中。A0 / A2 / A3、process-cold、dirty L、backend 選定は未確定。
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

## 次の測定
1. reusable repository で clean S / M / L の 11 iteration warm と process-cold を再測定する。
2. dirty / rename / untracked の L case。
3. fsmonitor disabled / untracked cache enabled 条件を S / L に広げる。built-in fsmonitor は P0 fingerprint を変えたため、watcher candidate から除外する。
4. A2 の libgit2 `snapshot` と CLI 合成の段別 process / time / 情報欠落。
5. A3 の `1k` / `5k` / `20k` / `50k` fixture における傾き。
6. A0 の GUI watcher scenario。`KAGI_BENCH_READ=1` の raw event、debounce 後 tick、reload、`snapshot` / `working_tree_status` 時間を記録する。
