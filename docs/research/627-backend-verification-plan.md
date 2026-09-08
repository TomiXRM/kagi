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

本文は依頼項目に合わせて A→F の順に並べるが、**実行順は C → E → B → A → D → F**
とする。C で予測経路の候補を先に確定し、E で CLI 候補の版・hardening 前提を落とし、
B で意味論を確認してから速度を測る。C で予測を libgit2 に残すと確定した場合も、
A は読み経路を CLI に寄せるか、libgit2 のまま最適化するかを決める独立材料なので
省略しない。

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
   消えない。Kagi は現在 git の版チェックも同梱もしていない。`merge-tree --write-tree`
   は git 2.38 以降なので、CLI 依存を広げるなら最低版要求が必要になる。
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

### 4.3 ハーネスと共通コマンド

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

### 4.4 結果に必ず添える環境メタ

OS と版 / arch / `git --version` / `git2` crate 版（現行 0.21） / ファイルシステム /
`core.fsmonitor` と `core.untrackedCache` の値 / index 版 / fixture manifest /
測定中の他プロセス負荷。これが無い数字は §6 に載せない。

### 4.5 全実験に共通する交絡

- **fsmonitor / untracked cache は CLI 側にしか効かない**。有効・無効の両条件で測る。
  片方だけの数字で backend を決めてはいけない。
- **git が読み取りでも index を書き戻し得る**。`--no-optional-locks` はそのために存在
  する。読み経路・予測経路の CLI 候補は、この抑止が必要かを C2 で必ず確認する。
- OS（macOS / Linux / Windows）、git 版、リポジトリ規模、ファイルシステム
  （APFS / ext4 / NTFS、ネットワーク FS は対象外と明記）。
- 同一マシンで cargo build が走っていないこと（AGENTS.md の build hygiene）。

### 4.6 閾値の根拠

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
  各 case を M と L で 5 回実行し、case 間ではアプリを再起動する。
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
     `renames_head_to_index(true)`（`crates/kagi-git/src/status.rs:52-56`）。CLI 側の
     等価は `git status --porcelain=v2 -z --untracked-files=all` に rename 検出の
     指定を足した形。両者の出力を同一 fixture で突き合わせ、行数と分類の一致件数を
     数える。
  2. 一致した組み合わせだけで S / M / L × warm / process-cold を測る。CLI 側は parse 込みの
     時間で測る（Kagi は構造化データを必要とするため、生の出力時間では比較にならない）。
- **値**: ms（median / p95）、出力行数、不一致件数。
- **判断基準**:
  - 意味が一致しない → 速度は測らず、差異を B3 / B4 に回す。
  - 一致し、L の median で libgit2 が CLI の **1.5 倍以上遅く、かつ絶対差 150 ms 以上**
    → CLI 候補。
  - A0 の実測頻度を掛けた 60 s 累積で CLI が **500 ms 以上かつ 30%以上削減**する
    → 1 tick の絶対差が 150 ms 未満でも CLI 候補。
  - 1 tick の絶対差 < 50 ms **かつ**累積削減 < 500 ms/60 s → libgit2 据え置き。
  - その間 → A3 の傾きで決める。
- **交絡**: fsmonitor / untracked cache（§4.5）、index の stat cache の warm 状態、
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

共通手順: 測定 PR の `backend_fixture --scenario <name>` で独立 fixture を作り、
`backend_probe --operation semantic-matrix --backend libgit2|cli` を各 3 回実行する。
probe は backend 固有出力を次の canonical JSON に正規化する: path は `/` 区切り、
status は Kagi の `ChangeKind`、内容は生バイトの SHA-256、plan は blocker の型と対象
path、oplog は recovery handle の `kind / oid / path / reference` と OID から読める
object bytes の SHA-256。時刻、表示順、backend 固有メッセージは比較対象から除く。
**生の porcelain と Rust struct を直接 byte compare しない。**

`backend_fixture` が作る scenario repo は読み取り専用の pristine template とする。
`backend_probe` は **backend ごと、かつ反復ごと**に template から byte-for-byte 同一の
独立した一時 copy を作り、その copy だけを変更して破棄する。hardlink / reflink による
mutable file の共有は禁止する。特に `recovery-handles` の stash push / drop / restore と
file backup は repo を使い回さない。全 backend・全反復が同じ manifest と canonical
事前 fingerprint から始まらなければ、その比較結果は無効とする。

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
| B8 | `recovery-handles` | stash push / drop と file backup を同一入力・固定時刻で両 backend 実行。記録 OID、object 存在、ref、復元後 worktree / index hash を照合 | oplog は残るが OID が別 object または不存在を指し、stash / backup blob を復元できない |

