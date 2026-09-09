# #627 P2 — CLI 環境

> Status: 実行済み。E1–E3はmacOSで実行し、E2 UI deliveryはTier Aで観測した。
> Base: `main` `069c42f717b610ba246e354589451fdc98e52f9e`
> Integration revision: `f545afb2a44ef8d3ccf26365e630638bec2ac432`
> P2 probe commit: `68acd7dd4ef9aa5c8d3d2edfec6fbd62c98c8d91`
> Executed at: 2026-09-08T21:34:14Z
> Raw artifacts: `/tmp/kagi-627-p2.53lWT3/`（fixture、P0 envelope、hyperfine JSON、E2 JSON matrix、E3 marker/stdio、Git source build logs）

## 固定した判定

計画書 §5 E1–E3 の基準を実行前から変更していない。特に E2 の最低版上限は **Git 2.38** のままであり、実験結果で引き上げない。E2 が「CLI依存を広げてよい」と言えるのは、standaloneでmissing/oldを安定分類し、かつTier Aでmissing/old双方のcapability modal・oplog failure・repo不変を確認できた場合だけである。

## 環境

| 項目 | 値 |
| --- | --- |
| OS / arch | macOS 26.4.1 / aarch64 |
| filesystem | APFS |
| system Git | `git version 2.50.1 (Apple Git-155)` |
| libgit2 crate | `0.21.0` |
| `core.fsmonitor` / `core.untrackedCache` | `null` / `null` |
| index version | 2 |
| other-process load | `{ 12.00 9.80 6.52 }` |
| fixture | seed 627, 200 tracked files, 50 commits, depth 3, 51,200 bytes, max width 17, HEAD `840cf0866bd5aec654180074dcbeb5feac0323e7` |

他プロセス負荷は高く、E1にoutlier検出が出た。以下の数字はその環境での生観測であり、OS横断の採否根拠ではない。

## E1 — process startup floor

### 手順

S fixtureに対し、`hyperfine 1.20.0 --warmup 1 --runs 20 --export-json`を実行した。

```sh
hyperfine --warmup 1 --runs 20 \
  'git --version' \
  'git -C /tmp/kagi-627-p2.53lWT3/S --no-optional-locks status --porcelain=v2 -z --untracked-files=all'
```

### 結果

| command | runs | median | p95 | min–max |
| --- | ---: | ---: | ---: | ---: |
| `git --version` | 20 | 6.735 ms | 9.072 ms | 6.068–15.242 ms |
| S fixture `git status --porcelain=v2` | 20 | 11.370 ms | 13.321 ms | 10.511–15.143 ms |

- status / startup floor: median 1.688x、p95 1.468x。
- 付加実処理の差分: median 4.635 ms、p95 4.249 ms。

### 判定

**決定不能（CLIへ寄せない）**。E1の判定式には A0 の watcher p95 と候補の必要プロセス数が必要であり、P2範囲にA0はない。加えてmacOS一台だけであり、Windows Defender有効/無効とLinuxの測定が未実施である。これは「起動が十分安い」の証明ではない。

## E2 — Git minimum and absent/old behavior

### 一次資料と候補版

