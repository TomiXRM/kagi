# #627 P1 — 予測 API

> Status: 実行済み（C1/C2）、C3/C4 は実行不能な前提を記録。
> Branch: `exp/627-p1-prediction`
> Base: `main`

## 結論

**Yes — 予測経路は libgit2 に残す。**

C2 で `git merge-tree --write-tree` は完全 fingerprint を変化させた。これは ODB も index も worktree も不変でなければならない C2 の絶対要件に反する。C1 の全用途について C2/C3/canonical JSON の三条件を満たした CLI 代替は確認できなかったため、速度を理由に CLI へ移さない。

## C1 — LSP inventory

### §2.1 との差異

`main` の LSP references を、production `crates/kagi-git/src` 内の call expression のみを 1 件として再取得した。§2.1 の列挙済み 8 API は 39 件で一致した。一方、同じ予測用途に `merge_file_from_index` が 2 件あり、§2.1 の API 表には含まれていなかった。

| API | production call expressions | §2.1 | 差 |
| --- | ---: | ---: | ---: |
| `merge_commits` | 7 | 7 | 0 |
| `merge_trees` | 2 | 2 | 0 |
| `cherrypick_commit` | 2 | 2 | 0 |
| `merge_file` | 2 | 2 | 0 |
| `merge_file_from_index` | 2 | — | +2 |
| `diff_tree_to_tree` | 12 | 12 | 0 |
| `diff_tree_to_index` | 8 | 8 | 0 |
| `diff_index_to_workdir` | 4 | 4 | 0 |
| `diff_tree_to_workdir` | 2 | 2 | 0 |
| **total** | **41** | **39** | **+2** |

差異の 2 call expression は次の通り。

| API | file:line | caller | Kagi 用途 | 必要な戻り値 |
| --- | --- | --- | --- | --- |
| `merge_file_from_index` | `crates/kagi-git/src/resolution.rs:778` | `merge_with_style` | conflict editor の zdiff3/standard marker materialization | merged content、automergeable、conflict marker style |
| `merge_file_from_index` | `crates/kagi-git/src/ops/pr_conflict.rs:130` | PR conflict preview helper | PR conflict preview | merged content、conflict state |

この差異を報告した後に C2 を開始した。C1 の CSV artifact は LSP references から API 別に作成し、`(api,file:line,caller)` の API 別・全体集合、CSV key 重複 0 を照合してから判定に使う。root dispatcher 登録はこの source inventory の前提ではない。

## C2 — complete fingerprint の実測

`crates/kagi-git/tests/prediction_side_effects_test.rs` は P0 の `fingerprint_repository` を用い、divergent two-branch repository で候補 command の前後を比較した。

| Candidate argv | 群 | 実測 | 判定 |
| --- | --- | --- | --- |
| `git merge-tree --write-tree main topic` | merge / tree merge | 前後 complete fingerprint が不一致 | **No** — prediction 不可 |
| `git cherry-pick --no-commit topic` | cherry-pick / revert | 前後 complete fingerprint が不一致 | **No** — prediction 不可 |
| `git merge-file -p ours ancestor theirs` | three-way content merge | 前後 complete fingerprint が一致 | C2 は Yes。ただし C1 schema / fixture 同値比較が未完了 |
| `git diff --no-ext-diff main topic` | tree↔tree | 前後 complete fingerprint が一致 | C2 は Yes |
| `git diff --no-ext-diff --cached` | tree↔index | 前後 complete fingerprint が一致 | C2 は Yes |
| `git diff --no-ext-diff` | index↔workdir | 前後 complete fingerprint が一致 | C2 は Yes |
| `git diff --no-ext-diff HEAD` | tree↔workdir | 前後 complete fingerprint が一致 | C2 は Yes |

実行: `cargo test -p kagi-git --test prediction_side_effects_test`。

## C3 — Kagi watcher

**未検証。** 計画が要求する `backend_prediction_watcher` GUI E2E scenario は存在せず、§0.1 は `tests/gui_e2e_runner.rs` の変更を integration owner 専有にしている。P1 は共有ハーネスおよび GUI runner を変更しないという依頼制約に従った。C2 で `merge-tree --write-tree` は既に不合格であり、watcher 結果でこれを覆せない。

## C4 — 恒久 enforcement

**未実装。** §5 C4 自体が「測定後の実装 PR」で追加する規則維持 test / CI baseline を規定している。P1 は backend 帰属を決定する実験であり、全 public `plan_*` / `preflight_*` entrypoint を対象にした恒久 table と CI baseline は、決定を集約する P6 または後続 implementation PR の所有物である。今回追加した C2 regression は `--write-tree` の ODB write を complete fingerprint が検出することだけを恒久化する。

## 判断基準

| 群 | C2 | C3 | canonical JSON | 判断 |
| --- | --- | --- | --- | --- |
| merge / tree merge | No | 未検証 | 未検証 | **libgit2 に残す** |
| cherry-pick / revert | No | 未検証 | 未検証 | **libgit2 に残す** |
| three-way content merge | Yes | 未検証 | 未検証 | **libgit2 に残す** |
| tree↔tree / tree↔index / index↔workdir / tree↔workdir diff | Yes | 未検証 | 未検証 | **libgit2 に残す** |

§5 の群判定は全 call expression の C2 non-write、C3 zero watcher event、canonical JSON zero missing/value mismatch が必要である。未検証は合格ではない。従って CLI 候補群は 0、libgit2 維持は Yes。

## 検証

- `cargo test -p kagi-git --test prediction_side_effects_test`
- `cargo fmt --all`
- workspace validation はこの report の commit 前に実行する。