B7 を足す理由: 既に製品仕様（blocker）の根拠になっている差異なので、根拠が実測で
正しいことは規則の前提である。誤っていれば blocker 側を直す作業が発生する。

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
     LSP references で API ごとに引き、
     `api / file:line / 呼び出し元 symbol / Kagi 用途 / 必要な戻り値` の CSV を作る。
     **LSP references で数えた呼び出し式の件数と inventory の行数が API 別・合計とも
     一致しなければ C1 を開始しない。** 開始条件に固定件数は置かない。
  2. 用途を群にまとめる。群は最低でも次の 8 つ: merge 予測 / cherry-pick・revert 予測 /
     三方向の内容 merge / tree↔tree diff / tree↔index diff / index↔workdir diff /
     tree↔workdir diff / tree merge。
  3. 各 CSV 行に CLI 候補 argv と予想する出力 schema を記し、C2 のハーネスで
     **書くかどうか**、C3 で watcher を実測する。
  4. 元 API と CLI 候補の canonical JSON を conflict fixture（clean / text conflict /
     binary / symlink / file-directory / rename-rename）で各 3 回比較する。
     情報の欠落を書く。「速いが情報が足りない」は代替ではない。
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

- **問い**: ある CLI コマンドは本当に何も書かないか。
- **手順**: 拡張 fingerprint を定義して前後で比較する。既存の `repo_fingerprint`
  （HEAD + porcelain status）では `merge-tree --write-tree` の object 増加を検出でき
  ないため、次を含める:
  1. ODB の全 OID set と、loose object / pack 関連ファイルごとの
     (relative path, size, mtime, mode, SHA-256)、
  2. `.git/index` の (size, mtime, mode, SHA-256)、
  3. `packed-refs` と `refs/` 配下の file / directory ごとの
     (relative path, size, mtime, mode, SHA-256。directory は hash なし)、
  4. 作業ツリーの全 file / directory ごとの
     (relative path, size, mtime, mode, SHA-256。directory は hash なし)、
  5. `.git/` 直下の一時ファイル（`*.lock`、`MERGE_*`、`CHERRY_PICK_HEAD` など）の有無。
  各候補コマンドを 3 回実行し（初回だけ書くケースを捕まえるため）、毎回比較する。
  `--no-optional-locks` の有無でも比較する（§4.5）:

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
  2 倍以上待って reload counter を読む。候補 1 件ずつ次の形で絞って実行する:

  ```sh
  KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY='backend_prediction_watcher' \
    cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
  ```

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

- **問い**: backend 規則の決定後も、plan / preflight 経路の書き込みを PR 時点で
  確実に検出できるか。
- **手順**:
  1. git layer の integration test `tests/prediction_non_write_test.rs` を測定後の実装
     PR で作る。pure domain では ODB / index / worktree を観測できず、ops 単位の unit
     test では Backend dispatch や public entrypoint を漏らすため、この layer に置く。
  2. `Operation` の exhaustive match で全 variant を fixture へ対応させ、public な
     `Backend::plan` と Backend dispatch 外の public `plan_*` entrypoint に加え、
     `Backend` 経由と直接公開された **全 public `preflight_*` entrypoint** を実行する。
     現行 inventory には `Backend::{preflight_check,preflight_check_stash}`、
     `preflight_check`、`preflight_check_stash`、`preflight_absorb`、
     `preflight_dir_file_resolution`、`preflight_restore_snapshot`、
     `preflight_apply_suggestion` がある。実装時に LSP symbols / references で取り直し、
     symbol の追加時に coverage が落ちれば失敗させる。成功・blocker・error のどの場合も、
     前後で ODB / index / refs / gitdir・commondir / worktree を比較する。file は
     `(relative path, size, mtime, mode, SHA-256)`、directory は
     `(relative path, mtime, mode)` を比較し、内容が同じ touch / chmod / rename も差分に
     する。ODB はこれに全 OID set と object file 数を加える。新しい `Operation` variant
     は match が非網羅になり、test 更新なしでは compile できない形にする。
  3. detector 自体の self-test として、test fixture から sentinel blob を 1 個書き、
     object 差分を **1/1 回検出する**ことを assert する。さらに内容不変の touch と mode
     変更も各 **1/1 回検出する**ことを assert する。
  4. `ci/` の `Rule` 候補は、既知の書き込み API の plan 内 direct / helper 経由と、
     preflight 内 direct / helper 経由の **4 sample** で評価する。現行 `Rule` は
     ファイル全体への regex で、Rust の nested block と call graph を解釈しないため、
     helper 経由を証明できない。4 sample 全てを捕捉できない限り、静的 Rule は主
     enforcement に採らない。採る場合も direct call の defense-in-depth と明記し、
     dynamic test を置き換えない。

  ```sh
  cargo test -p kagi --test prediction_non_write_test
  uv run --project ci check-all
  ```