| command / option | primary source evidence | required version |
| --- | --- | ---: |
| `merge-tree --write-tree` | [Git 2.38 release note](https://raw.githubusercontent.com/git/git/v2.38.0/Documentation/RelNotes/2.38.0.txt): two-commit merge-tree mode。[2.38 manual](https://raw.githubusercontent.com/git/git/v2.38.0/Documentation/git-merge-tree.txt): `--write-tree` syntax | 2.38 |
| `for-each-ref %(ahead-behind:<committish>)` | [2.37 manual](https://raw.githubusercontent.com/git/git/v2.37.0/Documentation/git-for-each-ref.txt) にatomなし、[2.38 manual](https://raw.githubusercontent.com/git/git/v2.38.0/Documentation/git-for-each-ref.txt) にatomあり | 2.38 |
| `status --porcelain=v2` | [Git 2.11 manual](https://raw.githubusercontent.com/git/git/v2.11.0/Documentation/git-status.txt)がv2出力を記載 | ≤2.11 |
| `--no-optional-locks` | [Git 2.15 release note](https://raw.githubusercontent.com/git/git/v2.15.0/Documentation/RelNotes/2.15.0.txt)が導入を記載 | 2.15 |

最高要求は2.38であり、計画の上限を**維持**する。2.38超を要求する候補は採らない。

### standalone matrix

integration revisionで登録済み`backend_probe --operation cli-capability`を実行した。probeはP0のfresh-copy/fingerprint envelopeを使用する。`merge-tree --write-tree`は2.38以降でobject databaseを書き換えるため、`mutates_fixture = true`にし、各3 iterationをpristine copyで実行した。これはproductのversion gateではなく、選択したexecutableのprocess事実を記録する実験probeである。

| executable | 3 / 3 outcome | `merge-tree --write-tree HEAD HEAD` | fingerprint | panic / hang |
| --- | --- | --- | --- | --- |
| missing path | `git-unavailable` | spawn error | unchanged | none |
| Git 2.37.7 | `version-unsupported` | exit 128, `fatal: unknown rev --write-tree` | unchanged | none |
| Git 2.38.5 | `supported` | exit 0 | changed（object DB） | none |
| Apple Git 2.50.1 | `supported` | exit 0 | changed（object DB） | none |
| Git 2.55.0 | `supported` | exit 0 | changed（object DB） | none |

- Git 2.37.7 / 2.38.5 / 2.55.0はofficial `git/git` tagから`NO_GETTEXT=YesPlease NO_TCLTK=YesPlease`で`/tmp`にbuildした。2.37.7は2.37系列の最新タグであり、要求値2.38未満のold conditionを満たす。
- missing/oldのprocess分類はprobe内で安定したが、現行production `run_git`にはGit version parse、最低版判定、`GitError` variant、stable error codeのいずれもない。
- `merge-tree --write-tree`のobject DB書込みはP0 fingerprintで検出した。HEAD/index/worktreeだけを見るfingerprintでは足りない。

### Tier A delivery observation

integration ownerが`KAGI_GUI_E2E_ONLY=backend_cli_capability_observation`で、local remoteに対する既存`fetch`を観測した。これはcurrent behaviorを恒久仕様に固定するassertではない。公開APIでfooter・`pull_modal()`・oplog・fingerprintを記録し、panic/hangだけをassertした。

| condition | visible result | capability modal / structured code | oplog | repo fingerprint | panic / hang |
| --- | --- | --- | --- | --- | --- |
| PATHにgitなし | `Failed`: `failed to start git fetch --prune -- origin: No such file or directory (os error 2)` | なし。一般実行失敗 | fetch record 0件 | unchanged | none |
| `git --version`だけ2.37.0のshim | `Success`: `Fetched origin` | なし | fetch record 0件 | changed（remote ref update） | none |

missing PATHの不変fingerprintは、spawn failureがrepoを汚さない証拠である。一方2.37 shimは他の引数を実Gitへ`exec`するため、version gateがない既存fetchは通常成功した。どちらもP2が必要とするcapability modal・stable code・failure oplogを提供しない。

missing Gitのfetch failureがoplogに残らないのは、#627のcandidate採否とは別の既存delivery gapである。AGENTS.mdの「User-facing errors must surface via the oplog **and** a modal」に照らし、[issue #646](https://github.com/TomiXRM/kagi/issues/646)へ分離した。

### 判定

**No**。minimum 2.38のstandalone事実は得たが、現行に2.38 candidate gateがない。missing / old条件で必要なcapability deliveryを観測できず、CLI依存を広げる前提を満たさない。

## E3 — hardening surface

### 手順

1. `cargo test -p kagi --test cli_hardening_test`を実行: **5 passed**。
2. `merge-tree --write-tree` candidateに対する未カバー項目を実測した。`conflict.txt merge=evil` attributeとrepo-local `merge.evil.driver=/tmp/.../E3-merge-driver.sh`を持つdivergent branch fixtureを作り、driverはmarkerをtouchしてexit 1する。
3. bare Gitをpositive controlとして実行後、同一repo/branchesに`kagi_git::cli::run_git(..., ["merge-tree", "--write-tree", "left", "right"])`を通した。

### 結果

| execution | Git exit | marker |
| --- | ---: | --- |
| bare `git merge-tree --write-tree left right` | 1 | created |
| hardened `run_git` | status 1 | created |

`run_git`は`core.fsmonitor`、`core.hooksPath`、`core.askPass`、`protocol.allow`、repo-local `core.sshCommand`/`credential.helper`をhardeningするが、`merge.<name>.driver`をneutraliseしない。そのため新規candidate `merge-tree --write-tree`に対する未対応項目が少なくとも1件ある。

### 判定

**No**。E3基準は未対応0件であり、1件でもあればその操作をCLIへ寄せない。既存hardening testが通ることはこのcandidateのhardening合格を意味しない。P2はproduction CLI operationを追加していないため、P2の直接`Command`使用はselected executableを実験するprobeだけであり、product operationの`run_git`経路を増やしていない。

## P2 conclusion

| experiment | Yes / No | reason |
| --- | --- | --- |
| E1: startup cost permits a CLI read path | **No decision** | A0 p95・process count・3 OSがない |
| E2: a new 2.38-gated CLI candidate meets environment/delivery precondition | **No** | standalone capability factsは得たがproduction gateとTier A delivery合格がない |
| E3: existing hardening is sufficient for `merge-tree --write-tree` | **No** | repo-local custom merge driver executes through hardened `run_git` |

P2はCLI candidate採用を支持しない。特にE3がNoの時点で、E1の性能値は採否を覆せない。最低版2.38は実測により引き上げず維持する。

## 未検証 / 引継ぎ

- Linux / Windows E1/E2/E3。P6 E4が3 OS gateを集約する。Windows Defender有効/無効条件はP2 macOSでは観測不能。
- `merge-tree --write-tree`をCLI candidateに採るなら、custom merge driverを含むrepo-local configを実行しないhardening対策とregression testを先に実装する必要がある。これはstage 8であり、P2は先取りしない。
