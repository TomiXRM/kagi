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
   呼び出しは `merge_commits` 17 / `diff_tree_to_tree` 17 / `diff_tree_to_index` 10 /
   `diff_index_to_workdir` 7 / `cherrypick_commit` 7 / `diff_tree_to_workdir` 6 /
   `merge_file` 4 / `merge_trees` 3 = 計 **71 箇所**。
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

- 予測 API 名の出現箇所を module 別に集計すると下表になる。合計は doc comment や
  wrapper 関数名を含む出現数で、§2-1 の呼び出し箇所数とは一致しない（同名の doc
  comment が 2 件、`crates/kagi-domain/src/plan_note/{pull,merge}.rs` にある）。
  実験 C1 は**呼び出し箇所の列挙から始める**こと。出現数だけで表を埋めてはいけない。

  | module | 出現する予測 API |
  | --- | --- |
  | `crates/kagi-git/src/resolution.rs` | `merge_file` |
  | `crates/kagi-git/src/ops/cherry_revert.rs` | `cherrypick_commit`, `diff_tree_to_tree` |
  | `crates/kagi-git/src/ops/merge.rs`, `ops/merge_into.rs` | `merge_commits` |
  | `crates/kagi-git/src/ops/pull.rs`, `ops/pull_conflict.rs` | `merge_commits`, `merge_file`, `diff_tree_to_tree`, `diff_tree_to_index` |
  | `crates/kagi-git/src/ops/pr_conflict.rs` | `merge_commits`, `merge_file` |
  | `crates/kagi-git/src/ops/stash.rs` | `merge_commits`, `merge_trees` |
  | `crates/kagi-git/src/ops/absorb.rs`, `ops/squash_merge.rs`, `ops/checkout.rs`, `ops/mod.rs` | 各種 diff、`cherrypick_commit` |
  | `crates/kagi-git/src/{diff,diffstat,staging,conflicts,message_gen}.rs`, `backend/stash.rs` | 各種 diff、`merge_trees` |

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

- 50 ms: watcher 1 回の処理として UI 応答への寄与が小さいと扱う境界。
- 150 ms: backend 移行の恒久コストを正当化する、知覚可能な絶対差。
- 200 ms: watcher 起因の reload が操作の連続性を阻害し始める優先対応境界。
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

#### A0 — watcher tick あたりの実測コスト

- **問い**: 読み経路の総コストは、backend 差が UI の待ち時間に占める割合を判断できる
  規模か。
- **手順**: `KAGI_BENCH_READ=1` の診断出力で reload 1 回あたりの `snapshot` /
  `working_tree_status` の時間を取り、M と L で watcher 発火 20 回分の分布を得る。
  fixture の tracked file を 1 個だけ更新して watcher を 20 回発火させ、各回は reload
  完了を待ってから次を更新する。測定 PR の GUI E2E scenario
  `backend_read_watcher` を次で単独実行する:

  ```sh
  KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY='backend_read_watcher' \
    cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
  ```

- **値**: ms（median / p95）、reload 1 回あたりの内訳（%）。
- **判断基準**: L の p95 < 50 ms → 読み経路の移行優先度を下げる。p95 ≥ 200 ms →
  読み経路を最優先候補にする。**A1・A2 はいずれの場合も実施する**（依頼 A の
  libgit2 / CLI 比較を省略しない）。
- **交絡**: watcher の debounce 間隔、UI スレッドとの競合、背景タブの読み。
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
  - 絶対差が **50 ms 未満** → libgit2 据え置き（プロセス起動と parse の恒久コストを
    買う理由がない）。
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
- **判断基準**: 合計が libgit2 の **0.7 倍以下**かつ情報等価 → CLI 候補。**1.0 倍超**
  → libgit2 据え置き確定。0.7–1.0 倍 → 全置換はせず、最も重い段だけを CLI に寄せる
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
path。時刻、表示順、backend 固有メッセージは比較対象から除く。**生の porcelain と
Rust struct を直接 byte compare しない。**

scenario 生成コマンド:

```sh
for scenario in clean-smudge autocrlf ignore submodule sparse partial-clone unrelated-histories; do
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
  内容 hash、(iii) plan の blocker 型または対象 path のいずれかを変える → 差異件数
  **1 件以上で不合格**。その経路は git 本体に合わせる（CLI に寄せる、または
  libgit2 側で同じ設定を再現できることを実証する）。
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

B7 を足す理由: 既に製品仕様（blocker）の根拠になっている差異なので、根拠が実測で
正しいことは規則の前提である。誤っていれば blocker 側を直す作業が発生する。

- **値**: scenario / OS / git 版ごとの canonical JSON 不一致件数、内容 hash、
  blocker 型、不一致が到達する Kagi 機能。
- **交絡**: filter executable、global config、改行、path の大小文字、欠落 object の
  lazy fetch。probe は `GIT_CONFIG_GLOBAL` と `GIT_CONFIG_SYSTEM` を隔離し、fixture
  manifest に必要な config だけを再現する。
- **決まらないこと**: 速度。B は正しさだけを見る。

### C. 予測 API の代替可能性

**ここが規則の骨格。** 71 箇所の用途それぞれについて、CLI で書かずに同じ情報が取れるかを
確定させる。取れないものは取れないと明記する。

#### C1 — 用途ごとの代替可能性表

- **問い**: 各予測用途に、object を増やさず index も worktree も変えない CLI 手段が
  あるか。
- **手順**:
  1. まず**71 呼び出し箇所を 1 行ずつ列挙する**。LSP references で API ごとに引き、
     `api / file:line / 呼び出し元 symbol / Kagi 用途 / 必要な戻り値` の CSV を作る。
     行数が API 別に 17 / 17 / 10 / 7 / 7 / 6 / 4 / 3、総数 71 と一致しなければ
     C1 を開始しない。comment、wrapper 定義、test helper は行数に含めない。
  2. 用途を群にまとめる。群は最低でも次の 8 つ: merge 予測 / cherry-pick・revert 予測 /
     三方向の内容 merge / tree↔tree diff / tree↔index diff / index↔workdir diff /
     tree↔workdir diff / tree merge。
  3. 各 CSV 行に CLI 候補 argv と予想する出力 schema を記し、C2 のハーネスで
     **書くかどうか**、C3 で watcher を実測する。
  4. 元 API と CLI 候補の canonical JSON を conflict fixture（clean / text conflict /
     binary / symlink / file-directory / rename-rename）で各 3 回比較する。
     情報の欠落を書く。「速いが情報が足りない」は代替ではない。
- **値**: 71 行それぞれの Yes / No / 条件付き、欠落する field 数、必要な git 最低版。
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
  1. loose object 数と pack 内 object 数、
  2. `.git/index` の mtime と内容ハッシュ、
  3. `packed-refs` と `refs/` 配下全ファイルの内容ハッシュ、
  4. 作業ツリー全ファイルの (path, size, mtime)、
  5. `.git/` 直下の一時ファイル（`*.lock`、`MERGE_*`、`CHERRY_PICK_HEAD` など）の有無。
  各候補コマンドを 3 回実行し（初回だけ書くケースを捕まえるため）、毎回比較する。
  `--no-optional-locks` の有無でも比較する（§4.5）:

  ```sh
  ./target/release/examples/backend_probe \
    --repo "$FIXTURE" --operation prediction-side-effects --backend cli \
    --candidate "$CANDIDATE" --iterations 3 --format json
  ```
- **値**: 項目ごとの delta（object 個数、変化したファイル数）。
- **判断基準**: **object delta 0 かつ index 不変 かつ worktree 不変 かつ 一時ファイル
  なし**のみ「予測に使える」。1 項目でも動けば予測不可（実行経路の候補にはなり得る）。
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

### D. 実行経路で libgit2 が遅い／危ういもの

#### D1 — 「未変更ファイルを読み直す」形の洗い出し
- **問い**: `stash_save2` と同じ形の API が他にあるか。

- **手順**:
  1. `crates/kagi-git/src/ops/`、`staging.rs`、`backend/` にある libgit2 の
     worktree / index 書き込み API を LSP references で 1 行ずつ inventory 化する。
  2. 各 API について「入力に workdir / index を渡すか」「pathspec を限定できるか」
     「内部で status / diff を作るか」を libgit2 1.9.1 の API 文書で分類する。
  3. 変更ファイル数を 2 に固定し、未変更 tracked ファイルを
     1k / 5k / 20k / 50k と増やして、分類した各 API を次の形で測る。

     ```sh
     ./target/release/examples/backend_probe \
       --repo "$FIXTURE" --operation "$OPERATION" --backend libgit2 \
       --iterations 11 --format json
     ```

     最低対象は `stash_apply` / `stash_drop`（stash push 以外は現在も libgit2）、
     staging の index 書き込み、`ops/checkout.rs`、`ops/discard.rs`、
     `working_tree_status`、`diff_index_to_workdir`。inventory で見つかった API を
     この列挙より優先し、候補を黙って落とさない。
- **値**: API ごとの傾き（ms / 未変更 1k files）、切片（ms）。
- **判断基準**: 傾き **≥ 0.5 ms / 1k files** → 「`stash_save2` 型」に分類し CLI 等価物を
  検討。**< 0.1 ms / 1k files** → 据え置き。中間は D2 で裏を取る。
- **交絡**: 未変更判定に使う stat cache の状態、ファイルサイズ分布（1 MiB blob を
  含めること）、ファイルシステム、他プロセスの I/O。
- **決まらないこと**: 遅い理由。それは D2。

#### D2 — 分類の裏取り（未変更 path への読み取り回数）

- **問い**: 傾きの原因は本当に「未変更ファイルの読み直し」か。
- **手順**: D1 で傾きが出た API だけを対象にする。Linux は次の形で open / read の
  syscall 数を取り、macOS は `fs_usage`、Windows は Process Monitor の
  `Operation is ReadFile` / `Path begins with <fixture>` filter で同じ値を取る。

  ```sh
  strace -f -c -e trace=openat,read -- \
    ./target/release/examples/backend_probe \
    --repo "$FIXTURE" --operation "$OPERATION" --backend libgit2 \
    --iterations 1 --format json
  ```

- **値**: open 回数、read 回数、read バイト数、未変更ファイル数との比。
- **判断基準**: file 数 1k→50k で open または read 回数が **0.9 以上の相関係数**で
  増え、かつ未変更 1 file あたり **0.9 回以上** → 「読み直し」と分類確定。
  どちらか未満 → 別原因なので CLI 化では直らないと判定する。
- **交絡**: トレース自体のオーバーヘッド、libgit2 内部のキャッシュ、OS の readahead。
- **決まらないこと**: CLI 化で直るか。D2 は原因の分類だけを確定する。

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
- **手順**: A1・B2・B3・E1 を Windows と Linux で同じ fixture seed と probe commit
  で再実行し、macOS の結果と並べる。
- **値**: OS ごとの ms と canonical JSON 不一致件数。
- **判断基準**: **OS ごとに backend を変える規則は採らない。** 3 OS のうち最悪の
  結果で判定する。ある OS だけで閾値を割るなら、その操作は libgit2 に残す。
- **交絡**: 改行変換（B2）、パスの大文字小文字、パス長制限、権限モデル。
- **決まらないこと**: 各 OS で個別最適な backend。OS 別分岐を採らないため測らない。

理由: OS 別分岐は plan / verify の意味論を OS ごとに分けることになり、ADR で管理する
規則としては維持できない。最悪値で決めるほうが安い。

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
- **判断基準**: 食い違いが 1 件でも再現する → **予測と実行で backend が異なる family
  には verify 段を必須**とする（`verify_X` で実行後の状態を読み直して plan の前提と
  突き合わせる）。0 件 → family 内分割を無条件に許可する。
- **交絡**: 実行時の repo 状態変化（TOCTOU）、oplog に残る記録の形。
- **決まらないこと**: どちらが速いか・正しいか。F は「分けたときに何を足す必要があるか」
  だけを決める。

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
| C1 | — | — | — | — |
| C2 | — | — | — | — |
| C3 | — | — | — | — |
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
| merge 予測 | 予測 | libgit2 | — | C1 / C2 / C3 |
| cherry-pick・revert 予測 | 予測 | libgit2 | — | C1 / C2 / C3 |
| 三方向の内容 merge | 予測 | libgit2 | — | C1 / C2 / C3 |
| diff 各種（tree / index / workdir） | 予測・読み | libgit2 | — | C1 / C2 / B1 / B2 |
| stash push | 実行 | CLI（#622/#623） | 据え置き（回帰させない） | — |
| stash apply / pop / drop | 実行 | libgit2 | — | D1 / D2 / F |
| rebase・sequencer continue | 実行 | CLI（ADR-0131） | 据え置き | — |
| fetch / pull / push | 実行 | CLI | 据え置き | — |
| switch / checkout | 実行 | 混在 | — | B5 / D1 / F |
| discard | 実行 | libgit2 | — | B3 / B5 / D1 |

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

5. 予測経路を触った場合は、C2 の拡張 fingerprint による非書き込み回帰テストを
   `tests/` に足す（#625 の再発条件を直接押さえる）。
6. backend を移した操作には `plan_/preflight_/execute_/verify_` の整合と、oplog への
   記録が残ることを確認する統合テストを足す。
7. 一度に走らせる cargo コマンドは 1 本（AGENTS.md の build hygiene）。

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
- [Issue #625](https://github.com/TomiXRM/kagi/issues/625) — plan 中の書き込みによる
  watcher / reload / modal 消失
- [`docs/adr/0131-sequencer-continue-and-rebase-current-onto.md`](../adr/0131-sequencer-continue-and-rebase-current-onto.md)
- [`docs/adr/0146-run-git-hardening.md`](../adr/0146-run-git-hardening.md)
- [`crates/kagi-git/src/status.rs`](../../crates/kagi-git/src/status.rs)
- [`crates/kagi-git/src/snapshot.rs`](../../crates/kagi-git/src/snapshot.rs)