- **値**: plan / preflight entrypoint coverage（対象 / 全数）、bytes / path / mtime / mode
  ごとの前後差分件数、sentinel / touch / mode 検出数、static Rule の direct / helper
  sample 検出数、現行 source の false positive 数。
- **判断基準**: dynamic test は plan / preflight coverage **100%**、通常経路の差分
  **0 件**、sentinel / touch / mode 検出が各 **1/1** 必須。1 つでも欠ければ規則を
  実装完了としない。static Rule は 4 sample **4/4**、false positive **0 件**のときだけ
  追加する。それ以外は call graph を扱えないため不採用と記録する。
- **交絡**: linked worktree の private gitdir / commondir、git auto-maintenance、
  index stat cache、error 経路で一時作成後に削除される object 以外のファイル。
- **決まらないこと**: どちらの backend を選ぶか。C4 は決定済み規則の維持だけを担う。

検知主体は local の当該 test と、PR ごとに `cargo test --workspace` を実行する CI。
規則違反は test failure として変更者・reviewer の両方に届く。

### D. 実行経路で libgit2 が遅い／危ういもの

#### D1 — 「未変更ファイルを読み直す」形の洗い出し
- **問い**: `stash_save2` と同じ形の API が他にあるか。

- **手順**:
  1. `crates/kagi-git/src/ops/`、`staging.rs`、`backend/` にある libgit2 の
     worktree / index 書き込み API を LSP references で 1 行ずつ inventory 化する。
  2. 各 API について「入力に workdir / index を渡すか」「pathspec を限定できるか」
     「内部で status / diff を作るか」を libgit2 1.9.1 の API 文書で分類する。同じ操作の
     CLI 実装は B の該当 scenario で意味論に合格したものだけ性能比較へ進める。
  3. 変更ファイル数を 2 に固定し、未変更 tracked ファイルを
     1k / 5k / 20k / 50k と増やす。同じ pristine fixture から libgit2 と合格済み CLI
     実装を次の形で測り、両 backend の傾きと L の median を比較する。

     ```sh
     ./target/release/examples/backend_probe \
       --repo "$FIXTURE" --operation "$OPERATION" --backend libgit2 \
       --iterations 11 --format json
     ./target/release/examples/backend_probe \
       --repo "$FIXTURE" --operation "$OPERATION" --backend cli \
       --iterations 11 --format json
     ```

     最低対象は `stash_apply` / `stash_drop`（stash push 以外は現在も libgit2）、
     staging の index 書き込み、`ops/checkout.rs`、`ops/discard.rs`、
     `working_tree_status`、`diff_index_to_workdir`。inventory で見つかった API を
     この列挙より優先し、候補を黙って落とさない。
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
- **手順**: D1 で傾きが出た API と、B で意味論に合格した対応 CLI 実装を同じ pristine
  fixture で測る。Linux は次の形で open / read の syscall 数を取り、macOS は
  `fs_usage`、Windows は Process Monitor の
  `Operation is ReadFile` / `Path begins with <fixture>` filter で同じ値を取る。

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
  最低版になる。`backend_probe --operation cli-capability` に git executable を渡し、
  git 不在 / 2.37 / 2.38 / 2.50.1 / 最新安定版を各 3 回実行する:

  ```sh
  ./target/release/examples/backend_probe \
    --operation cli-capability --git-executable /missing/git --iterations 3 --format json
  ./target/release/examples/backend_probe \
    --operation cli-capability --git-executable "$GIT_237" --iterations 3 --format json
  ./target/release/examples/backend_probe \
    --operation cli-capability --git-executable "$GIT_238" --iterations 3 --format json
  ```

- **値**: 機能ごとの導入版、算出した最低版、各版の exit class、Kagi での oplog と
  モーダルの有無。
- **判断基準**: **最低版の上限を 2.38 とする。** これを超える版を要求する規則は採らない
  （代替コマンドを使うか、その操作を libgit2 に残す）。最低版を宣言する場合は、起動時
  検出と、git 不在 / 旧版を oplog + モーダルで通知することを実装条件に含める。
- **交絡**: ディストリの古い git、macOS 同梱の Apple Git の版差、企業環境の固定版、
  `PATH` 上の別 executable。
