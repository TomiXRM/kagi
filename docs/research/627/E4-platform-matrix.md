# 627 P6 — platform matrix と backend 決定表

> 状態: macOS arm64 の P4 / P5 evidence を統合済み。Linux / Windows の headless probe と native GUI evidence は未取得であり、platform 採用 gate は未確定。
>
> branch: `exp/627-p6-platform-matrix`
>
> この文書は main に merge 済みの P4 PR [#664](https://github.com/TomiXRM/kagi/pull/664) の `docs/research/627/A-read-paths.md`、P5 execution / mixed report、owner が実施した `.gitattributes` filter と実機値を入力にする。

## 結論

- **既定 backend は libgit2。** repository config 由来 filter を構造的に実行しない安全性が、単一 host の速度比より重い。
- **CLI は利用者が明示選択する `CLI compatibility mode` だけにする。** repo size、cache、OS、測定値による暗黙の自動切替は採らない。
- **snapshot 全置換は不採用。** CLI composite は現在 4 情報を欠き、情報等価ではない。
- **局所最適化は backend 選択から独立させる。** pathspec status と index stat cache repair は「一部を賢く」する対象であり、既定 backend を変える根拠ではない。
- Linux / Windows の証拠がないため、CLI mode の既定昇格、OS 別規則、新しい backend 採用は**未確定**。不足時は現行の安全な既定を維持する。

## 数値の読み方と共通条件

P4 の formal A2 / A3 / cache / dirty L は、macOS arm64 の単一 host 上で次の条件を同じ場所に記録した。

```
copy → writable copy → timer 外 hardened plain status
（--no-optional-locks なしで index stat cache を prime）
→ 2 warm-up を破棄 → 30 measured
```

- timed CLI status は `--no-optional-locks`、libgit2 は open 済み repository handle を使う。
- 各値の `a / b` は `libgit2 → CLI / CLI → libgit2` の順序反転 series。p95 は各表に明記した nearest-rank 値。
- P4 formal series は timed fingerprint 不変を確認した。index prime は timer 外であり、read timing に write を混ぜない。
- owner 値は条件を併記した local measurement または 1 回の end-to-end 確認であり、P4 formal series の統計値と直接比較しない。
## 出典と条件 ID

- **[P4-A2]** main の `docs/research/627/A-read-paths.md`、`A2 — snapshot と CLI composite`。P0 index-prime、2 warm-up 除外、30 measured、両順序の formal series。
- **[P4-A3]** 同 report の `A3 — P0 index-prime series`。同じ P0 index-prime と両順序の 1k / 5k / 20k / 50k series。
- **[P4-cache]** 同 report の `untracked cache と dirty L`。L の `1 tracked modification + 100 untracked files`、canonical 101 entry、fingerprint stable、P0 formal series。
- **[P4-fsmonitor]** 同 report の `--git-executable` と fsmonitor 条件。M fixture の `core.fsmonitor=true` / `core.untrackedCache=true` candidate は 3 iteration を試行し、1 回目後の fingerprint 変更で 2 回目開始前の P0 check が失敗した。
- **[P5-D1]**, **[P5-D2]**, **[P5-F]** P5 `docs/research/627/D-execution-F-mixed.md` の各 section。D1 は macOS arm64 synthetic dirty tracked 2、D2 は macOS `fs_usage -w -f filesys`、F は S / 200 / 3 iteration。
- **[Owner-index]** `/tmp/kagi-627-p4/L` の新規 copy（tracked 50,000、286 MB）。`chmod -R u+w` 後に `find -type f -not -path '*/.git/*' -exec touch {} +` を実行し、内容を変えず stat のみ変更する。kagi 実機初回 load 前後の `.git/index` mtime と `git status --porcelain` 実時間を 1 回測定した end-to-end 確認。
- **[Owner-pathspec]** 同 fixture（tracked 50,000）の local libgit2 measurement。stale index は full `5,427 ms` / pathspec `0.8 ms`、warm index は full `145.3 ms` / pathspec `1.2 ms`。backend 統計でなく、index 状態を揃えた局所探索範囲の比較にだけ使う。
- **[Owner-filter]** libgit2 の `statuses` / `diff_index_to_workdir` / index `add_path` / `checkout_tree` を、filter が走ることを確認した bare `git add` と対照した 4 経路測定。安全性の確認であり、latency benchmark ではない。


## 証拠表

| 領域 | 出典 | 条件と数値 | 事実 | P6 での使い方 |
| --- | --- | --- | --- | --- |
| 安全性 / 予測 | [Owner-filter] | `statuses` / `diff_index_to_workdir` / index `add_path` / `checkout_tree` を、filter 実行確認済み bare `git add` と対照。 | libgit2 の 4 経路はいずれも `.gitattributes` filter を実行しない。予測を libgit2 に置けば repository config 由来コード実行を構造的に避けられる。 | **既定 libgit2 の主根拠。** 速度とトレードしない。 |
| A1 / A3 status | [P4-A3] | 1k / 5k / 20k / 50k、P0 index-prime、2 warm-up 除外、30 sample × 両順序。50k: libgit2 `141.971 / 132.672 ms`、CLI `93.361 / 91.143 ms`。 | 20k 以上では CLI が速い。1k / 5k は順序感度が大きい。 | crossover は事実として保持。**自動切替の根拠にはしない。** |
| A2 snapshot | [P4-A2] | S / M / L、P0 index-prime、2 warm-up 除外、30 sample × 両順序。libgit2 は pure `snapshot()`、CLI は 9 process composite。S: libgit2 `2.951 / 2.894 ms`、CLI `119.862 / 120.500 ms`。M: `19.065 / 24.926` 対 `142.311 / 140.867`。L: `200.777 / 173.259` 対 `344.556 / 242.053`。 | snapshot 全置換を除外。S 約41x、M 約6x、L は最小比でも約1.4xで libgit2 が速い。 |
| A2 情報等価性 | [P4-A2] | 同じ A2 composite | CLI は common-dir `FETCH_HEAD`、detached worktree root、annotated-tag peeling、linked-worktree WIP を返さない。4 項目は CLI 原理限界でなく、現 composite の未実装。 | 現在の全置換は不可。補完しても追加 process / complexity を負担する。 |
| A2 局所候補 | [P4-A2] | L、P0 index-prime、2 warm-up 除外、30 sample × 両順序。CLI 最重段 `status`: `177.334 / 120.859 ms`。 | libgit2 の private snapshot stage 内訳は未計測。 | explicit CLI mode で検討できる局所候補。production 計測 seam なしに他段へ一般化しない。 |
| pathspec status | [Owner-pathspec] | 同一 L fixture（tracked 50,000）。stale index: full `5,427 ms` / pathspec `0.8 ms`。**warm index: full `145.3 ms` / pathspec `1.2 ms`**。 | 局所最適化の比較は warm 同士の約121xだけを使う。stale-full と warm-pathspec を対比しない。 | 意味論を変えない局所最適化。backend mode と独立。 |
| index stat cache | [Owner-index] | L 新規 copy、tracked 50,000 / 286 MB、全 worktree file の stat のみを変更後、実機 kagi 初回 load と `git status --porcelain` を 1 回測定。#657 適用後、`4.25 s → 0.09 s`。 | **統計値でない end-to-end 確認。** stale index が status 遅延に寄与する。 | 「遅いから CLI」論拠を弱める。repair policy は read/write contract と watcher contention を満たす必要がある。 |
| untracked cache | [P4-cache] | L clean / dirty（tracked modification 1、untracked 100）、P0 index-prime、2 warm-up 除外、30 sample × 両順序。dirty cache off: libgit2 `145.399 / 138.237 ms`、CLI `98.398 / 95.595 ms`。on: `134.246 / 139.702`、`94.216 / 94.162`。 | 順序差 / tail を越える一貫した差はない。 | backend 選択・auto mode の条件にしない。設定値を観測表示することは可。 |
| fsmonitor | [P4-fsmonitor] | M fixture、`core.fsmonitor=true` / `core.untrackedCache=true` の CLI candidate を 3 iteration 試行。1 回目後に fingerprint が変わり、2 回目開始前の P0 check が失敗。 | libgit2 は fsmonitor を利用しない。CLI candidate は watcher read の非書込み契約を満たさない。 | libgit2 既定の性能 / 意味論判断に効果なし。CLI mode の watcher read には採らない。 |
| D1 | [P5-D1] | macOS arm64、synthetic、dirty tracked 2、1k / 5k / 20k / 50k。registered warm-index rerun は未完。 | historic private-driver values は stale / contract差を含み、採否に使わない。 | **未確定。** CLI 実行経路の採用根拠にしない。 |
| D2 | [P5-D2] | macOS `fs_usage -w -f filesys` は root 権限を要求。 | syscall 回数・read bytes は未測定。 | **未確定。** 原因診断の宿題。採否を単独で決めない。 |
| F mixed stash | [P5-F] | S / 200、synthetic、3 iteration。CLI が作成した stash に libgit2 `plan` / `run_recorded` を適用。 | 探索範囲の plan / post-state divergence は `0`。`f-apply` / `f-pop` / `f-drop` は verified。 | 「混在が安全」の証明ではない。未実施の conflict、large fixture、untracked restore 境界を残す。全 write の pipeline を弱めない。 |

## backend 決定表

| 対象 | 決定 | 根拠 | 禁止 / 保留 |
| --- | --- | --- | --- |
| 予測の既定 | **libgit2** | filter 非実行の構造的安全性。in-memory 予測を repository state を変えずに返す。 | 速度のために CLI へ移さない。3 OS watcher safety が無い CLI 予測は不採用。 |
| 読み取りの既定 | **libgit2** | A2 snapshot は全規模で libgit2 優位。status 単体の size crossover は composite の既定を覆さない。 | repo size / cache / host / OS による自動 backend 切替をしない。 |
| `CLI compatibility mode` | **明示選択のみ** | 利用者が mode を固定すれば、予測と実行前表示の挙動を説明できる。 | 暗黙の fallback / 閾値切替をしない。mode の既定化は 3 OS gate と情報等価性後。 |
| snapshot | **全置換しない** | 現 CLI composite は 9 process かつ 4 情報不足。 | CLI snapshot を既定にしない。private libgit2 helper を probe / production へ複製しない。 |
| snapshot 内 status | **局所候補としてのみ検討** | L CLI status は A2 の最重段。 | libgit2 stage 内訳が無いため、他段を「遅い」と断定しない。 |
| pathspec status | **局所最適化として優先** | 同一 L / 50k / warm index の owner 測定で full `145.3 ms` 対 pathspec `1.2 ms`（約121x）。 | backend mode と結び付けない。pathspec が意味を満たさない集約画面には適用しない。 |
| index stat cache | **repair/read policy を backend 選択から分離** | L 新規 copy / 50k / 286 MB、全 file stat 変更後の実機初回 load 1 回で `4.25 s → 0.09 s`。 | global read path に無条件 write を混ぜない。watcher 競合と ADR-0193 contract を守る。 |
| untracked cache / fsmonitor | **観測対象、選択条件ではない** | cache 差は一貫しない。fsmonitor は libgit2 に効果がなく、CLI candidate は fingerprint を変えた。 | cache / fsmonitor を hidden backend switch にしない。 |
| mixed write family | **operation ごとに verify を維持** | F は探索範囲で 0 件だが未実施境界がある。 | `plan → confirm → preflight → execute → verify → oplog` を省略・弱化しない。 |

## platform matrix

| gate | macOS arm64 | Linux | Windows | P6 判定 |
| --- | --- | --- | --- | --- |
| P4 A1 / A2 / A3 performance | 取得済み。単一 host。 | 未取得 | 未取得 | 新 backend 採用の証拠として未確定。既定 libgit2 を維持。 |
| P4 canonical semantics / cache / dirty L | macOS fixture で取得済み | 未取得 | 未取得 | OS 別規則を作らない。 |
| P5 D1 registered warm-index | 未完 | 未取得 | 未取得 | 未確定。 |
| P5 D2 syscall / bytes | root 権限不足 | 未取得 | 未取得 | 未確定。原因診断のみ。 |
| P5 F mixed boundary | S basic condition 3 repeat は取得済み | 未取得 | 未取得 | 未確定範囲あり。pipeline 維持。 |
| A0 / C3 watcher | macOS GUI / watcher evidence 未完 | headless probe 未取得 | headless probe 未取得 | CLI を予測 backend に採用しない。 |
| native GUI | macOS のみ可能 | runner 不可 | runner 不可 | OS 非依存の採否根拠にしない。 |

E4 の採用 gate は plan §E4 に従う。新しい backend を operation に採るには、関連する意味論・安全性・性能を **macOS / Linux / Windows の全て**で確認する。3 OS が揃わない場合、その operation は現行 backend に据え置く。これは既存 CLI operation を差し戻す理由ではない。

## 未確定事項と次の証拠

1. Linux / Windows で A1–A3、A2 information gaps、F boundary を同一 fixture / index-prime / order-reversal contract で取得する。
2. production `snapshot()` に、private helper を露出しない stage timing seam を設計する。A2 の局所候補を他段へ一般化する前提である。
3. D2 は root 権限または root 不要の代替診断で syscall / bytes を記録する。
4. F は conflict、large fixture、untracked restore の boundary を追加する。0 divergence を安全一般化しない。
5. CLI snapshot composite の 4 gap を埋めるかは、全置換を再開するためでなく、explicit mode の情報表示 contract を満たす必要が生じた時に判断する。
