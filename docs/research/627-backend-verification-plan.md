# 627 — Git backend 選定の検証要件書

> Status: 計画（実験未実行・決定未確定）。測定値の欄はすべて空欄。
> Last updated: 2026-09-09

Kagi の git backend を「どの操作を libgit2 に残し、どの操作を Git CLI に寄せるか」の
規則として確定させるための、実行可能な粒度の実験計画。**この PR は計画のみで、
実験の実行・結果・決定を含まない。**

先行調査は [#627](https://github.com/TomiXRM/kagi/issues/627)。本書はその上に
「何を測れば規則が決まるか」だけを足す。

## 0. 進行段と本 PR の範囲

| 段 | 内容 | 本 PR | 成果物 |
| --- | --- | --- | --- |
| 1 | 理解（何を決めるのか） | 済 | §1 |
| 2 | 先行調査（既知の事実） | 済 | §2 |
| 3 | 仮説（反証可能な形に） | 済 | §3 |
| 4 | 設計（実験の定義） | 済 | §4・§5 |
| 5 | 実験（実行） | **未** | §6 の空欄 |
| 6 | 評価（判断基準への当てはめ） | **未**（基準は §5 で先に固定） | §6 |
| 7 | 決定（操作ごとの規則） | **未** | §7 の空欄表 |
| 8 | 実装 | **未** | 決定後に別 PR |
| 9 | 検証 | **未**（手順は §8 に確定済み） | §8 |
| 10 | 知識化 | **未** | §9 |

段 6 の判断基準を段 5 より先に固定するのが本書の主目的である。測ってから基準を
決めると、どちらの backend にも寄せられる数字が出たときに議論が終わらない。

本文は依頼項目に合わせて A→F の順に並べる。論理依存は
**C・E → B → A・D → F → E4** とし、実行 wave は §0.1 の単位に固定する。
C で予測経路の候補を、E で CLI 候補の版・hardening 前提を先に確定し、B の意味論を
確認してから速度を採用根拠にする。C で予測を libgit2 に残すと確定した場合も、A は
読み経路を CLI に寄せるか、libgit2 のまま最適化するかを決める独立材料なので省略しない。

### 0.1 実行 PR と subagent の分割

実験段は **7 個の work package** に分ける。共有ハーネスを先に固定し、その後も依存を
満たす wave だけを並列化する。各 package は `docs/research/627/` に固有 report を持ち、
他 package の report と source module を編集しない。

| package | report | 担当 | 前提 / wave |
| --- | --- | --- | --- |
| P0 共通ハーネス | `00-harness.md` | fixture manifest、pristine copy、canonical JSON、fingerprint、probe の共通 CLI | なし。**最初に 1 本だけ** |
| P1 予測 | `C-prediction.md` | C1–C4、予測候補、非書き込み、watcher | P0 後。P2 と並列可 |
| P2 CLI 環境 | `E-cli-environment.md` | E1–E3、git 最低版、hardening | P0 後。P1 と並列可 |
| P3 意味論 | `B-semantics.md` | B1–B8、recovery handle | P1・P2 後 |
| P4 読み経路 | `A-read-paths.md` | A0–A3 | P3 後。P5 と並列可 |
| P5 実行・混在 | `D-execution-F-mixed.md` | D1–D2 と F。stash fixture の owner を 1 人に固定 | P3 後。P4 と並列可 |
| P6 platform matrix | `E4-platform-matrix.md` | E4、3 OS の証拠集約、候補ごとの採否 | P4・P5 後。最後に 1 本 |

実行 wave は **P0 → (P1 ∥ P2) → P3 → (P4 ∥ P5) → P6**。C と E を通す前に
B の CLI 候補を確定せず、B の意味論に合格する前に A / D の速度を採用根拠にしない。
F は C・B・D の結果を使うため P5 の末尾で行う。これが本文の論理順
`C → E → B → A → D → F` を、依存を壊さず並列化できる最小の分割である。

source ownership は次で固定する:

- P0 owner だけが `backend_probe.rs` / `backend_fixture.rs` の root dispatcher、共通 schema、
  共通 fixture / copy / fingerprint 実装を変更する。各実験 owner は
  `backend_probe/{a,b,c,d_f,e}.rs`、`backend_fixture/{a,b,c,d_f,e}.rs` の自分の module と
  自分の report だけを持つ。
- dispatcher への module 登録は各 wave 後に integration owner が直列で行う。実験 owner
  が同じ root file を並行編集しない。`tests/gui_e2e_runner.rs` と workflow も
  integration owner が単独で統合する。GUI scenario、verification flag、runner feature
  を追加した場合は同じ owner が `.claude/skills/verify/SKILL.md` も更新する。
- `docs/research/627-backend-verification-plan.md` の §6・§7、ADR、PR 本文は integration
  owner だけが更新する。subagent は共有結果表を直接編集せず、自分の report に raw JSON
  artifact の場所、commit、環境メタ、判定を書く。
- P0 の schema / fixture manifest を変える必要が出た場合は downstream で独自拡張せず、
  P0 owner に戻して版を上げてから全 package を同じ版へ揃える。

7 未満にまとめると共通ハーネスまたは stash / platform の ownership が競合し、7 より
細かくすると B の 8 scenario と C の API 群で schema 調整 PR が増える。並列実行は
最大 2 package に留めるのが安全である。

## 1. 理解 — 決めたいこと

決めるのは **操作ごとの backend 帰属規則**。単一 backend への全面移行は候補ではない
（§2-1・§2-4）。

経路を 3 つに分けて呼ぶ。規則はこの区分ごとに違ってよい。

| 区分 | 定義 | 代表 | 絶対要件 |
| --- | --- | --- | --- |
| 読み経路 | watcher 発火ごとに走る読み取り | `working_tree_status`（`crates/kagi-git/src/status.rs:51`）、`snapshot`（`crates/kagi-git/src/snapshot.rs:87`） | 体感を落とさない |
| 予測経路 | `plan_X` / `preflight_X`。結果をデータで受け取る | merge / cherry-pick / stash pop の衝突予測 | **作業ツリー・index・object のいずれにも書かない** |
| 実行経路 | `execute_X`。書くのが仕事 | rebase、pull、push、stash push、switch | git 本体と同じ意味論で書く |

予測経路の「書かない」は性能要件ではなく正しさの要件である。#625 で、plan が書くと
watcher が発火し、reload がその plan の埋めるはずのモーダルを消す事故を実際に踏んだ。

## 2. 先行調査 — 再調査しない既知の事実

1. **libgit2 を使う唯一の理由は予測**。作業ツリーにも index にも object database にも
   書かずに結果をデータとして受け取れること。確定前の予測がこれに依存する。現行の
   呼び出し式の inventory は、文字列出現数ではなく LSP references で採った §2.1 を
   正とする。
2. **`git merge-tree --write-tree` は object を書く**。git 2.50.1 実測で作業ツリーと
   index は無変更、object が 2 個増える。#625 の事故があるため、予測経路が書くことは
   許容できない。`git cherry-pick -n` は index と作業ツリーを変更するので予測ではない。
3. **速度問題の原因は特定済み**。`stash_save2` が未変更の tracked ファイルを読み直して
   いた。#622/#623 で stash push だけを CLI に移して 12.5 s → 0.93 s。libgit2 が一般に
   遅いのではなく、特定 API が特定の仕事をしていた。
4. **すでに両方使っている**。#627 調査時点で `run_git` を呼ぶ実装ファイルは
   fetch / pull / push / rebase / stash_push / switch / tag / worktree_lifecycle /
   file_history / hotspot / force_lease / conflicts / branch_cleanup / remote_branch / cli の
   15 個。ADR-0131 は sequencer continue API が libgit2 に無いことを CLI 採用理由と
   して記録している。
5. **git の同梱は採らない方針**。同梱しても OS 3 つ分の差異を自分が持つだけで差異は
   消えない。Kagi の git 版 check / bundled Git の現状は**未確認**であり、CLI 依存を
   広げる前に実験 E2 で実際の振る舞いを確定する。`merge-tree --write-tree` は git 2.38
   以降なので、CLI 依存を広げるなら最低版要求が必要になる。
6. **`run_git` の hardening（ADR-0146）は repo-local config がコマンドを実行させ得る
   ために存在する**。同梱してもこの危険は消えない。開いた repo の設定は攻撃者の管理下
   のままである。

### 2.1 本書のために取った repo 実測（2026-09-09 / `main` = `491f3de7`）

- `crates/kagi-git/src` 配下の production code を対象に、各 git2 API の定義へ解決される
  LSP references を取得し、構文上の call expression / method call expression だけを
  1 呼び出し 1 件として数えた。レシーバが `repo` / `self.repo` / 一時値のどれか、
  1 行か複数行かは問わない。
- comment・doc comment・文字列リテラル、wrapper の定義名、re-export、test helper と
  `#[cfg(test)]` 内の test-only call、`examples/`・`tests/`・依存 crate 内の reference
  は除外する。wrapper 本体が対象 API を呼ぶ場合、その内側の call expression 自体は
  含める。この定義での実測は次の **39 呼び出し式**:

  | API | LSP references から数えた呼び出し式 |
  | --- | ---: |
  | `merge_commits` | 7 |
  | `merge_trees` | 2 |
  | `cherrypick_commit` | 2 |
  | `merge_file` | 2 |
  | `diff_tree_to_tree` | 12 |
  | `diff_tree_to_index` | 8 |
  | `diff_index_to_workdir` | 4 |
  | `diff_tree_to_workdir` | 2 |

  この値は `main` の指定 commit に対する事前 inventory であり、C1 の固定期待値ではない。
  C1 実行時は対象 commit で同じ定義の LSP references を取り直す。
- **既知の意味論差異がコードに書かれている**: `crates/kagi-domain/src/plan_note/merge.rs:61-63`
  は「real git は unrelated histories を `--allow-unrelated-histories` なしに拒否するが、
  libgit2 の `merge_commits` は空の base に対して黙って merge する」と記録し、その差異が
  `UnrelatedHistories` blocker の根拠になっている。差異は仮説ではなく、少なくとも 1 件は
  既に製品仕様に織り込まれている。
- **既存の非書き込み検査は弱い**: GUI E2E の `repo_fingerprint`
  （`tests/gui_e2e_runner.rs:450`）は `git rev-parse HEAD` と porcelain status の 2 値。
  `merge-tree --write-tree` は HEAD も status も変えず object だけ増やすので、この
  fingerprint では検出できない。実験 C2 は fingerprint の拡張から始める。
- **測定の前例がある**: `tests/perf/oplog_detail.rs` は warm-up 1 回を破棄して 7 回の
  median を取り、上限定数と突き合わせ、前後で `repo_fingerprint` を比較する。本書の
  反復・判定の作法はこれに合わせる。`crates/kagi-git/examples/conflict_timing.rs` が
  既にあるので、libgit2 側の計測は新しい bench framework を入れずに example で足りる。
- 手元の git は 2.50.1（Apple Git-155）。最低版の議論は実験 E2。

### 2.2 現行実装への照合（2026-09-09）

本書で「現行」「既存」と呼ぶ Kagi 実装を全て照合した。§2.1 の 39 呼び出し式と
`run_git` 15 file は `main = 491f3de7` に固定した**履歴上の観測値**であり、現在値とは
主張しない。実験開始時に C1 / D1 の inventory を取り直す。

| 本書の現行実装に関する記述 | 照合結果 | 根拠 |
| --- | --- | --- |
| `working_tree_status` の option | `include_ignored(false)`、untracked 再帰、HEAD→index rename 検出 | [`status.rs:51-56`](../../crates/kagi-git/src/status.rs#L51-L56) |
| untracked の定義 | `WT_NEW` でも nested repo / linked worktree（`.git` directory **又は file**）は除外 | [`status.rs:150-163`](../../crates/kagi-git/src/status.rs#L150-L163)、[`status.rs:206-213`](../../crates/kagi-git/src/status.rs#L206-L213) |
| `snapshot` の構成 | head / status / worktrees / commits / refs / stashes / `FETCH_HEAD` mtime を読む | [`snapshot.rs:87-127`](../../crates/kagi-git/src/snapshot.rs#L87-L127) |
| linked worktree の layout | worktree root の `.git` は file、private gitdir は `repo.path()`、ODB / shared refs は `repo.commondir()` | [`status.rs:206-213`](../../crates/kagi-git/src/status.rs#L206-L213)、[`backend.rs:42-57`](../../crates/kagi-git/src/backend.rs#L42-L57)、[`snapshot.rs:134-142`](../../crates/kagi-git/src/snapshot.rs#L134-L142) |
| existing `repo_fingerprint` | HEAD と porcelain status の 2 値だけ | [`gui_e2e_runner.rs:449-464`](../../tests/gui_e2e_runner.rs#L449-L464) |
| GUI perf precedent | warm-up 1 回を破棄して 7 回 median、前後 fingerprint 比較 | [`oplog_detail.rs:46-102`](../../tests/perf/oplog_detail.rs#L46-L102) |
| unrelated histories blocker | real Git は拒否、libgit2 `merge_commits` は空 base merge | [`merge.rs:60-63`](../../crates/kagi-domain/src/plan_note/merge.rs#L60-L63) |
| stash family の現行混在 | push は `run_git`、apply / pop / drop は libgit2 | [`stash_push.rs:340-378`](../../crates/kagi-git/src/ops/stash_push.rs#L340-L378)、[`stash.rs:153-168`](../../crates/kagi-git/src/ops/stash.rs#L153-L168)、[`stash.rs:356-399`](../../crates/kagi-git/src/ops/stash.rs#L356-L399)、[`stash.rs:593-613`](../../crates/kagi-git/src/ops/stash.rs#L593-L613) |
| CLI executable seam / hardening | PATH の `git` を起動し、`run_git` は hardening args / local override を prepend する | [`cli.rs:231-242`](../../crates/kagi-git/src/cli.rs#L231-L242)、[`cli.rs:259-294`](../../crates/kagi-git/src/cli.rs#L259-L294) |
| watcher / platform CI | `start_git_watcher` は `notify` recursive watcher、`DEBOUNCE` は 500 ms。CI は macOS test blocking、Linux test / Windows build advisory | [`watcher.rs:149-211`](../../src/ui/watcher.rs#L149-L211)、[`.github/workflows/ci.yml:105-168`](../../.github/workflows/ci.yml#L105-L168) |
| 予測 API | merge / cherry-pick / content merge / diff は in-memory libgit2 API を呼ぶ | [`ops/merge.rs:200-204`](../../crates/kagi-git/src/ops/merge.rs#L200-L204)、[`ops/cherry_revert.rs:209-215`](../../crates/kagi-git/src/ops/cherry_revert.rs#L209-L215)、[`resolution.rs:777-780`](../../crates/kagi-git/src/resolution.rs#L777-L780)、[`diff.rs:87-89`](../../crates/kagi-git/src/diff.rs#L87-L89) |
| fetch / pull / push | fetch と push は CLI。pull は CLI fetch の後に libgit2 graph / checkout を行う混在 | [`fetch.rs:40-54`](../../crates/kagi-git/src/ops/fetch.rs#L40-L54)、[`pull.rs:340-407`](../../crates/kagi-git/src/ops/pull.rs#L340-L407)、[`push.rs:384-397`](../../crates/kagi-git/src/ops/push.rs#L384-L397) |
| rebase / switch / checkout / discard | rebase は CLI。switch は remote fetch で CLI を使い checkout は libgit2。checkout / discard は libgit2 | [`rebase.rs:121-132`](../../crates/kagi-git/src/ops/rebase.rs#L121-L132)、[`switch.rs:140-145`](../../crates/kagi-git/src/ops/switch.rs#L140-L145)、[`switch.rs:342-344`](../../crates/kagi-git/src/ops/switch.rs#L342-L344)、[`checkout.rs:235-286`](../../crates/kagi-git/src/ops/checkout.rs#L235-L286)、[`discard.rs:232-253`](../../crates/kagi-git/src/ops/discard.rs#L232-L253) |

この表に無い将来実装（`backend_fixture` / `backend_probe` / C4 test / headless watcher
probe / GUI scenario）は**未確認**であり、P0–P6 の実装 PR で初めて確認する対象である。

## 3. 仮説

反証可能な形で 4 つ立てる。**どの実験がどの操作群の仮説を反証しうるか**を明記する。
判定は操作群ごとに行い、ある群の CLI 代替が成立しても他の群へ一般化しない。

| # | 仮説 | 反証条件 | 反証しうる実験 |
| --- | --- | --- | --- |
| H1 | 各予測経路は libgit2 に残す。CLI に「書かない等価手段」が無いため | 当該用途の全呼び出しで、object を増やさず index も worktree も変えず、同じ情報を返す CLI 手段が成立する | C1 + C2 + C3 |
| H2 | 実行経路は、git 本体との意味論差または規模依存の遅延が製品挙動を壊す操作だけ CLI に寄せる | 当該操作で B の差異が 0 件かつ D の傾きが据え置き閾値内、または E の CLI コストが不合格 | B + D + E |
| H3 | 読み経路の libgit2 は据え置ける。#622 の原因は特定 API だった | L 規模で A1 / A2 の CLI 候補が移行閾値を超える | A0 + A1 + A2 + A3 |
| H4 | `stash_save2` 型（未変更ファイルを読み直す）API は他にもある | D1 の傾き測定で該当 0 件 | D1 + D2 |

H1 と H2 の判定結果により、同じ family 内で「plan は libgit2、execute は CLI」に
分かれ得る。その混在を許す条件は実験 F で決める。

## 4. 共通測定条件

これを固定しないと、実験間で数字が比較できない。

### 4.1 fixture

| 名 | 定義 | 用途 |
| --- | --- | --- |
| S | 合成。200 files / 3 階層 / 50 commits | 下限とプロセス起動コストの可視化 |
| M | Kagi 自身（tracked file 数と commit 数は測定時に manifest に記録） | 実運用相当 |
| L | 合成。50,000 files / 深さ 6 / 2,000 commits / 1 MiB blob 20 個 | 傾きと最悪値 |
| X | 任意。大規模 OSS の clone | 実物の分布での sanity。必須ではない |

生成器の契約（実装は測定 PR、本 PR では作らない）: seed 固定で決定的、ネットワーク
不要、生成後に manifest（tracked files / commits / dirs / bytes / 最大ディレクトリ幅）
を出力する。置き場所は `scripts/bench/` を提案する。

### 4.2 反復と cold/warm

- **warm**: 1 プロセスで同じ open 済み repository に対して warm-up 1 回を破棄し、
  10 回測る。median と p95 を記録する（`tests/perf/oplog_detail.rs` の作法）。
- **process-cold**: 毎回プロセスを起動し、repository を open して 1 回だけ測る。
  5 回の median を記録する。CLI の実態は常にこの条件なので、libgit2 にも同条件を作る。
- **filesystem-cold**: OS page cache の排除は OS ごとに権限と手段が違い、同じ意味に
  揃えられない。backend 判定値には使わず、実施できた OS の補助値としてだけ別欄に残す。
  「fixture の複製直後」は複製処理が cache を温めるので cold と呼ばない。
- 1 回だけの数字は結果として記録しない。#627 の 8 s / 300 ms がその形で、条件が
  分からないため一般化できなかった。

### 4.3 pristine copy 契約（fixture を変更する全実験に適用）

**これは P0 共通ハーネスの契約であり、特定実験の特例ではない。** B8 だけの規定に
していた版では D1 が同じ repo に書き込み操作を 11 回反復し、`stash_drop` は初回で
stash を消し、stage と checkout は初回後は適用済みになり、backend の実行順で結果が
偏っていた。2 回目以降は対象操作を測っていない。

- `backend_fixture` が作る fixture は **読み取り専用の pristine template** とする。
  `backend_probe` は template を直接開かない。
- **測定単位ごと**に、同一 manifest から byte-for-byte 同一の独立 copy を作り、その
  copy だけを変更して破棄する。hardlink / reflink による mutable file の共有は禁止する。
- 測定単位は fixture を変更するかで決める。
  - **変更する操作**: backend ごと、かつ反復ごとに 1 copy。11 反復なら backend ごとに
    11 copy。C2 / C3 のように「書いたか」を判定する操作も、候補・反復ごとに新しい
    copy を使い、前後 fingerprint を比較する。
  - **変更しない操作**: 1 backend の 1 系列 = 1 copy。warm（§4.2）は「同じ open 済み
    repository を反復する」ことが定義なので、反復ごとに copy を作ると warm 条件自体が
    消える。
- copy の作成と破棄は計時の外側に置き、各回の wall / user / sys に含めない。
- fixture を変更する操作の warm は **process-warm** と読み替える。プロセスは維持するが
  repository は copy ごとに open し直すため libgit2 の object cache は毎回冷える。両
  backend が同条件なので比較は成立するが、この読み替えを §4.5 の環境メタに明記する。
- 全 backend・全測定単位が同じ manifest と同じ事前 fingerprint（C2 の拡張 fingerprint）
  から始まったことを probe が記録する。**一致しない測定は無効**とし §6 に載せない。
- backend の実行順が結果を動かしてはならない。契約が効いていることの実証として、
  fixture を変更する各実験は `libgit2 → cli` と `cli → libgit2` の両順で 1 系列ずつ
  実行し、判定に使う統計量が閾値の範囲で一致することを確認する。一致しなければ
  copy 契約の実装不良として、数字を採用せず P0 に差し戻す。

どの実験が fixture を変更するかを漏れなく列挙する。新しい実験を足すときはこの表に
行を足すまで実行しない。

| 実験 | fixture を変更するか | pristine の単位 | 変更の中身 |
| --- | --- | --- | --- |
| A0 | する | fixture × case × 反復ごと | 保存 120 回、branch 往復、pull |
| A1 | しない | backend ごとの 1 系列 | 読みのみ。index stat cache の書き戻しが出たら C2 の対象に回す |
| A2 | しない | backend ごとの 1 系列 | 読みのみ |
| A3 | しない | 規模 × backend ごとの 1 系列 | 読みのみ |
| B1 | する | 反復ごと（3 回 = 3 copy） | filter 経由の stage / discard |
| B2 | する | 反復ごと | 改行変換後の stage |
| B3 | しない | scenario ごと | 一覧取得のみ。discard を測る場合は反復ごとに変更 |
| B4 | しない | scenario ごと | status / diff のみ |
| B5 | しない | scenario ごと | status のみ。discard を含める場合は反復ごとに変更 |
| B6 | する可能性 | 反復ごと | lazy fetch が object を増やし得る |
| B7 | する | 反復ごと | CLI 側 `merge-tree --write-tree` が object を書く（§2-2） |
| B8 | する | 操作 × backend × 反復ごと | stash push / drop（通常条件）、stash apply / pop（clean / conflict）、file backup |
| C1 | しない | repo を使わない | source inventory のみ |
| C2 | 変更の有無が判定対象 | 候補 × 反復ごと（3 回 = 3 copy） | 各 copy の前後比較で「初回だけ書く」を含む全書き込みを検出する |
| C3 | 変更の有無が判定対象 | 候補 × 実行ごと | watcher 観測前後の fingerprint を比較する |
| D1 | する | **反復ごと（11 反復 = 11 copy）** | stash apply / pop / drop、staging の index 書き込み、checkout、discard |
| D2 | する | trace 1 回ごと | D1 と同じ操作を 1 反復ずつ |
| E1 | しない | S fixture 1 個 | `git --version` と status |
| E2 | しない | repo 非依存。UI scenario は自前 fixture | capability probe |
| E3 | する | test ごと | 悪性 config fixture。cargo test が管理 |
| E4 | 各実験に従う | 各実験の単位 | 3 OS で同じ契約を適用する |
| F | する | 条件 × 反復ごと（3 回 = 3 copy） | mixed-stash の plan と execute |

### 4.4 ハーネスと共通コマンド

測定 PR は次の 2 example を実装する。本書のコマンドはその CLI 契約を固定するもので、
本 PR では example を作らない。

| example | 契約 |
| --- | --- |
| `backend_fixture` | `--out`、`--files`、`--commits`、`--depth`、`--seed` または `--scenario` を受け、fixture と JSON manifest を作る |
| `backend_probe` | `--repo`、`--operation`、`--backend libgit2|cli|mixed`、`--candidate`、`--git-executable`、`--iterations`、`--format json` を受け、各回の wall / user / sys 時間、canonical JSON、前後 fingerprint を出す |

生成と build:

```sh
cargo run -p kagi-git --release --example backend_fixture -- \
  --out "$FIXTURE_ROOT/S" --files 200 --commits 50 --depth 3 --seed 627
cargo run -p kagi-git --release --example backend_fixture -- \
  --out "$FIXTURE_ROOT/L" --files 50000 --commits 2000 --depth 6 --seed 627
cargo build -p kagi-git --release --example backend_probe
```

warm 比較（A1 の例。probe 内の parse と canonical 化を含む）:

```sh
./target/release/examples/backend_probe \
  --repo "$FIXTURE" --operation working-tree-status --backend libgit2 \
  --iterations 11 --format json
./target/release/examples/backend_probe \
  --repo "$FIXTURE" --operation working-tree-status --backend cli \
  --iterations 11 --format json
```

process-cold と外側から見たプロセス起動込みの比較:

```sh
hyperfine --warmup 1 --runs 10 \
  './target/release/examples/backend_probe --repo "$FIXTURE" --operation working-tree-status --backend libgit2 --iterations 1 --format json' \
  './target/release/examples/backend_probe --repo "$FIXTURE" --operation working-tree-status --backend cli --iterations 1 --format json'
```

アプリ体感は既存 GUI E2E の perf scenario と同じ作法で測る。実行は必ず
`KAGI_GUI_E2E_ONLY` で絞る（§8）。読み経路の内訳は、測定 PR で
`KAGI_BENCH_READ=1` のときだけ出る診断出力を足す。**既存の `[kagi]` 契約行は
変更・追加しない**（AGENTS.md の logging rules）。

### 4.5 結果に必ず添える環境メタ

OS と版 / arch / `git --version` / `git2` crate 版 / ファイルシステム /
`core.fsmonitor` と `core.untrackedCache` の値 / index 版 / fixture manifest /
測定中の他プロセス負荷。これが無い数字は §6 に載せない。

### 4.6 全実験に共通する交絡

- **fsmonitor / untracked cache は CLI 側にしか効かない**。有効・無効の両条件で測る。
  片方だけの数字で backend を決めてはいけない。
- **git が読み取りでも index を書き戻し得る**。`--no-optional-locks` はそのために存在
  する。読み経路・予測経路の CLI 候補は、この抑止が必要かを C2 で必ず確認する。
- OS（macOS / Linux / Windows）、git 版、リポジトリ規模、ファイルシステム
  （APFS / ext4 / NTFS、ネットワーク FS は対象外と明記）。
- 同一マシンで cargo build が走っていないこと（AGENTS.md の build hygiene）。

### 4.7 閾値の根拠

- 50 / 150 / 200 ms: watcher 1 回の低影響 / 移行を正当化する絶対差 / 優先対応境界。
- 100 / 200 ms/s: 1 秒 bucket あたりの累積 read 時間の低影響 / 優先対応境界。
- 500 / 1,000 ms/60 s: 1 分あたりの累積 read 時間の低影響 / 優先対応境界。
- 1.5 倍: OS と cache のばらつきだけで移行しないための相対差。
- 0 件 / 0 writes / 0 watcher events: 意味論と予測の安全性は許容誤差を置けない。
- git 2.38: 既知候補 `merge-tree --write-tree` の導入版。2026-09-09 時点の採用上限を
  ここに固定し、これより新しい Git だけで成立する規則は採らない。実験 E2 の結果で
  この値自体を後から引き上げない。

これらは測定結果ではなく、段 6 で結果を分類するための product threshold である。

## 5. 実験

各実験は **問い / 手順 / 測る値 / 判断基準 / 交絡 / この実験では決まらないこと** を持つ。
判断基準は測定前に固定してある。決まらない実験は実行しない。

### A. 常時走る読み経路の速度

#### A0 — watcher の発火頻度・累積コスト・1 tick コスト

- **問い**: 現実的な操作で watcher は何回発火し、読み経路の単位時間あたりの累積
  コストは backend 選定を動かす規模か。
- **手順**: 測定 PR の GUI E2E scenario `backend_read_watcher` に次の 3 case を作る。
  M / L × case ごとに 5 回実行する。**各回の fixture lifecycle は §4.3 に従う。**
  アプリ再起動は case 間の UI state 隔離だけに使い、repo reset の代替にしない。
  1. **連続保存**: 同じ tracked file を 500 ms 間隔で 60 s、計 120 回保存する。
  2. **ブランチ切り替え**: 1,000 path の内容が異なる 2 branch を 1 s 間隔で 10 往復し、
     各切り替え後に reload 完了を待つ。
  3. **大きな pull**: 同一ディスク上の bare remote に 5,000 path を変える 1 commit を
     用意して 1 回 pull し、開始から完了 10 s 後まで観測する。

  `KAGI_BENCH_READ=1` の診断出力で raw filesystem event、debounce 後の watcher tick、
  reload 回数、tick ごとの `snapshot` / `working_tree_status` 時間を記録する:

  ```sh
  KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY='backend_read_watcher' \
    cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
  ```

  この native GUI scenario は macOS 専用。A0 の累積値を採用根拠に使う場合は、E4 の
  Linux / Windows headless watcher probe でも同じ workload の event / tick を測る。

- **値**: raw event 数、watcher tick 数、reload 数（回 / case、回 / s）、1 tick の
  ms（median / p95）、最も重い 1 s bucket の累積 read ms、60 s 換算の累積 read ms。
- **判断基準**:
  - 第 1 段（1 tick）: p95 < 50 ms は低影響、p95 ≥ 200 ms は優先対応。
  - 第 2 段（頻度込み）: 最重 1 s < 100 ms **かつ**累積 < 500 ms/60 s は低影響。
    最重 1 s ≥ 200 ms **または**累積 ≥ 1,000 ms/60 s は優先対応。
  - 2 段のどちらかが優先対応なら A1 / A2 の CLI 候補を評価する。両段が低影響なら
    tie-break により現状維持。中間も現状維持。
- **交絡**: watcher の debounce 間隔、エディタの atomic save、UI スレッドとの競合、
  branch / pull 自体の時間、背景タブの読み。
- **決まらないこと**: どちらの backend が速いか。それは A1・A2。

#### A1 — `working_tree_status` と `git status` の比較

- **問い**: 同じ意味の status を、libgit2 と CLI のどちらが速く返すか。
- **手順**:
  1. まず**情報等価性**を確認する。現行は `include_ignored(false)` /
     `include_untracked(true)` / `recurse_untracked_dirs(true)` /
     `renames_head_to_index(true)`で、さらに `WT_NEW` の nested repository / linked
     worktree（`.git` directory 又は file）を untracked から除外する
     （[`status.rs:51-56`](../../crates/kagi-git/src/status.rs#L51-L56)、
     [`status.rs:150-163`](../../crates/kagi-git/src/status.rs#L150-L163)）。CLI は
     `git status --porcelain=v2 -z --untracked-files=all --renames` を parse した後に同じ除外を適用する。
     `(group, path, rename-from, ChangeKind)` の canonical **集合**を比較し、双方の key が
     一意であることも確認する。missing / extra / value-mismatch / duplicate がいずれも
     0 の組み合わせだけを等価とする。
  2. 一致した組み合わせだけで S / M / L × warm / process-cold を測る。CLI 側は parse 込みの
     時間で測る（Kagi は構造化データを必要とするため、生の出力時間では比較にならない）。
- **値**: ms（median / p95）、canonical status 集合の missing / extra / value-mismatch /
  duplicate 件数。
- **判断基準**:
  - 意味が一致しない → 速度は測らず、差異を B3 / B4 に回す。
  - 一致し、L の median で libgit2 が CLI の **1.5 倍以上遅く、かつ絶対差 150 ms 以上**
    → CLI 候補。
  - A0 の実測頻度を掛けた 60 s 累積で CLI が **500 ms 以上かつ 30%以上削減**する
    → 1 tick の絶対差が 150 ms 未満でも CLI 候補。
  - 1 tick の絶対差 < 50 ms **かつ**累積削減 < 500 ms/60 s → libgit2 据え置き。
  - その間 → A3 の傾きで決める。
- **交絡**: fsmonitor / untracked cache（§4.6）、index の stat cache の warm 状態、
  rename 検出の閾値差、ファイル名の非 UTF-8 と大文字小文字の扱い。
- **決まらないこと**: 表示される内容が正しいか。それは B。

#### A2 — `snapshot` と CLI 合成の比較

- **問い**: `snapshot` 相当を CLI で組むと何プロセスかかり、合計で勝てるか。
- **手順**: `backend_probe --operation snapshot --backend libgit2|cli` を S / M / L で
  §4.2 の warm / process-cold 条件どおり実行する。現行 `snapshot`
  （`crates/kagi-git/src/snapshot.rs:87-128`）は
  `resolve_head` → `working_tree_status` → `collect_worktrees` →
  `commit_log_with_roots` → `collect_branches`（upstream の ahead/behind 付き） →
  `collect_remote_branches` → `collect_tags` → `collect_stashes` → `last_fetch_secs`
  の 9 段。CLI 合成の候補は `symbolic-ref` / `rev-parse`、`status --porcelain=v2`、
  `worktree list --porcelain`、`rev-list --parents`（commit budget 10,000 相当）、
  `for-each-ref`（branch と upstream 情報）、`for-each-ref refs/tags`、`stash list`、
  `FETCH_HEAD` の mtime（これは現行もファイルの mtime を見ている）。段ごとに時間を
  取り、合計と最も重い段を出す。
- **値**: プロセス数（個）、合計 ms、段ごとの ms、取得できた情報の欠落項目。
- **判断基準**: 合計が libgit2 の **0.7 倍以下**かつ情報等価、または A0 の頻度を
  掛けた累積を **500 ms/60 s 以上かつ 30%以上削減** → CLI 候補。合計が **1.0 倍超**
  → libgit2 据え置き確定。それ以外は全置換せず、最も重い段だけを CLI に寄せる
  部分判定に落とす。
- **交絡**: ahead/behind を 1 コマンドで取る `for-each-ref` の atom は git の版要求を
  上げる可能性がある（E2 で版を確定させること）。detached linked worktree を graph root
  に含める #595 の扱いは CLI 側で自前計算が必要になる。
- **決まらないこと**: プロセス起動コストの OS 差。それは E1。

#### A3 — 規模に対する傾き

- **問い**: 差は規模に対して開くのか、一定なのか。
- **手順**: tracked file 数 1k / 5k / 20k / 50k の合成 fixture で A1 を反復し、
  ms / 1k files の回帰直線を出す。
- **値**: 傾き（ms / 1k files）、切片（ms）、決定係数。
- **判断基準**: libgit2 の傾きが CLI の **1.5 倍超** → 大規模で必ず負けるため、A1 の
  絶対差が閾値未満でも CLI 候補に昇格させる。傾き差が 1.1 倍以内 → 規模は判断材料に
  しない。
- **交絡**: ディレクトリの幅と深さ（同じファイル数でも走査コストが変わる）、
  1 ディレクトリあたりのファイル数、シンボリックリンク。
- **決まらないこと**: snapshot 全体の置換可否。それは A2。

### B. 意味論の一致

共通手順: 測定 PR の `backend_fixture --scenario <name>` で fixture を materialize し、
`backend_probe --operation semantic-matrix --backend libgit2|cli` を各 3 回実行する。
**fixture lifecycle、反復、開始 fingerprint は §4.3 に従う。** probe は backend 固有出力を
次の canonical JSON に正規化する: path は `/` 区切り、status は Kagi の `ChangeKind`、
内容は生バイトの SHA-256、plan は blocker の型と対象 path、oplog は recovery handle の
`kind / oid / path / reference` と OID から読める object bytes の SHA-256。時刻、表示順、
backend 固有メッセージは比較対象から除く。**生の porcelain と Rust struct を直接 byte
compare しない。**

scenario 生成コマンド:

```sh
for scenario in clean-smudge autocrlf ignore submodule sparse partial-clone unrelated-histories recovery-handles; do
  cargo run -p kagi-git --release --example backend_fixture -- \
    --out "$FIXTURE_ROOT/semantic-$scenario" --scenario "$scenario" --seed 627
done
```

partial clone はネットワークを交絡させないため、同じ一時ディレクトリ内の bare repo を
`file://` remote とし、blob filter を有効にして欠落 blob を作る。clean/smudge filter は
fixture 内の固定済み補助 executable だけを呼び、PATH・標準入力・標準出力を manifest
へ記録する。

共通の判断基準:

- canonical JSON の差異が (i) 画面に表示される内容 hash、(ii) stage / commit される
  内容 hash、(iii) plan の blocker 型または対象 path、(iv) oplog の recovery handle
  の指す object / ref / 復元後内容 のいずれかを変える → 差異件数 **1 件以上で不合格**。
  その経路は git 本体に合わせる（CLI に寄せる、または libgit2 側で同じ設定を再現する）。
- recovery handle は backend 間で文字列 OID が同じかだけを見ない。各実行で記録 OID が
  実際に作成・drop・backup した object と同一で、object が存在し、handle から復元した
  worktree / index の canonical hash が事前状態と一致することを要求する。固定した
  author / committer / timestamp / parent topology でも stash OID が異なる場合は不合格。
  backup blob は同じ bytes なら OID も一致しなければ不合格。
- 差異 **0 件** → 意味論は backend 選定を動かさない。
- 時刻や表示順だけの差異 → 据え置き。ただし「既知の差異」として §9 に記録する。

| # | scenario | fixture と照合項目 | Kagi のどこが壊れるか（想定、実験で確定させる） |
| --- | --- | --- | --- |
| B1 | `clean-smudge` | `.gitattributes` の `filter=`、required filter、成功・失敗時の status / diff / stage content hash | diff pane の表示が git と食い違う。discard / stage が filter 前後の別バイト列を書く |
| B2 | `autocrlf` | CRLF / LF 混在、`core.autocrlf=true|input|false`、attribute の `eol=lf|crlf`。Windows と macOS/Linux で実施 | 全ファイルが変更扱いになる。commit panel が偽の変更を並べる |
| B3 | `ignore` | negation、`**`、末尾 `/`、階層別 precedence、`core.excludesFile`、`.git/info/exclude` | untracked 一覧、commit 候補、discard の対象範囲 |
| B4 | `submodule` | clean / modified / untracked / missing submodule の status と diff | status に偽変更、plan の preview、checkout の blocker |
| B5 | `sparse` | cone / non-cone / sparse index の 3 条件、未展開 path の status | 未 checkout の path を削除と表示し、discard が対象にする |
| B6 | `partial-clone` | 欠落 blob の status / diff / blame / file history、fetch 禁止時の error class | diff / blame / file history が失敗する。エラーが oplog とモーダルの両方に出ない場合は error handling が壊れる |
| B7 | `unrelated-histories` | `merge_commits` と git 本体の blocker 型 | `UnrelatedHistories` blocker（`plan_note/merge.rs:61`）の根拠が崩れる |
| B8 | `recovery-handles` | stash push / drop は通常条件、stash apply / pop は clean と conflict の両条件で、同一入力・固定時刻に両 backend 実行。記録 OID、object 存在、ref、実行後 worktree / index hash、stash entry の残留 / 削除を照合 | oplog は残るが OID が別 object または不存在を指す。apply / pop の conflict 後に worktree / index が違う、または残すべき stash を消す／消すべき stash を残す |

B7 を足す理由: 既に製品仕様（blocker）の根拠になっている差異なので、根拠が実測で
正しいことは規則の前提である。誤っていれば blocker 側を直す作業が発生する。

**B8 の stash matrix は push / drop の通常条件と、apply / pop の各 clean / conflict 条件を
両 backend で測る 12 cell**とする。`apply` と `pop` を push / drop の合格から推測しない。
各 cell は exit class / `GitError` 分類、worktree content hash、index の staged / unstaged
canonical state、stash entry の存在、recovery handle の OID / ref / object bytes hash、
handle から復元した state を記録して比較する。`apply` は成功・conflict とも stash を残し、
`pop` は clean 成功時だけ stash を消し conflict 時は残す、`drop` は対象 entry だけを消す
ことを Git 本体の期待値として固定する。実装がこの条件または両 backend 間の値を 1 件でも
満たさなければ B8 不合格であり、速度に関係なく CLI 候補に採らない。これは #624 / #625
で実際に問題になった recovery と state 遷移の軸である。

D1 / D2 が性能比較できる stash apply / pop CLI 実装は、測る**同じ操作**について B8 の
clean と conflict の両 cell に合格したものだけとする。push / drop の B8 合格を apply /
pop へ流用しない。stash 以外の CLI 実装は、それぞれに対応する B scenario の意味論合格
を要件とする。

- **値**: scenario / OS / git 版ごとの canonical JSON 不一致件数、内容 hash、
  blocker 型、recovery handle の OID / ref / object bytes hash / 復元後 state hash、
  不一致が到達する Kagi 機能。
- **交絡**: filter executable、global config、改行、path の大小文字、欠落 object の
  lazy fetch。probe は `GIT_CONFIG_GLOBAL` と `GIT_CONFIG_SYSTEM` を隔離し、fixture
  manifest に必要な config だけを再現する。
- **決まらないこと**: 速度。B は正しさだけを見る。

### C. 予測 API の代替可能性

**ここが規則の骨格。** LSP references で inventory 化した全用途について、CLI で書かずに
同じ情報が取れるかを確定させる。取れないものは取れないと明記する。

#### C1 — 用途ごとの代替可能性表

- **問い**: 各予測用途に、object を増やさず index も worktree も変えない CLI 手段が
  あるか。
- **手順**:
  1. 対象 commit で、§2.1 と同じ数え方により**全呼び出し式を 1 行ずつ列挙する**。
     LSP references から API ごとに引き、`api / file:line / 呼び出し元 symbol / Kagi 用途 /
     必要な戻り値` の CSV を作る。LSP の call expression 集合と CSV の
     `(api, file:line, caller)` 集合が API 別・全体とも完全一致し、CSV key の重複が 0
     でなければ C1 を開始しない。件数だけは比較しない。開始条件に固定件数は置かない。
  2. 用途を群にまとめる。群は最低でも次の 8 つ: merge 予測 / cherry-pick・revert 予測 /
     三方向の内容 merge / tree↔tree diff / tree↔index diff / index↔workdir diff /
     tree↔workdir diff / tree merge。
  3. 各 CSV 行に CLI 候補 argv と予想する出力 schema を記し、C2 のハーネスで
     **書くかどうか**、C3 で watcher を実測する。
  4. 元 API と CLI 候補の canonical JSON を conflict fixture（clean / text conflict /
     binary / symlink / file-directory / rename-rename）で各 3 回比較する。fixture lifecycle
     は §4.3 に従う。情報の欠落を書く。「速いが情報が足りない」は代替ではない。
- **値**: inventory の各行の Yes / No / 条件付き、欠落する field 数、必要な git 最低版。
- **判断基準**: 1 呼び出し単位で (a) C2 が非書き込み、(b) C3 が watcher 0 回、
  (c) canonical JSON の欠落 field 0・値の不一致 0 → CLI 代替可能。同じ用途群の
  **全呼び出しが合格したときだけ**その群を CLI 候補にする。1 行でも欠ければ当該群は
  **libgit2 に残す**。速度は理由にしない（予測経路の要件は正しさ）。
- **交絡**: git 版（`merge-tree --write-tree` は 2.38 以降）、conflict の表現形式の差
  （Kagi の conflict FSM が要求する情報粒度）、binary / symlink / 実行ビット、
  rename 検出の有無。
- **決まらないこと**: 実行経路の帰属。予測と実行は別に決める。

記入表（空欄のまま。実験 C1 が埋める）:

| 群 | 主な呼び出し元 | 必要な情報 | CLI 候補 | 書くか（C2） | watcher 発火（C3） | 判定 |
| --- | --- | --- | --- | --- | --- | --- |
| merge 予測 | `ops/merge.rs`, `ops/merge_into.rs`, `ops/pull*.rs`, `ops/pr_conflict.rs`, `ops/stash.rs` | — | — | — | — | — |
| cherry-pick / revert 予測 | `ops/cherry_revert.rs`, `ops/mod.rs` | — | — | — | — | — |
| 三方向の内容 merge | `resolution.rs`, `ops/pr_conflict.rs`, `ops/pull_conflict.rs` | — | — | — | — | — |
| tree↔tree diff | `diff.rs`, `diffstat.rs`, `ops/cherry_revert.rs`, `ops/squash_merge.rs`, `ops/checkout.rs` | — | — | — | — | — |
| tree↔index diff | `staging.rs`, `message_gen.rs`, `conflicts.rs`, `ops/absorb.rs`, `backend/stash.rs` | — | — | — | — | — |
| index↔workdir diff | `staging.rs`, `conflicts.rs`, `diffstat.rs`, `backend/stash.rs` | — | — | — | — | — |
| tree↔workdir diff | `diff.rs`, `ops/absorb.rs`, `backend/stash.rs` | — | — | — | — | — |
| tree merge | `conflicts.rs`, `ops/stash.rs` | — | — | — | — | — |

#### C2 — 非書き込みの判定ハーネス

- **手順**: 拡張 fingerprint を定義して前後で比較する。既存の `repo_fingerprint`
  （HEAD + porcelain status）では `merge-tree --write-tree` の object 増加を検出できない。
  repository handle ごとに canonical path を取り、main / linked worktree の同一 root は
  一度だけ走査して次を含める（file は `(relative path, size, mtime, mode, SHA-256)`、
  directory は `(relative path, mtime, mode)`）:
  1. worktree root（`.git` entry 以外）、
  2. worktree root の `.git` entry 自体（linked worktree では gitdir を指す file）、
  3. private gitdir `repo.path()`（`index` と worktree-private `HEAD` を含む）、
  4. common dir `repo.commondir()`（ODB、`packed-refs`、shared `refs/` を含む）。

  linked worktree の root / private gitdir / commondir を混同しない。layout の根拠は
  [`status.rs:206-213`](../../crates/kagi-git/src/status.rs#L206-L213)、
  [`backend.rs:42-57`](../../crates/kagi-git/src/backend.rs#L42-L57)、
  [`snapshot.rs:134-142`](../../crates/kagi-git/src/snapshot.rs#L134-L142)。
  候補コマンドの反復と fixture lifecycle は §4.3 に従う。`--no-optional-locks` の有無でも
  比較する（§4.6）:

  ```sh
  ./target/release/examples/backend_probe \
    --repo "$FIXTURE" --operation prediction-side-effects --backend cli \
    --candidate "$CANDIDATE" --iterations 3 --format json
  ```
- **値**: fingerprint 項目ごとの delta（OID、path、size、mtime、mode、SHA-256、
  一時ファイル）。
- **判断基準**: **ODB・index・refs・worktree の content と metadata が全て不変、かつ
  一時ファイルなし**のみ「予測に使える」。1 項目でも動けば予測不可
  （実行経路の候補にはなり得る）。
- **交絡**: gc / auto-maintenance の自動起動、`core.fsmonitor` のデーモン、
  index の stat cache 書き戻し、ファイルシステムの mtime 粒度。
- **決まらないこと**: 書かなくても watcher が発火するか。それは C3。

#### C3 — watcher 発火の有無

- **問い**: 候補コマンドは Kagi の watcher を起こすか。
- **手順**: 測定 PR に GUI E2E scenario `backend_prediction_watcher` を足す。
  scenario は実際の watcher を起動し、C2 の候補 argv を 1 回実行し、debounce の
  2 倍以上待って reload counter を読む。候補ごとの fixture lifecycle は §4.3 に従う。
  次の形で絞って実行する:

  ```sh
  KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY='backend_prediction_watcher' \
    cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
  ```

  この GUI scenario は macOS 専用。Linux / Windows は E4 の headless watcher probe で
  同じ候補 argv の raw event を測り、3 OS のいずれかで 1 回でも発火すれば不合格とする。

- **値**: 発火回数（回 / コマンド 1 回）、reload 到達の有無。
- **決まらないこと**: 候補が返す情報の完全性。それは C1。
- **判断基準**: **1 回でも発火 → 予測経路では使わない。** 0 回のみ可。
- **交絡**: debounce による見逃し（観測窓を debounce の 2 倍以上にする）、他タブの
  watcher、エディタや LSP など外部プロセスの書き込み。scenario 中は Kagi と候補
  command 以外が fixture を開かない。

C2・C3 を独立の実験として立てる理由: この 2 つが C1 の答えの信頼性を担保し、かつ
#625 の再発防止条件そのものだからである。C1 の表を文献だけで埋めると、
`merge-tree --write-tree` のような「worktree も index も無変更なのに object が増える」
ケースを Yes と書いてしまう。

#### C4 — 予測非書き込み規則の継続 enforcement

- **問い**: backend 規則の決定後も、plan / preflight 経路の書き込みと entrypoint の
  列挙漏れを PR 時点で確実に検出できるか。
  1. git layer の integration test `tests/prediction_non_write_test.rs` を測定後の実装
     PR で作る。通常の plan / preflight 実行 fixture lifecycle は §4.3 に従う。pure domain
     では ODB / index / worktree を観測できず、ops 単位の unit test では Backend dispatch や
     public entrypoint を漏らすため、この layer に置く。
  2. `Operation` の exhaustive match で全 variant を fixture へ対応させ、public な
     `Backend::plan` と Backend dispatch 外の public `plan_*` entrypoint に加え、
     `Backend` 経由と直接公開された **全 public `preflight_*` entrypoint** を実行する。
     成功・blocker・error のどの場合も、前後で C2 と同じ fingerprint を比較する。
     linked worktree では worktree root、`.git` file、private gitdir `repo.path()`、
     common dir `repo.commondir()` を別 root として扱い、canonical path が同じ root は
     一度だけ走査する。新しい `Operation` variant は match が非網羅になり、test 更新なし
     では compile できない形にする。
  3. detector 自体の self-test として、test fixture から sentinel blob を 1 個書き、
     object 差分を **1/1 回検出する**ことを assert する。さらに内容不変の touch と mode
     変更も各 **1/1 回検出する**ことを assert する。
  4. `ci/` の `Rule` 候補は、既知の書き込み API の plan 内 direct / helper 経由と、
     preflight 内 direct / helper 経由の **4 sample** で評価する。現行 `Rule` は
     ファイル全体への regex で、Rust の nested block と call graph を解釈しないため、
     helper 経由を証明できない。4 sample 全てを捕捉できない限り、静的 Rule は主
     enforcement に採らない。採る場合も direct call の defense-in-depth と明記し、
     dynamic test を置き換えない。
  5. **entrypoint inventory は名前集合で CI に置く。** `ci/` に
     `check-prediction-entrypoints` を追加し、`crates/kagi-git/src/**/*.rs` の各 public
     `plan_*` / `preflight_*` 定義を `repo-relative-path:function-name` key として集める。
     source key の重複を拒否し、その集合とソート済みの
     `ci/prediction-entrypoints-baseline.txt` の key 集合を完全一致させる。missing / extra
     key はどちら向きでも CI failure とする。`pub fn plan_x(`、`pub async fn preflight_y(`
     を含み、非 public `fn plan_x(` と `pub fn planner(` を含まない positive / negative
     sample を `check-all --selftest` で毎回証明する。
  6. `prediction_non_write_test` は `Entrypoint { key, dispatch }` table を持ち、table key
     の**集合**と baseline key の集合が完全一致し、かつ table key に重複がないことを
     assert する。各 dispatch arm は対応 symbol を直接呼ぶ。新関数追加は CI の source
     set 差分で、baseline 更新後の table 漏れは test の set 差分で、削除・rename は
     direct symbol の compile error で失敗する。

     既存 `Ratchet` は per-file 数値用
     （[`cli.py:85-131`](../../ci/src/kagi_checks/cli.py#L85-L131)）なので名前集合を表せない。
     同じ `ci/` project / `check-all --selftest` / baseline update workflow を使う custom
     check にする。静的 `Rule` は whole-text regex で call graph を解釈しない
     （[`rules.py:66-116`](../../ci/src/kagi_checks/rules.py#L66-L116)）。Rust test が source を
     走査する案は `cfg(test)`、macro、module 解決を別実装で再現する source-text test に
     なるため採らない。dynamic test は非書き込み挙動、CI check は entrypoint 集合を担う。

     `ci/pyproject.toml` の entry、workflow matrix、baseline を同じ実装 PR に含める。
     guidance は「`Entrypoint` table を追加して test を通してから
     `uv run --project ci check-prediction-entrypoints --write-baseline`」とする。

  ```sh
  cargo test -p kagi --test prediction_non_write_test
  uv run --project ci check-prediction-entrypoints
  uv run --project ci check-all
  ```

- **値**: plan / preflight の source key 集合、baseline key 集合、`Entrypoint` table key
  集合、それぞれの missing / extra / duplicate、bytes / path / mtime / mode ごとの前後差分、
  sentinel / touch / mode 検出数、static Rule の direct / helper sample 検出数、現行 source
  の false positive 数。
- **判断基準**: dynamic test は plan / preflight coverage **100%**、3 key 集合が完全一致、
  table duplicate **0 件**、通常経路の差分 **0 件**、sentinel / touch / mode 検出が各
  **1/1** 必須。1 つでも欠ければ規則を実装完了としない。static Rule は 4 sample **4/4**、
  false positive **0 件**のときだけ追加する。それ以外は call graph を扱えないため不採用と
  記録する。
- **交絡**: linked worktree の private gitdir / commondir、git auto-maintenance、
  index stat cache、error 経路で一時作成後に削除される object 以外のファイル。
- **決まらないこと**: どちらの backend を選ぶか。C4 は決定済み規則の維持だけを担う。

検知主体は local の当該 test と `check-prediction-entrypoints`、PR ごとの
`cargo test --workspace` と `uv run --project ci check-all` を実行する CI である。
規則違反は test または entrypoint-set check failure として変更者・reviewer の両方に届く。

### D. 実行経路で libgit2 が遅い／危ういもの

#### D1 — 「未変更ファイルを読み直す」形の洗い出し
- **問い**: `stash_save2` と同じ形の API が他にあるか。

- **手順**:
  1. `crates/kagi-git/src/ops/`、`staging.rs`、`backend/` にある libgit2 の
     worktree / index 書き込み API を LSP references で 1 行ずつ inventory 化する。
  2. 各 API について「入力に workdir / index を渡すか」「pathspec を限定できるか」
     「内部で status / diff を作るか」を libgit2 1.9.1 の API 文書で分類する。stash apply /
     pop の CLI 実装は B8 の同一操作・clean / conflict 両条件で、他の操作の CLI 実装は
     B の該当 scenario で意味論に合格したものだけ性能比較へ進める。
  3. 変更ファイル数を 2 に固定し、未変更 tracked ファイルを 1k / 5k / 20k / 50k と
     増やして libgit2 と合格済み CLI 実装を測り、両 backend の傾きと L の median を
     比較する。**fixture lifecycle、反復、copy の計時除外は §4.3 に従う。**

     ```sh
     ./target/release/examples/backend_probe \
       --repo "$FIXTURE" --operation "$OPERATION" --backend libgit2 \
       --iterations 11 --format json
     ./target/release/examples/backend_probe \
       --repo "$FIXTURE" --operation "$OPERATION" --backend cli \
       --iterations 11 --format json
     ```

     最低対象は `stash_apply` / `stash_pop` / `stash_drop`（stash push 以外は現在も
     libgit2）、staging の index 書き込み、`ops/checkout.rs`、`ops/discard.rs`、
     `working_tree_status`、`diff_index_to_workdir`。inventory で見つかった API をこの
     列挙より優先し、候補を黙って落とさない。
- **値**: backend・API ごとの傾き（ms / 未変更 1k files）、切片（ms）、L の median。
- **判断基準**: libgit2 の傾き **≥ 0.5 ms / 1k files** なら「`stash_save2` 型」の疑い。
  CLI 採用候補にするのは、意味論合格に加え、L で libgit2 が CLI の **1.5 倍以上遅く、
  かつ絶対差 150 ms 以上**の場合だけ。libgit2 の傾き **< 0.1 ms / 1k files**、
  または相対差・絶対差の片方でも未達なら据え置く。中間は D2 で原因を確認する。
- **交絡**: 未変更判定に使う stat cache の状態、ファイルサイズ分布（1 MiB blob を
  含めること）、ファイルシステム、他プロセスの I/O。
- **決まらないこと**: 遅い理由。それは D2。

#### D2 — 分類の裏取り（未変更 path への読み取り回数）

- **問い**: 傾きの原因は本当に「未変更ファイルの読み直し」で、CLI 実装はそれを
  解消するか。
- **手順**: D1 で傾きが出た API と、stash apply / pop なら B8 の同一操作・clean /
  conflict 両条件で、それ以外なら B の該当 scenario で意味論に合格した対応 CLI 実装を
  測る。**fixture lifecycle は §4.3 に従う。** Linux は次の形で open / read の syscall
  数を取り、macOS は `fs_usage`、Windows は `Operation is ReadFile` /
  `Path begins with <fixture>` filter で同じ値を取る。

  ```sh
  strace -f -c -e trace=openat,read -- \
    ./target/release/examples/backend_probe \
    --repo "$FIXTURE" --operation "$OPERATION" --backend libgit2 \
    --iterations 1 --format json
  strace -f -c -e trace=openat,read -- \
    ./target/release/examples/backend_probe \
    --repo "$FIXTURE" --operation "$OPERATION" --backend cli \
    --iterations 1 --format json
  ```

- **値**: backend ごとの open 回数、read 回数、read バイト数、未変更ファイル数との比。
- **判断基準**: file 数 1k→50k で libgit2 の open または read 回数が **0.9 以上の
  相関係数**で増え、かつ未変更 1 file あたり **0.9 回以上**なら「読み直し」と分類する。
  CLI が同じ原因を解消したとするには、対応値が libgit2 の **50% 以下**かつ未変更
  1 file あたりの絶対差 **0.5 回以上**を要求する。さらに D1 の相対時間差と絶対時間差を
  満たした場合だけ CLI 採用候補にする。いずれか未達なら CLI 化の根拠にせず据え置く。
- **交絡**: トレース自体のオーバーヘッド、backend 内部のキャッシュ、OS の readahead。
- **決まらないこと**: 意味論の一致。それは B の合格を前提とする。

D の優先順: **stash family（apply / pop / drop）を最初に見る**。push だけが CLI に
移った状態（#622/#623）は同一 family 内で backend が分かれているので、残りが同型なら
規則の適用対象として最も自然であり、同型でなければ実験 F の混在コストの実例になる。

### E. CLI 依存を広げる場合のコスト

#### E1 — プロセス起動の下限

- **問い**: プロセス起動コストは hot path でどう効くか。
- **手順**: 3 OS で次を各 20 回実行し、`git --version` をほぼ純粋な起動コスト、
  S fixture の status を起動 + 最小実処理として扱う。Windows は Defender の
  実時間スキャン有効・無効の両方。

  ```sh
  hyperfine --warmup 1 --runs 20 \
    'git --version' \
    'git -C "$FIXTURE_ROOT/S" --no-optional-locks status --porcelain=v2 -z --untracked-files=all'
  ```

- **値**: ms（median / p95）、OS ごとの比。
- **判断基準**: 読み経路では、**起動下限 × 必要プロセス数が A0 の p95 の 20% を
  超えるなら CLI に寄せない**。Windows の起動下限が **30 ms 以上**なら、A2 の
  複数プロセス合成は読み経路では採らない（単発コマンドの置換のみ検討）。
- **交絡**: ウイルス対策ソフト、`PATH` 上の shim、WSL / MSYS の git、
  ネットワークドライブ。
- **決まらないこと**: status / snapshot 自体の処理時間。それは A1・A2。

#### E2 — 最低 git 版と、無い場合・古い場合の振る舞い

- **問い**: 規則が要求する git 最低版はいくつになり、それを満たさない環境で Kagi は
  何をするか。
- **手順**: 規則が使うことになった各コマンド／オプションについて導入版を一次資料で
  確定する（`merge-tree --write-tree` = 2.38、`status --porcelain=v2`、
  `for-each-ref` の ahead/behind atom、`--no-optional-locks` など）。最も新しい要求が
  最低版になる。

  1. **standalone probe（3 OS）**: `backend_probe --operation cli-capability` に git
     executable を渡し、git 不在 / 2.37 / 2.38 / 2.50.1 / 最新安定版を各 3 回実行する。
     probe は `KagiApp` を起動しないため、ここで測る値は exit class、`GitError` variant /
     stable error code、検出した version text と parse 結果、stderr 分類、panic / hang
     なしだけとする。

     ```sh
     ./target/release/examples/backend_probe \
       --operation cli-capability --git-executable /missing/git --iterations 3 --format json
     ./target/release/examples/backend_probe \
       --operation cli-capability --git-executable "$GIT_237" --iterations 3 --format json
     ./target/release/examples/backend_probe \
       --operation cli-capability --git-executable "$GIT_238" --iterations 3 --format json
     ```

  2. **UI delivery（Tier A、macOS）**: probe とは別に GUI E2E scenario
     `backend_cli_capability_modal` を測定 PR で追加する。`git_command` が PATH の `git`
     を起動し `run_git` がそれを使う既存 seam
     （[`cli.rs:231-242`](../../crates/kagi-git/src/cli.rs#L231-L242)、
     [`cli.rs:284-294`](../../crates/kagi-git/src/cli.rs#L284-L294)）を使い、(a) git を
     含まない PATH、(b) `git --version` に 2.37 を返す executable shim だけの PATH の
     2 条件で KagiApp を mount し、CLI 依存操作を起動する。`active_modal` の capability
     error、`KAGI_LOG_DIR` の oplog failure record、HEAD / index / worktree 不変を assert
     する。実行は必ず次で絞る:

     ```sh
     KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY='backend_cli_capability_modal' \
       cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
     ```

     Tier B `pidclick` ではなく Tier A を gate にする。modal slot と oplog record を
     deterministic に assert でき、実機 click の可視確認を必要としないためである。scenario
     を追加する integration owner は `.claude/skills/verify/SKILL.md` の Tier A inventory
     も同じ PR で更新する。

- **値**: standalone probe は機能ごとの導入版、算出した最低版、各版の exit class /
  `GitError` variant / stable error code / version parse / panic・hang の有無。Tier A は
  missing / old の各条件で capability modal、oplog failure record、repo fingerprint
  不変の pass / fail。
- **判断基準**: **最低版の上限を 2.38 とする。** これを超える版を要求する規則は採らない
  （代替コマンドを使うか、その操作を libgit2 に残す）。最低版を宣言する実装は、
  standalone probe で missing / old を安定した `GitError` として返し、Tier A の
  `backend_cli_capability_modal` で git 不在 / 旧版の **両条件**について oplog + modal
  を確認できることを必須とする。片方でも欠ければ CLI 依存を広げない。
- **交絡**: ディストリの古い git、macOS 同梱の Apple Git の版差、企業環境の固定版、
  `PATH` 上の別 executable。
- **決まらないこと**: 最低版を満たす環境での速度・意味論。それぞれ A・B で決める。

#### E3 — hardening 面の拡大

- **問い**: CLI 経路を増やすと ADR-0146 の対策範囲は足りるか。
- **手順**: 新たに CLI へ寄せる候補コマンドについて、repo-local config で挙動を変え
  られる項目（`core.*` の外部コマンド系、`filter.*`、`diff.external`、`credential.*`
  など）を列挙し、`run_git` の既存対策で覆えているかを 1 件ずつ突き合わせる。候補ごとの
  悪性 config fixture lifecycle は §4.3 に従い、既存 hardening test に追加してその file
  全体を次で実行する:

  ```sh
  cargo test -p kagi --test cli_hardening_test
  ```

- **値**: 未対応項目の件数と内容、hardening test の pass / fail。
- **判断基準**: **未対応が 1 件でもあれば、その操作を CLI に寄せない。** 対策を実装して
  test が全件通った後にだけ再候補化する。新規 CLI 操作は `run_git` 経由のみとし、
  `Command` 直呼びは 0 件を要求する。
- **交絡**: submodule 内の config、`include.path`、環境変数（`GIT_*`）。
- **決まらないこと**: hardening 実装後の速度。E3 は CLI 採用の前提条件だけを決める。

#### E4 — Windows と Linux の差

- **問い**: 現在の CI / GUI 制約の下で、OS 非依存の backend 規則をどの証拠から決めるか。
- **実行環境**:

  | 種類 | 対象 | macOS | Linux | Windows | 規則への使い方 |
  | --- | --- | --- | --- | --- | --- |
  | source / 一次資料 | C1、E2 の導入版調査 | 実行可 | 再実行不要 | 再実行不要 | 同一 commit なら 1 回 |
  | headless fixture / probe | A1–A3、B1–B8、C2、D1、E1、E2 capability、E3、F | 実行可 | hosted runner で実行可 | hosted runner で実行可 | 採用候補は 3 OS 必須 |
  | 恒久 integration test | C4 | 実行可 | advisory test job で実行可 | 現 CI は build のみ。測定 PR で test job を追加 | backend 実装完了条件。3 OS の回帰監視 |
  | native GUI E2E / Tier B | A0 の実 UI、C3 の reload、E2 の capability modal、実機 GUI | Tier A / `pidclick` で実行可 | 現 runner では実行不可 | 現 runner では実行不可 | macOS の統合確認。E2 modal は Tier A gate、単独では OS 非依存採用の根拠にしない |
  | OS watcher probe | A0 の event/tick、C3 の発火有無 | GUI E2E と照合 | `start_git_watcher` と同じ `notify` recursive watcher / `DEBOUNCE` 500 ms を使う headless probe を追加 | 同じ headless probe を追加 | C3 は 3 OS 必須。A0 は後述 |

  現 CI は macOS test だけが blocking、Linux test と Windows build は advisory
  （[`.github/workflows/ci.yml:105-168`](../../.github/workflows/ci.yml#L105-L168)）。Windows
  test job と実験 job はまだ無い。測定 PR で `workflow_dispatch` の macOS / Ubuntu /
  Windows matrix を追加し、headless probe の JSON artifact を保存する。performance は backend
  を同一 runner 内で交互に測り、runner image / CPU / 負荷を §4.5 の環境メタへ残す。
  advisory か blocking かは証拠の有無と分けて扱う。
- **判断基準**:
  - OS ごとに backend を変える規則は採らない。新しい backend を採る操作は、関連する
    headless の意味論・安全性・性能 gate を **3 OS 全て**で満たし、最悪値でも各節の
    閾値を満たすこと。
  - 3 OS の証拠が揃わない場合は計画全体を永久保留にせず、**その操作だけ現行 backend に
    据え置く**。証拠が追加された時点で再判定する。既存 CLI 操作を差し戻す理由にはしない。
  - 予測経路を CLI に移す候補は C3 の watcher 0 回を 3 OS で要求する。Linux / Windows
    の headless watcher probe が用意できなければ、その候補は libgit2 に残す。
  - A0 の macOS GUI 値は読み経路の優先順位付けと実 UI 回帰に使う。A0 の累積削減条件を
    採用根拠に使う場合だけ、Linux / Windows の headless event/tick probe も必須にする。
    それを用意できない場合でも、A1–A3 の OS 別絶対・相対閾値だけで採否を決められる。
  - D2 は原因説明を強める診断であり、Windows Process Monitor の未実施だけでは候補を
    保留しない。採否は B、D1、E1–E3、F の 3 OS gate で決め、D2 が意味論上の危険を
    示した場合は不合格側へ倒す。
- **値**: OS・実験ごとの規定値、canonical JSON 不一致件数、各 gate の pass / fail、
  実行不能だった lane と代替 probe。
- **交絡**: hosted runner の CPU / I/O 変動、改行変換（B2）、パスの大文字小文字、
  パス長制限、権限モデル、watcher backend（FSEvents / inotify / ReadDirectoryChangesW）。
- **決まらないこと**: 各 OS で個別最適な backend。OS 別分岐を採らないため測らない。

この方針は「全実験を無条件に 3 OS」ではなく、**backend 採用を動かす portable gate と
C3 だけを 3 OS 必須**にする。macOS 専用 GUI と OS 固有 trace は統合確認・原因診断・
採用後の回帰監視に限定する。現在存在しない GUI runner や対話的 Process Monitor を
必須条件にして永久保留を作らず、安全証拠が不足した操作は保守的に据え置く。

### F. 混在のコスト（追加）

- **問い**: family 内で backend が分かれるとき、規則の単位は family か操作か。
- **手順**: 既に混在している stash family（push だけ CLI、apply / pop / drop は
  libgit2）を実例として、plan と execute の前提が食い違う条件を列挙する。少なくとも
  「libgit2 の予測が通ったのに CLI の実行が別の結果になる／逆」の fixture を
  `backend_fixture --scenario mixed-stash` で作り、各条件を 3 回再現する。fixture
  lifecycle は §4.3 に従う:

  ```sh
  cargo run -p kagi-git --release --example backend_fixture -- \
    --out "$FIXTURE_ROOT/mixed-stash" --scenario mixed-stash --seed 627
  ./target/release/examples/backend_probe \
    --repo "$FIXTURE_ROOT/mixed-stash" --operation stash-family \
    --backend mixed --iterations 3 --format json
  ```

- **値**: 食い違う条件の件数、再現できたもの／できなかったもの。
- **判断基準**: write 操作は F の結果に関係なく、常に
  **plan → confirm → preflight → execute → verify → oplog** を実装条件とする。これは
  backend 混在時だけの追加条件ではなく、全 write に対する不変条件である。食い違いが
  1 件でも再現する場合は、`verify_X` が実行後の状態を読み直して plan の予測との差を
  検出・報告できるまで family 内分割を許可しない。0 件なら family 内分割を許可するが、
  `verify_X` を省略・弱化してはならない。
- **交絡**: 実行時の repo 状態変化（TOCTOU）、oplog に残る記録の形。
- **決まらないこと**: verify の要否。全 write で必須と既に決まっている。F は
  backend を分けた場合に verify が照合すべき追加条件だけを決める。

### 実施しない領域とその理由

| 領域 | 理由 |
| --- | --- |
| jj の評価 | jj は Git index / `.gitattributes` / partial clone を自ら unsupported と文書化しており（#627 §3）、現行 `Backend` の差し替え候補ではない。測っても操作ごとの帰属規則は決まらない |
| git の同梱の測定 | §2-5 で方針として外れている。同梱しても OS 3 つ分の差異を自分が抱えるだけ。代わりに E2 で最低版を決める |
| 全面 CLI 移行の測定 | §2-1・§2-2 で予測経路が成立しないため、比較対象として意味がない |
| libgit2 upstream の修正 | 規則の確定に必要ない。D で該当 API が特定できたら別課題として扱う |
| ネットワーク FS 上での測定 | 交絡が支配的で閾値が決まらない。対象外と明記する |

## 6. 結果記入欄（未実施）

| 実験 | 実施日 | 環境 | 値 | 判定 |
| --- | --- | --- | --- | --- |
| A0 | — | — | — | — |
| A1 | — | — | — | — |
| A2 | — | — | — | — |
| A3 | — | — | — | — |
| B1 | — | — | — | — |
| B2 | — | — | — | — |
| B3 | — | — | — | — |
| B4 | — | — | — | — |
| B5 | — | — | — | — |
| B6 | — | — | — | — |
| B7 | — | — | — | — |
| B8 | — | — | — | — |
| C1 | — | — | — | — |
| C2 | — | — | — | — |
| C3 | — | — | — | — |
| C4 | — | — | — | — |
| D1 | — | — | — | — |
| D2 | — | — | — | — |
| E1 | — | — | — | — |
| E2 | — | — | — | — |
| E3 | — | — | — | — |
| E4 | — | — | — | — |
| F | — | — | — | — |

## 7. 決定表（未確定）

| 操作群 | 区分 | 調査時点の現行（根拠は §2.2） | 決定 | 直接測る根拠実験 |
| --- | --- | --- | --- | --- |
| `working_tree_status` | 読み | libgit2 | — | A0（watcher tick）、A1（等価性・時間）、A3（file 数傾き）、E1（起動費） |
| `snapshot` | 読み | libgit2 | — | A0（watcher tick）、A2（9 段合成）、E1（起動費） |
| merge 予測 | 予測 | libgit2 | — | C1（用途集合）、C2（write）、C3（watcher）、C4（恒久 enforcement） |
| cherry-pick・revert 予測 | 予測 | libgit2 | — | C1（用途集合）、C2、C3、C4 |
| 三方向の内容 merge | 予測 | libgit2 | — | C1（用途集合）、C2、C3、C4 |
| diff 各種（tree / index / workdir） | 予測・読み | libgit2 | — | C1（全用途）、C2、C4、B1 / B2（filter / 改行意味論） |
| stash push | 実行 | CLI（#622/#623） | 据え置き（回帰させない） | B8（push recovery / state） |
| stash apply / pop / drop | 実行 | libgit2 | — | B8（各操作意味論）、D1（傾き）、D2（read/open）、F（mixed-stash） |
| rebase・sequencer continue | 実行 | CLI（ADR-0131） | 据え置き | —（移行候補でない。実験の採否対象外） |
| fetch | 実行 | CLI | 据え置き | —（移行候補でない。実験の採否対象外） |
| pull | 実行 | CLI fetch + libgit2 統合 | 据え置き | —（A0 は pull 後の読取負荷だけを測り、pull backend は決めない） |
| push | 実行 | CLI | 据え置き | —（移行候補でない。実験の採否対象外） |
| switch | 実行 | 混在 | 据え置き | —（移行候補でない。checkout 実験を流用しない） |
| checkout | 実行 | libgit2 | — | B5（sparse checkout blocker）、D1（`ops/checkout.rs` 傾き）。**F は stash 専用で根拠に使わない。** |
| discard | 実行 | libgit2 | — | B1（filter content）、B3（ignore）、B5（sparse）、D1（傾き） |

**根拠範囲の監査**: 上表の 15 行を手順本文と照合した。F を checkout から外した。さらに
pull / switch は現行 backend を変える実験が無いので、A0 / checkout の結果を根拠に流用
しない。ほかに操作を直接測らない実験を根拠へ置く行はない。`—` は既存 backend を据え置く
行であり、新規 backend 決定に使わない。

**tie-break 規則**: 測定値が閾値の間に落ちた場合は**現状維持**（libgit2 据え置き、
CLI 済みは CLI 据え置き）。移行は「閾値を超えた」ことを示せた操作にだけ行う。

## 8. 実装後の検証手順（段 9、実装 PR で使う）

1. `cargo build`、`cargo test --workspace` が緑。
2. `cargo fmt --all` 実行後に `cargo fmt --check` が差分なし、`cargo clippy --workspace`
   に新規警告なし。
3. `uv run --project ci check-all` が緑。
4. 変更が読み経路や UI に触れる場合のみ、GUI E2E を**シナリオを絞って**実行する
   （フル実行は禁止。`KAGI_GUI_E2E_ONLY` は必須）:

   ```sh
   KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY='read_owner_switch,footer_status_line' \
     cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
   ```

5. C4 の `tests/prediction_non_write_test.rs` は一時的な実験ではなく、規則を守らせる
   恒久 integration test として残す。全 `Operation` variant / public plan /
   public preflight entrypoint の coverage 100%、通常経路前後の C2 fingerprint 差分 0、
   sentinel object / touch / mode 変更の検出各 1/1、source / baseline / `Entrypoint`
   table の**名前集合完全一致**、table key 重複 0 を固定する。新しい public `plan_*` /
   `preflight_*` は CI の source-set check が先に落とし、baseline 更新後も table への
   実呼び出し追加なしには test を通さない。次の test file / gate 全体を実行する:

   ```sh
   cargo test -p kagi --test prediction_non_write_test
   uv run --project ci check-prediction-entrypoints
   ```

   静的 `ci/ Rule` は C4 の plan / preflight の direct / helper sample **4/4**・
   false positive 0 の基準を満たす場合だけ defense-in-depth として追加する。現行 regex
   Rule は call graph を追えないため、満たさなければ追加せず、その理由を ADR に残す。
6. B8 の stash / backup recovery handle は、push / drop の通常条件と apply / pop の
   clean / conflict 全 cell で、記録 OID の object 存在、ref、worktree / index hash、
   stash entry の残留 / 削除を統合テストで固定する。文字列が残るだけの test では不可。
7. backend を移した操作には `plan_/preflight_/execute_/verify_` の整合と、oplog への
   記録が残ることを確認する統合テストを足す。
8. 一度に走らせる cargo コマンドは 1 本（AGENTS.md の build hygiene）。

## 9. 知識化（段 10、決定後）

- 決定は ADR にする（番号は作成時に採番。`docs/adr/` の重複番号ゲートあり）。
  最低限、操作ごとの帰属規則・閾値・却下した選択肢とその理由を書く。
- ADR に載せるほどでない細目は `docs/decisions.md` に 1 行。
- B で見つかった差異は「既知の意味論差異」として ADR に一覧で残す。
  `crates/kagi-domain/src/plan_note/merge.rs:61-63` のように、コード側にも根拠コメント
  として置く。
- #627 に本書と結果へのリンクを追記する。
- 本書は結果と決定で更新し、`> Status:` と `> Last updated:` を書き換える。

## 10. 根拠

- [Issue #627 — Git backend 候補・Kagi の現状・VS Code Git 拡張](https://github.com/TomiXRM/kagi/issues/627)
- [Issue #622](https://github.com/TomiXRM/kagi/issues/622) /
  [#623](https://github.com/TomiXRM/kagi/issues/623) — stash push の原因特定と CLI 移行
- [Issue #624](https://github.com/TomiXRM/kagi/issues/624) — oplog recovery handle の
  OID / backup blob 同一性
- [Issue #625](https://github.com/TomiXRM/kagi/issues/625) — plan 中の書き込みによる
  watcher / reload / modal 消失
- [`docs/adr/0131-sequencer-continue-and-rebase-current-onto.md`](../adr/0131-sequencer-continue-and-rebase-current-onto.md)
- [`docs/adr/0146-run-git-hardening.md`](../adr/0146-run-git-hardening.md)
- [`crates/kagi-git/src/status.rs`](../../crates/kagi-git/src/status.rs)
- [`crates/kagi-git/src/snapshot.rs`](../../crates/kagi-git/src/snapshot.rs)