- **決まらないこと**: 最低版を満たす環境での速度・意味論。それぞれ A・B で決める。

#### E3 — hardening 面の拡大

- **問い**: CLI 経路を増やすと ADR-0146 の対策範囲は足りるか。
- **手順**: 新たに CLI へ寄せる候補コマンドについて、repo-local config で挙動を変え
  られる項目（`core.*` の外部コマンド系、`filter.*`、`diff.external`、`credential.*`
  など）を列挙し、`run_git` の既存対策で覆えているかを 1 件ずつ突き合わせる。
  候補ごとの悪性 config fixture を既存 hardening test に追加し、その test file 全体を
  次で実行する:

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

- **問い**: OS 差で規則を変える必要があるか。
- **手順**: macOS で候補を絞った後、§7 の採用判断に根拠として使う**全ての実行可能な
  決定実験**を Windows と Linux でも、同じ fixture seed・probe commit・backend 候補で
  再実行し、macOS と並べる。対象は候補に関係する A0–A3、B、C2–C4、D1–D2、E1–E3、F。
  C1 の source inventory と E2 の一次資料による導入版調査は同一 commit に対して 1 回
  行い、OS 固有の capability 実行だけ 3 OS で行う。
- **値**: OS・実験ごとの規定値、canonical JSON 不一致件数、各閾値への pass / fail。
- **判断基準**: **OS ごとに backend を変える規則は採らない。** 採用候補に関係する決定
  実験が 3 OS で揃わなければ決定を保留する。3 OS のうち最悪の結果で判定し、ある OS
  だけでも意味論・安全性・性能閾値を割るなら、その操作は libgit2 に残す。
- **交絡**: 改行変換（B2）、パスの大文字小文字、パス長制限、権限モデル。
- **決まらないこと**: 各 OS で個別最適な backend。OS 別分岐を採らないため測らない。

理由: OS 別分岐は plan / verify の意味論を OS ごとに分けることになり、ADR で管理する
規則としては維持できない。採用に使う実験を全 OS で揃え、最悪値で決める。

### F. 混在のコスト（追加）

- **問い**: family 内で backend が分かれるとき、規則の単位は family か操作か。
- **手順**: 既に混在している stash family（push だけ CLI、apply / pop / drop は
  libgit2）を実例として、plan と execute の前提が食い違う条件を列挙する。少なくとも
  「libgit2 の予測が通ったのに CLI の実行が別の結果になる／逆」の fixture を
  `backend_fixture --scenario mixed-stash` で作り、各条件を 3 回再現する:

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

| 操作群 | 区分 | 現行 | 決定 | 根拠実験 |
| --- | --- | --- | --- | --- |
| `working_tree_status` | 読み | libgit2 | — | A0 / A1 / A3 / E1 |
| `snapshot` | 読み | libgit2 | — | A0 / A2 / E1 |
| merge 予測 | 予測 | libgit2 | — | C1 / C2 / C3 / C4 |
| cherry-pick・revert 予測 | 予測 | libgit2 | — | C1 / C2 / C3 / C4 |
| 三方向の内容 merge | 予測 | libgit2 | — | C1 / C2 / C3 / C4 |
| diff 各種（tree / index / workdir） | 予測・読み | libgit2 | — | C1 / C2 / C4 / B1 / B2 |
| stash push | 実行 | CLI（#622/#623） | 据え置き（回帰させない） | B8 |
| stash apply / pop / drop | 実行 | libgit2 | — | B8 / D1 / D2 / F |
| rebase・sequencer continue | 実行 | CLI（ADR-0131） | 据え置き | — |
| fetch / pull / push | 実行 | CLI | 据え置き | — |
| switch / checkout | 実行 | 混在 | — | B5 / D1 / F |
| discard | 実行 | libgit2 | — | B3 / B5 / B8 / D1 |

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
   public preflight entrypoint の coverage 100%、通常経路前後の ODB・index・refs・
   gitdir / commondir・worktree の bytes / path / mtime / mode 差分 0、sentinel object /
   touch / mode 変更の検出各 1/1 を固定する。次の test file 全体を実行する:

   ```sh
   cargo test -p kagi --test prediction_non_write_test
   ```

   静的 `ci/ Rule` は C4 の plan / preflight の direct / helper sample **4/4**・
   false positive 0 の基準を満たす場合だけ defense-in-depth として追加する。現行 regex
   Rule は call graph を追えないため、満たさなければ追加せず、その理由を ADR に残す。
6. B8 の stash / backup recovery handle は、記録 OID の object 存在、ref、復元後
   worktree / index hash を統合テストで固定する。文字列が残るだけの test では不可。
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
