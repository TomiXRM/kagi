# ADR-0214: Operation Log パネルを一級ビューにする — 全体設計、slice 1(読むだけ)、slice 2a(ref 移動の記録)、slice 2b(op revert / restore to point)、slice 2c(実行前のグラフプレビュー)

- Status: **Accepted**(slice 1、2a、2b-1 = backend、2b-2 = UI、2c = プレビュー)
- Date: 2026-10-01
- Related: [#334](https://github.com/TomiXRM/kagi/issues/334)、#333 / ADR-0149(actor・worktree・id/parent)、ADR-0081 / ADR-0084(undo)、ADR-0111(`OpLogPanel`)、#468 / #548(行の展開・コピー)

## 文脈

Kagi の操作はすべて oplog を通るが、UI では bottom panel の Operation Log tab が時刻・op 名・outcome を並べるだけで、誰が(人 / MCP / CLI)、どの worktree で行い、その操作で ref がどう動いたかが見えない。#334 はこの一覧を「行を選んで時点に戻す / 1 操作だけ取り消す / 実行前にグラフで見る」入口にすることを求めている。

`OpLogEntry` の `before` / `after` は表示用の文字列(`"branch: main"` など)で、object id を持たない。

## 決定

### 1. 全体設計(#334 §4)

- **パネル**: 既存の `OpLogPanel`(`src/ui/oplog_panel.rs` + `oplog_render.rs`、bottom panel の Operation Log tab)を強化する。置換や別画面は作らない。
  - 1 行 = 1 操作。
  - 行に表示するもの: actor バッジ(Human / MCP / CLI)、worktree バッジ、outcome の色分け。
  - 行を選ぶと詳細(before / after / error / 対応する reflog 行)を出す。
- **3 つの操作**: いずれも `plan → confirm → preflight → execute → verify → oplog` を通し、「戻す操作」自体も oplog に載る(戻したことも戻せる)。

  | 操作 | 意味 | 実装の骨子 |
  |---|---|---|
  | restore to point | その時点の ref 状態へ戻す | checkpoint + イベント列から目標 ref 状態を再構成し、ref 更新の plan を作る |
  | revert one op | 1 操作だけ打ち消す | その操作の before / after 差分だけを逆適用する。後続操作が同じ ref を動かしていれば blocker |
  | preview | 実行前に結果のグラフを描く | in-memory でグラフを再計算し、現在のグラフと並べる |

- **confirm の内容**: 実行後グラフのプレビュー、逆操作の一覧、「できないこと」(untracked の削除は戻らない等)の明示。
- **未決**(slice 2 以降で決める): #334 §5 の論点。working tree も戻すか、`--keep` 相当、プレビューを並べるか差分ハイライトか。
  - Refused / Failed / Partial の時点の扱いは #888 で決めた: 記録どおり(何も動いていなければ `Some(空)`、Partial は実際に動いた分)。§4 の「outcome に関わらず実際の差分を記録する」に従い、outcome で分けない。`Unknown` は記録しないのでまたげない(`docs/decisions.md`)。

### 2. slice 1 の範囲: 読むだけ

- **行**
  - actor バッジ: `OpLogEntry.actor`。人は控えめ、MCP / CLI は色付き。
  - worktree バッジ: `worktree`(無ければ `repo`)のディレクトリ名。full path は tooltip。
  - outcome の色分け: 既存のまま。
- **選択した行の詳細**: 既存の before / after / error に、その操作の **reflog 行** を加える。
  - 読み取りは `Backend::reflog_lines_between`(`crates/kagi-git/src/backend/reflog.rs`)。HEAD(worktree ごと)と `refs/heads/*` の reflog を、時刻範囲で拾う。
  - 選択時に background で 1 回読む。
  - 新しい選択が来たら古い結果は捨てる(generation)。結果は entry の同一性に結びつけ、行の位置には結びつけない。
- **書かないもの**: repository への write も oplog への追記もしない。restore / revert / グラフプレビューは slice 2 以降。

### 3. reflog 行の帰属は時刻窓で決める。曖昧は明示し、前提に使わない

entry は outcome が確定した時点(ref が動いた後)に時刻を刻む。そこで、同じ worktree の直前の entry の秒を下限とする `(prev, this]` を、その操作の窓とする(`kagi_domain::oplog_reflog::ReflogWindow`)。

- **同じ worktree の判定**: 隣接する entry は同じ worktree のものに限る。別 worktree の行は飛ばす。
  - `Backend::run` は workdir を末尾 `/` 付きで記録し、他の経路は付けないので、末尾の区切りを除いて比較する。
- **同じ秒の境界**: 同じ worktree の別 entry が同じ秒に記録されていれば、その秒の reflog 行は **`Ambiguous`** として表示する(「別の操作と同じ秒 — どちらの操作のものか判別できません」)。どちらの操作にも帰属させない。
- **下限が無い場合**: 読み込まれた範囲に直前の entry が無ければ、下限を 60 秒前とし、見出しでそう明示する。
- **既知の限界**: 窓は時刻だけで決まるので、Kagi の外(terminal の git など)で同じ時間帯に動いた ref も窓に入る。見出しは「この worktree の前の操作以降の reflog」であり、因果関係を主張しない。
- **slice 2 以降への拘束**: restore / revert / preview は、この時刻窓の帰属を前提にしてはならない。特に `Ambiguous` の行を、復元先や逆適用対象の根拠にしてはならない。
- **将来の正攻法**: oplog entry に「動かした ref と old / new OID」を持たせる。→ slice 2a で実装した(§4)。時刻窓は記録の無い entry の表示補助に下がる。

### 4. slice 2a: recorded op は動かした ref を OID で記録する

- **記録点**: `Backend::run_recorded_with_events`(`Backend::run` / `run_recorded` が通る 1 箇所)と、undo / redo の `run_history_move`。
  - 実行の前後に ref を読み(`backend/reflog.rs::ref_snapshot`)、差分(`kagi_domain::ref_moves::diff`、pure)を entry の `ref_moves` に載せる。
  - 読むもの: **op を実行した worktree の HEAD**(symbolic 先 + 解決 OID)と `refs/heads/*`。
  - 他の worktree の HEAD は走査しない。replay-onto などで checkout 先の HEAD が追従しても、それは `refs/heads/<branch>` の差分から導ける(snapshot のコストを増やさない)。他の worktree の detached HEAD を記録しない決定は §7。
- **型**: `RefMove { refname, old, new: Option<OID>, old_symbolic, new_symbolic }`。`None` は ref が無かったこと(作成 / 削除)、symbolic は HEAD のみ。entry 側は `Option<Vec<RefMove>>`。
  - `None`: 記録なし。この field より前に書かれた entry、記録しない経路(下記)、前後どちらかの ref を読めなかった場合。読めない時は誤った差分より記録なしを選ぶ。
  - `Some(空)`: 記録した結果、何も動いていない。refused / 失敗 / no-op がこれにあたる。
- **outcome に関わらず実際の差分を記録する**。失敗しても動かなければ空、Partial なら実際に動いた分になる。
- **wire**: additive。
  - 欠落・`null`・不正な値は `None` として読む。行ごと捨てず、「何も動いていない」とも読まない。
  - `Some(空)` は空配列として書く。
- **repository への書き込みはない**。増えるのは oplog の 1 field だけ。
- **パネルの表示**
  - `Some` の entry は記録を「記録」として即表示する(background read なし)。
  - `None` の entry は §3 の時刻窓を「推定」として表示する。見出しの色と文言で区別する。
- **既知の限界**
  - 前後の snapshot の間に、外部の process が動かした ref も差分に入る(op の実行中だけの窓。時刻窓よりはるかに狭い)。
  - **#885: fetch・GitHub 書き込み・worktree の lock などを記録に加えた**。判定は 2 通りで、推定で `Some(空)` にはしない。
    - **local git を動かす経路は観測する**(`Backend::observe_ref_moves`)。動かさないはずでも、構造を根拠にせず実際の前後を記録する。
      - fetch(`fetch_async_for`、argv `fetch --prune -- <remote>` / `fetch --all --prune`): 書き込み先は `remote.<name>.fetch` の refspec が決める。mirror 型の refspec(`+refs/heads/*:refs/heads/*`)なら同じ argv で local branch も動くので、観測が要る。成功した fetch はこれまでどおり記録しない(local branch が動けば、restore は記録外の reflog 変化として RefChangedOutsideRecord で止まる)。失敗(`FetchFailure.ref_moves`)は観測した移動を記録する。repository を開けなかった場合は何も動いていないので `Some(空)`。
      - PR の fetch(`fetch-pr`、refspec は `refs/remotes/**` と `refs/kagi/pr/**`): `fetch_pr_refs` の呼び出しだけを観測し、失敗の記録に載せる。その後の commit / diff の解析は観測の外で行う(`Backend::observe_then`。#907 review)。解析中に外部で動いた branch をこの job の移動として記録すると、restore がその無関係な変更を巻き戻しうるため。
      - remove-worktree(#900 review からの追加): 試行全体を前後 snapshot で囲む。branch を残せば空、`delete_branch` なら branch の削除を記録する。実行前に捨てた job・計画の失敗は `Some(空)`。
      - #915: 削除を own tab から実行するとその worktree の `HEAD` / repo path は実行後に読めない。Remove だけは前後とも surviving common dir から main の `HEAD` と共有 `refs/heads/*` を読む。削除する worktree の `HEAD` 消滅は ref 移動としない。これにより branch 維持は `Some(空)`、branch 削除は削除された branch の OID 差分になり、RestoreToPoint がこの entry を越えられる。
      - #938 review: common dir が bare の場合も `git2::Repository::open` で直接読み取る。`Backend::open` は bare を拒否するため、ここで使うと成功した Remove の `ref_moves` が `None` になり、削除 branch の OID も失う。bare の `HEAD` を前後同じ基準で読み、`run_recorded_remove` の receipt と RestoreToPoint の往復で検証する。
    - **local ref に触れない経路は `Some(空)`**。共通の builder `OpLogEntry::with_nothing_moved`(= `with_ref_moves(Some(空))`。conflict の `nothing_moved` もこれを使う)で書くので、`Unknown` は `None` のまま。
      - worktree の lock / unlock(auto-lock の確認も同じ経路)・prune・repair: worktree の admin file だけを書く。前後の snapshot は取らず、UI の `record_op_persist_nothing_moved` で記録する(#907 review)。観測すると、実行中に外部で動いた branch がこの操作の移動として記録され、restore がそれを巻き戻しうる。
      - GitHub 書き込み: `gh pr comment|edit|review -R <repo> …`、`gh issue create|comment -R <repo> …`。argv は GitHub API だけを呼ぶ。
      - PR merge の計画時の拒否(`gh` を実行する前)。
    - **`None` のまま**の経路
      - 実行した PR merge: `--delete-branch` で local branch を消すことがあるため(backup ref も残す)。
      - Pull に付随する fetch の失敗(`record_pull_fetch_failure`): pull の経路は対象外とする。
      - UI の `record_refused`(計画時の拒否): #885 の範囲外。
  - **UI の記録の panel には採番済みの entry を渡す**(#907 review)。`record_op_impl` と PR fetch の失敗記録は `recording::finalize` が返す entry(log が振った id)を `OpLogPanel::entry_for_recording` で panel に載せる。placeholder の id 0 は別の entry を指しうるので、記録した移動で有効になる「取り消す / この時点まで戻す」を誤った操作に向けてしまう。append に失敗した entry は移動を推定扱いにする。
  - **conflict 経路は #884 で記録に加えた**。記録点は `Backend::observe_ref_moves` の 1 か所で、`Backend::run` と同じ前後 snapshot を使う。
    - save / dir-file / abort: `run_recorded_conflict` が executor をこれで包み、`record_receipt` に渡す。
    - continue / skip: UI が実行するので、UI がこれで包み、`record_op_persist_moves`(#885 で `record_conflict_persist` から改名)で記録する。
    - 実行前の拒否(`record_conflict_refusal` / `record_conflict_save_refusal`、UI の continue の計画時の拒否)・実行前に捨てた job(`conflict_abandoned`)・repository を開けなかった場合は、何も動いていないことが構造上確かなので `Some(空)` にする。
    - **終了が確認できない(`Unknown`)outcome の移動は記録しない**(#891 review)。process がまだ ref を動かしうるので、観測した snapshot は記録にならない。`OpLogEntry::with_ref_moves` が `Unknown` なら `None` にする。`record_receipt`(`Backend::run` を含む)と UI の `record_op_persist_moves` の両方がこれを通るので、restore はその entry をまたげない(fail closed)。
    - これで、Kagi の中で解いた merge(merge → save → merge-commit)や cherry-pick の時点を越えて restore できる。rebase は HEAD が detached を経由するので、引き続き HeadMoved(#886)。
  - backend の専用経路(absorb、merged branch の一括削除、oplog entry の forget)は `Backend::observe_ref_moves` で前後を観測して記録する(#871 review)。

### 5. slice 2b: op revert / restore to point(2b-1 = backend、2b-2 = UI)

- **根拠は記録だけ**: 復元先や逆適用対象は `ref_moves`(`Some`)だけを根拠にする。範囲内に `None` の entry(推定しか無い時点)があれば blocker にし、推定(§3)は使わない。
- **Operation**: `Operation::OpRevert { entry_id }`(oplog 名 `op-revert`)と `Operation::RestoreToPoint { entry_id }`(`restore-to-point`)。
  - `Backend::run` を通すので、自分の ref の移動も記録される(§4)。戻した操作もさらに戻せる。
  - triple は `ops/oplog_restore.rs`。計画は pure な `kagi_domain::ref_restore::plan`。
- **対象の範囲**
  - 同じ repository の entry(worktree の common dir が一致するもの)。branch は worktree 間で共有されるので、他の worktree の entry も含める。
  - 読むのは oplog の末尾 1000 件。target が無ければ blocker(EntryNotLoaded)。
  - **範囲は証明できなければ fail closed**(#878 review、restore to point)。planner は全 repository の entry を受け取り、次の場合を blocker にする。
    - 対象以降で `parent → id` の鎖が途切れる(HistoryGap): retention での削除、壊れた行、読めない行。
    - worktree を開けない entry(UnknownRepository): 削除・prune された worktree の entry は、この repository のものだった可能性がある。黙って除外しない。別の repository だと証明できた entry(Other)だけを除く。
  - **entry は記録時の repository を持つ**(#894)。`repo_identity` = 正規化した common dir の path、unix ではその `(dev, ino)`、取れる環境では common dir の作成時刻(`Metadata::created()`、macOS は birthtime、Linux は statx の btime。JSON は `born_s` / `born_ns`)。
    - append の 1 か所(`append_oplog_receipt`)で、entry の worktree を開いて埋める。worktree が無い、または開けない場合は repo を開く(#900 review 3)。remove-worktree の成功は、削除・prune 済みの worktree を `entry.worktree` に記録するので、repo へのフォールバックが無いと UnknownRepository になり、全 repository の restore を止める。Backend の経路も UI の persist 経路もここを通る。どちらも開けない scope(remote の `host:repo`)は `None`。
    - **backend の書き込みは "前" の snapshot と同時に読む**(#900 review 8)。`run_recorded_with_events` と `observe_ref_moves` が ref の前 snapshot と一緒に identity を読み、`record_receipt` が entry に載せる(append はすでにある identity を読み直さない)。append 時に読むと、操作の完了から追記までの間に repository が削除・再 clone されたとき、A の `ref_moves` に B の identity が付く。UI が直接記録する経路(`record_op_persist*`)は前 snapshot を Backend の外で取るので、append 時に読むまま。残る窓は、操作の完了から追記まで(ms 単位)に同じ path へ repository を作り直した場合だけ。
      - remove-worktree(`run_recorded_remove_with_events`)は `record_receipt` を通らないので、実行の前に `plan.repo` の identity を読み、entry に載せる(#900 review 10)。削除した worktree は後で開けず、append 時に読むと `plan.repo` に後から置かれた別の repository の identity になりうる。
    - 分類(worktree は開かない)。削除済みの worktree の entry は Mine になり、記録した移動は restore に入る。削除した無関係の repository の entry は Other になり、blocker にならない。

      | file id `(dev, ino)` | 作成時刻 | path | 判定 |
      |---|---|---|---|
      | 両側にあって違う | | | Other |
      | | 両側にあって違う | | Other |
      | 両側にあって一致 | 両側にあって一致(ns 部が 0 でない) | (見ない) | Mine |
      | 両側にあって一致 | どちらかが無い、または一致しても ns 部が 0 | | 曖昧 → UnknownRepository(fail closed) |
      | どちらかが無い | 両側にあって一致(ns 部が 0 でない) | 一致 | Mine |
      | どちらかが無い | | 違う | Other |
      | どちらかが無い | どちらかが無い | 一致 | 曖昧 → UnknownRepository |

      file id があれば path より優先する(#900 review)。path を優先すると、repository を消して同じ path に別の repository を作り直したとき、古い repository の entry が新しい repository の restore 地点になってしまう。file id の一致だけでも足りない(#900 review 2): 消した `.git` の inode が、同じ場所に同じ remote から clone し直した `.git` に再利用されると、path も `(dev, ino)` も一致し、`main` が同じ OID なら RefMovedSince も通って、古い repository の revert が新しい repository の branch を動かす。作成時刻はこの再利用を区別する。作成時刻を確認できない一致は Mine にせず、曖昧として fail closed にする。repository の config に UUID を書く案は、config への write になるので採らない。

      作成時刻の解像度が粗い filesystem(秒精度。HFS+ や一部の network filesystem)では、同じ秒の中で削除と再作成が起きると inode も作成時刻も再利用されうる(#900 review 4)。ns 部が 0 の作成時刻は「秒精度」とみなし、一致しても確認にならないので曖昧にする。違えば Other のまま。`.git` に再利用されない世代 ID を書く案は採らない。ユーザーが頼んでいない repository への書き込みになるため。
    - identity の無い旧形式の entry だけ、従来どおり worktree を開いて判定する(開けなければ UnknownRepository)。
    - **field はあるが読めない identity は旧形式と区別する**(#900 review 5・7・9)。codec は `RecordedIdentity::{Absent, Known, Invalid}` を返す。field が無ければ Absent。それ以外は decode 時に一括で検証し(`valid_identity`)、1 つでも違反すれば Invalid にする。
      - 型: object で、知らないキー(将来の形式)が無く、各 field の型が合う(文字列・数値・`null` は不可)。
      - `common_dir`: 空でない、この platform での絶対 path。
      - `dev` と `ino`: 両方あるか、両方無いか。
      - `born_s` と `born_ns`: 両方あるか、両方無いか。`born_ns` は 10 億未満。

      Invalid を旧形式として扱うと path を開いて判定してしまい、同じ path に作り直した repository の entry を Mine にしうる。壊れた値を Known として受けると、自分の entry が Other / Different になり、restore の範囲から黙って外れる。そのため Invalid は UnknownRepository とする(fail closed)。
    - **限界 3**: identity の無い本物の旧形式の行(#894 より前に書かれたもの)は path で判定するので、同じ path に作り直した repository では Mine になる。移行前のデータの限界で、#894 以降に書いた行には当てはまらない。
    - `(dev, ino)` も持つのは、同じ filesystem 内での移動や rename では inode も作成時刻も変わらないため。path だけだと、移動した repository の自分の entry を Other と誤判定して黙って除外してしまう。
    - **限界 1**: filesystem をまたいで移動すると、path も `(dev, ino)` も変わるので、それ以前の自分の entry は Other になる。その entry が動かした branch は、記録が説明しない reflog の変化として RefChangedOutsideRecord の blocker になる(fail closed)。対象 entry 自体が移動前のものなら EntryNotLoaded。
    - **限界 2**: 削除した無関係の repository の `.git` と、`(dev, ino)` も作成時刻(ns 単位)も一致する場合は Mine と誤判定する。APFS / ext4 / btrfs のような ns 精度の filesystem では、同じ ns の中で削除・再作成が起き、しかも同じ inode を得る場合に限られる。その記録は、この repository に無い ref や OID を期待値に持つことが多いので、RefMovedSince の blocker になる(fail closed)。作成時刻を返さない、または秒精度の filesystem では、同じ inode の entry は曖昧になり、restore はその範囲で止まる。偶然 ns 部が 0 になった作成時刻も同じ扱いになる(10 億分の 1 の確率で restore が不要に止まる)。
      - 時刻の精度は値から推定しない(#900 review 6)。ns 部が 0 なら Ambiguous にするのは、明らかに粗い場合だけに足した fail closed で、精度の証明ではない。FAT(作成時刻は 10 ms 単位)や Windows(file id を持たない)では、同じ tick の中に同じ path で作り直した repository を区別できない。
  - append に失敗した操作は鎖に現れない(次の entry の parent は最後に書けた entry)。その操作が動かした branch は、RefMovedSince か RefChangedOutsideRecord で捕まる。
- **revert**: 対象 entry の各 branch の移動を old に戻す(作成は削除、削除は作り直し)。同じ ref を後続の記録済み entry が動かしていれば blocker(LaterEntryMoved)。
- **restore to point**: 対象 entry の**直後**の状態に戻す(jj の `op restore` と同じく、対象自身の効果は残す)。それより新しい entry を古い順に合成し、ref ごとに「最古の old を戻し先、最新の new を期待値」とする。動いて元に戻った ref は除く。
- **HEAD**
  - branch に追従しただけの HEAD の移動(commit など、symbolic が同じ)は、その branch の移動として扱う。
  - HEAD の切替・detached を含む entry は blocker(HeadMoved)。戻すには checkout(作業ツリーに触れる)が要るため、対象外とする(§7 で確定)。
- **その他の blocker**
  - 範囲の記録が触れた ref が期待値に無い(RefMovedSince。記録外での移動)。動いて元に戻ったため戻さない ref も、今その値にあることを確かめる。
  - restore to point: 対象 entry の記録時刻より後に reflog の更新がある local branch のうち、範囲の記録が一度も動かしていないもの(RefChangedOutsideRecord。terminal での `git branch` など)。戻しても残るので、「その時点の branch 状態」にならない。
    - 限界: reflog を書かない設定(`core.logAllRefUpdates=false`)や、記録外で削除された branch(reflog ごと消える)は検出できない。対象 entry と同じ秒の記録外の変更も検出できない。
  - 戻し先の commit が既に無い(TargetGone)。
  - merge / rebase などが**いずれかの worktree で**進行中(OperationInProgress、path 付き)。その worktree が checkout している branch を動かすと、進行中の操作の前提の HEAD が変わる。
  - checkout 中の branch を削除することになる(DeletesCheckedOutBranch)。
- **checkout 中の branch を動かす場合**: soft な移動(ADR-0084 の undo と同じ)で、その worktree の index とファイルはそのまま。
  - 変更が無ければ warning MovesCheckedOutBranch、未コミットの変更があれば warning CheckedOutDirty(変更は残り、戻した先端との差分と一緒に見える)。
  - 設計時は dirty を blocker にしていたが、2b-1 の実装中に変更した。checkout 中の branch を restore すると、その worktree は必ず「dirty」になる(restore 由来の差分)。dirty を blocker にすると、その restore を revert できなくなり「戻したことも戻せる」が成り立たない。
- **plan / preflight / execute**
  - plan: 戻す内容を `restore <ref> <to|-> <expect|->` の行として載せる(`preview_commits`)。destructive で二段 confirm。warning に、ref ごとの逆操作(Moves)と「戻らないもの」(RefsOnly: 作業ツリー / index / untracked / stash / tag / remote branch)を出す。
  - preflight: 再計画して、行が確認したものと一致すること。
  - execute: 動かす全 ref の現在の先端を `refs/kagi/backups/<op>/<i>` に保持 → `git update-ref --stdin` の 1 トランザクション(`update` / `create` / `delete` に old 値を付け、git が再照合)→ 各 ref を verify。
  - 結果は `OperationOutcome::OplogRestore`。backup は recovery handle(BRANCH_TIP)になる。
- **2b-2(UI)**
  - panel の選択行に「この操作を取り消す…」「この時点まで戻す…」を出す。押すと panel が `OpLogPanelEvent::Restore(Operation)` を出し、app が active な repository で plan して card(`ActiveModal::OplogRestore`)を開く。
  - 記録なしの行(`ref_moves = None`)では両方 disabled にし、理由を出す。
  - card は shared plan card で、warning の Moves(ref ごとの逆操作)と RefsOnly(戻らないもの)を描画する。script 行(`preview_commits`)は commit 一覧として描かない。
  - destructive なので、最初の confirm(button / Enter)で arm し、二度目で `run_recorded`。CLI / MCP には出さない(#888 で確定。理由は `docs/decisions.md`)。
  - **記録できなかった receipt は戻す対象にしない**: append に失敗した entry は panel 上で id が placeholder(0)になる。oplog の id は 0 始まりなので、0 は最初の実 entry も指しうる。そこで `entry_for_recording` は `Recording::Failed` の `ref_moves` を落として「記録なし」(推定表示・ボタン disabled)にする。
  - 別 repository の行を選んだ場合、plan は EntryNotLoaded(「この repository の操作にない」)になる。
  - **読む範囲は oplog の末尾 1000 件**(`ENTRY_SCAN`)。対象がそれより古い場合、範囲を広げず EntryOutsideWindow(「直近 1000 件より古い」)で拒否する(#888)。対象 id が末尾に無いときだけ、log 全体を 1 回だけ後ろから走査して id の有無を確かめる。見つからなければ EntryNotLoaded のまま(retention で消えた、など)。
    - 見つかった entry は、末尾の entry と同じ規則(`classify_entry`。記録した identity があれば #900 の `same_repository`、無ければ worktree を開く)で帰属を判定し、この repository のもの(Mine)の場合だけ EntryOutsideWindow にする(#910 review)。別の repository の古い entry を「この repository の古い操作」と案内しないため。それ以外は EntryNotLoaded のまま。

### 6. slice 2c: 実行前のグラフプレビュー(表示専用)

- **入力**: tab が読み込み済みの rows(id + parents、topo 順)、`branch_targets`、plan の `restore` 行だけを使う。repository は読まず、plan の一部でもない(書き込みなし、preflight にも関わらない)。
  - branch Solo 中は、表示用に絞った `rows` ではなく、退避してある全行(`branch_solo.saved_rows`)を使う。Solo は表示の絞り込みにすぎない(#883 review)。
- **計算**(pure、`kagi_domain::restore_preview`)
  - 戻した後の local branch の先端と、動かない根(remote branch / tag / Kagi が fetch した PR head `refs/kagi/pr/**` / detached な worktree の HEAD / stash の base)から到達できる commit を求める。attached な HEAD は branch に追従するので根にしない。PR head は snapshot が graph の根として読み込んでいるので、`RepoSnapshot.pr_heads` として read model に渡す(#883 review)。
  - 戻す前にどの根からも到達しなかった行(読み込み範囲の外を指す ref など)は消さない。
  - 残った行は読み込み順の部分列、すなわち topo 順なので、既存の `graph::layout()` をそのまま使う(新しいレイアウトは書かない)。
  - 消える行数 = 戻す前は到達し、戻した後は到達しない行。
- **範囲**: 戻し先の行と、消える行が下がっていた行(その下の最初の残る行)を含む最小区間に、前後 4 行を足す。上限 40 行で、窓の外の行数は「… ほか N 行」と出す。lane は最大 8 本分の幅で切る(既存の graph 列と同じく clip)。
  - card の body はスクロールしない(modal の規則)ので、行は自分の高さ上限(`modal_list_max_h`)付きのスクロール領域に入れる。40 行 × 29px は通常の窓に収まらない(#883 review)。
- **推定しない**: 戻し先 commit が読み込み済み rows に無い場合(削除した branch の作り直し、restore で branch から外れた commit への revert など)は `NotLoaded` とし、「プレビューできません」と明示する。復元そのものは可能。見出しも消える数を言わない中立な「戻した後のグラフ」にする(#883 review。以前は「消える commit はありません」と出て矛盾していた)。
  - 文言で読み込みの手段を案内する(#888): commit 一覧が途中までなら「commit をさらに読み込む」で表示できることがある。どの branch・tag・remote branch からも届かない commit(削除した branch の先端など)は表示されない。祖先を card が読み足す案は、Tier B(#908)で頻度が分かるまで採らない。
- **描画**: commit graph と同じ `graph_view::graph_canvas` を行ごとに使う。card への差し込みは `render_plan_modal_wrapper_extra`(`PlanCardExtra { element, clipboard }` を warning の後に描く)。`Copy all` は card のテキストにこの clipboard(見出し・各行・窓外の行数、または NotLoaded の理由)を足す(#883 review)。既存の `wrapper_styled` / `wrapper_staged` は `None` で委譲するので、呼び出し元の署名は変えない。この card は #872 の `ConfirmStage`(Unarmed / Armed)にも乗る。
- **focus**: card を開くとき root に focus を移す(`focus_root_for_modal`、plan modal の規約)。実ボタンのクリックは root(`track_focus`)が focus を受けるので、現状の入口では Enter / Escape は届いている。キーボードの入口が増えても届くようにするためのもの(#878 review)。
- **コスト**: card を開くときに 1 回だけ計算して modal に保持する。到達計算は O(rows)、layout は O(rows × lanes)。描画は最大 40 行。

### 7. HEAD を含む restore は行わない(#886)

設計原則として、単純で正しい方を既定にする。restore に checkout を合成すると、作業ツリーに触れる経路が restore に入る。dirty の扱い(blocker か持ち越しか)、restore 自身の revert の往復(HEAD を元に戻す checkout も要る)、plan card の表示(作業ツリーが変わることを示す)がすべて増える。そこで次の 3 点を決める。

1. **HEAD の切替を含む範囲は戻さない**。restore / revert は branch だけを動かし、HEAD は動かさない。branch に追従しただけの HEAD の移動(symbolic が同じ)は、これまでどおりその branch の移動として扱う。
   - blocker HeadMoved は、どの entry で HEAD が何から何に切り替わったかを示す(`HeadMoved { id, op, from, to }`、`HeadAt::{Branch, Detached, Unknown}`、entry の `ref_moves` の HEAD の行から作る)。
   - 手順は 1 種類だけ示す: 「<from> を自分で checkout してから、#id 以降の時点へ restore する」。手で checkout すれば作業ツリーの扱いはユーザーが checkout で決め、restore は branch の移動だけで済む。
   - **それより複雑な場合は手で戻す、と明記して分岐を増やさない**(#912 review)。複数回の HEAD の切替、同じ entry での branch の作成・削除(作成して checkout など)、rename、その後に削除された <from> などに個別の手順を出すと、案内そのものが状況の判定になり、誤った断定(「branch が無いだけなので restore で戻る」など)を生む。一度は `also_moved` / `from_gone` で分岐を足したが、この理由で削除した。
   - **切替が別の worktree で記録されていれば、その worktree を示す**(#912 review 3)。案内を active な worktree でそのまま実行すると、別の worktree を切り替えてしまう。`RecordedEntry.worktree`(entry の worktree、無ければ repo)を `HeadMoved.worktree` に載せ、restore を計画した worktree と同じなら消す。EN / JA の文言は「<path> の worktree で」を入れる。
2. **detached HEAD は戻し先にしない**。restore は commit への checkout を行わない。1 と同じく、HEAD を detached にする・detached から戻す entry は HeadMoved。
3. **他の worktree の detached HEAD は記録しない**。記録は op を実行した worktree の HEAD と `refs/heads/*` だけ(§4)。attached な HEAD は `refs/heads/<branch>` の差分から導ける。detached HEAD は restore が動かさないので、記録しても使い道がない。snapshot のコストも増やさない。

要望が出たら別 issue で再検討する(checkout を含む restore の triple、dirty の扱い、往復のテストから決める)。

## 結果

- Operation Log の各行で、誰が・どの worktree で行った操作かが一目で分かる。選択すると、その時間帯に動いた ref が reflog の形で読める。
- テスト
  - domain unit: 窓、同じ秒の曖昧、下限なし。
  - panel unit: 隣接は同じ worktree のみ、末尾 `/` の表記揺れ。
  - kagi-git integration: 2 操作の窓に、それぞれの操作の reflog 行だけが入る。読み取りは何も書かない。
  - Tier A `oplog_actor_reflog`
    - 3 actor の記録が 3 行になり、それぞれのバッジが描画される。
    - 実クリックで選んだ checkout が自分の HEAD 行だけを示す。
    - 共有された秒の行は `Ambiguous` として描画される。
    - repo の指紋と oplog の件数が不変。
- 変異確認: 曖昧判定を外す、または隣接を worktree で絞らないと、それぞれ上記のテストが落ちる。
- slice 2a のテスト
  - domain unit: checkout は HEAD の symbolic だけが動く、作成 / 削除、commit。
  - codec unit: 旧行は `None`、`Some(空)` が往復する、不正値は行を捨てずに `None`。
  - kagi-git integration(実 `Backend::run`): checkout / commit / replay-onto(別 worktree で checkout 中の branch は `refs/heads/feat` だけ)/ 失敗 op は `Some(空)` / pipeline 外の `git branch` は次の entry に入らない。
  - Tier A: 実 run の行は「記録」を描画して推定の読み取りを始めない。記録の無い合成行は推定 + 曖昧。
- slice 2a の変異確認: 記録を止める → integration と Tier A、HEAD の symbolic を見ない → domain と integration、`Some(空)` を書かない → codec が落ちる。
- slice 2b-1 のテスト
  - domain unit(`ref_restore`): 合成(最古の old → 最新の new、削除)、移動して戻った ref は除く、revert は同じ ref の後続移動だけで blocker、範囲内の記録なし、checkout は blocker で commit の HEAD 追従は可、記録外の移動、行の往復と transaction。
  - kagi-git integration(実 `Backend::run`)
    - 3 手前へ restore すると全 branch が一致し、自分の `ref_moves` と backup 2 件が残る。
    - restore を revert すると restore 前に戻る(往復)。
    - 中間の revert で後続が保たれる。
    - 後続が同じ ref を動かしていれば blocker で、何も動かない。
    - 範囲内の記録なし entry、checkout、記録外の移動は blocker。
    - 計画後に branch が動くと refuse し、何も動かない。
    - checkout 中の branch の削除は blocker。
- slice 2b-1 の変異確認: LaterEntryMoved を外す、合成を最新の old にする、dirty を blocker に戻す → それぞれ domain と integration(往復を含む)が落ちる。
- slice 2b-2 のテスト
  - panel unit: append 済みは id 0 でも戻せる、append 失敗は戻せない。
  - Tier A `oplog_restore_card`: 記録なしの別 repository の行は両ボタン disabled。最初の create-branch の行の実ボタンで card が開き、Moves(drop1 / drop2 の削除)と RefsOnly が並び、`plan-confirm` が描画され、計画では何も動かない。Enter 1 回目で arm、2 回目で restore され、keep は残る。restore 自身の行の「取り消す」で元に戻る(UI 経由の往復)。
- slice 2b-2 の変異確認: arm を飛ばす → Tier A、`restorable` が記録を見ない → Tier A(disabled の行)、失敗 receipt の記録を残す → panel unit が落ちる。
- slice 2c のテスト
  - domain unit(`restore_preview`): branch の削除はその branch 固有の commit だけを消す、tag / remote が保持する commit は残る、移動した branch は戻し先の行に付く、戻し先が範囲外なら NotLoaded、窓 = 変化 ± 4 行で上限 40。
  - Tier A `oplog_restore_card` の拡張: 記録済みの commit を main に 1 つ足す。keep 時点への restore の card で、プレビュー行数 ≤ 40、main の移動先の行が戻し先 commit、消える行 = 1(その commit は描かれない)。restore 後に `git rev-list --count --branches` の減少数と一致する。描画 probe(`restore-preview` / `-removed-1` / `-moved-main-<sha>`)。revert card は、main の戻し先が reload 後の rows に無いので NotLoaded になり、「プレビューできません」が描画される。
- slice 2c の変異確認: restores を after に適用しない → domain 4 件と Tier A、card の extra を描かない → Tier A、NotLoaded の判定を外す → domain と Tier A が落ちる。
- #884 のテスト
  - kagi-git integration(`oplog_conflict_ref_moves_test`)
    - conflict save は `Some(空)`、merge commit は HEAD と main の移動を記録し、merge 前の時点への restore が blocker なしで通る。
    - cherry-pick の continue は HEAD と branch を記録する。
    - rebase の abort は HEAD が side へ戻る移動(detached → symbolic)を記録する。
    - rebase 最後の skip は side が main の先端へ進む移動を記録する。
    - 実行前の拒否は `Some(空)`。
  - Tier A: 新しい `oplog_restore_across_merge`(merge → 実 `run_recorded_conflict` の save → merge commit の後、panel の実ボタンで開いた restore card に blocker が無く、confirm で main が merge 前に戻る)。`stash_conflict_close_reopen` に、UI の continue の記録が `Some(空)` であることを追加した。
- #884 の変異確認: `run_recorded_conflict` が `None` を記録 → integration 2 件と Tier A(blocker `NotRecorded { op: "conflict-save:merge" }`、つまり #884 以前の状態)、`nothing_moved` を外す → 拒否の integration、UI が moves を捨てる → Tier A(stash continue)が落ちる。
- #891 review 対応: unit `an_unconfirmed_termination_records_no_ref_moves`(Unknown は `None`、Refused は観測どおり)。`stash_conflict_close_reopen` で、未解決のまま Continue を押した計画時の拒否が `Some(空)` で記録されることを確認する。変異確認: Unknown の規則を外す → unit、計画時の拒否を `None` に戻す → Tier A が落ちる。
- #885 のテスト
  - kagi-git integration(`oplog_restore_test`)
    - 実際に失敗した fetch を `observe_ref_moves` で包み、UI と同じ builder で記録すると `Some(空)` になり、その前の時点への restore が blocker なしで実行できる。`Unknown` の fetch は `None` で NotRecorded のまま。
    - remove-worktree は、branch を残すと `Some(空)`、`delete_branch` では `refs/heads/wtb` の削除(old = 元の tip)を記録する。#900(削除済み worktree の identity)と合わせて、自分の repository で 2 回の remove をまたぐ restore は blocker なしで plan でき、削除した remove の revert も EntryNotLoaded にならず blocker なしで plan できる。
    - 観測の範囲(#907 review): `observe_then` は write の前後だけを観測し、その後の read で外部が作った branch は記録しない。変異確認: read まで観測に含める → 落ちる。
  - kagi-git integration(fake `gh`): pr-comment / pr-review / pr-edit / issue create・comment の成功と拒否は `Some(空)`、issue の `Unknown` は `None`、PR merge は実行すると `None`、`gh` 前の拒否は `Some(空)`。
  - Tier A
    - `fetch_failure_oplog`: 実 `fetch_async` の失敗が `Some(空)`(`Unknown` なら `None`)。
    - `worktree_lock_reason`: lock / unlock の記録が `Some(空)`。lock の実行中に別 thread が `refs/heads/churn` を作成・削除し続けても `Some(空)` のまま(#907 review)。panel に出た lock の行の id が、log に永続化された id と一致する(#907 review)。
    - 新しい `worktree_prune_repair_receipt`: prune / repair の記録が `Some(空)`。
    - `pr_open_enters_before_ref_fetch`: 実 `fetch_pr_for_open` の失敗(`fetch-pr`)が `Some(空)`(`Unknown` なら `None`)。
- #885 の変異確認
  - `with_nothing_moved` が `None` を書く → transport の integration 8 件が落ちる。
  - remove-worktree が `None` を記録する → integration が落ちる。
  - fetch の記録を `None` にする → 統合テストの restore が `NotRecorded { op: "fetch" }` で止まる。UI の fetch で `None` → Tier A `fetch_failure_oplog` が落ちる。
  - lock / unlock、prune / repair、fetch-pr で `None` → それぞれの Tier A が落ちる。lock を前後の観測に戻す → churn の branch が記録されて落ちる。panel に placeholder の entry を渡す → id の一致で落ちる。
- #888 のテスト: kagi-git integration `an_entry_older_than_the_read_window_says_so`(自分の entry の後に別 repository の entry を 1000 件足すと EntryOutsideWindow { window: 1000 }、log に無い id は EntryNotLoaded)。変異確認: 置き換えをやめる → 落ちる。log 全体の有無を確かめずに置き換える → log に無い id も EntryOutsideWindow になり落ちる。プレビューの文言は wording なのでテストしない。
- #894 のテスト(kagi-git integration `oplog_restore_test`)
  - 削除・prune した worktree の entry は Mine で、その branch の削除が restore に入る(blocker なし)。identity を外した旧形式の行は UnknownRepository のまま。
  - 削除した無関係の repository の entry は blocker にならない。
  - 限界 1 の固定: 自分の entry の identity を別 volume のもの(path も inode も違う)に書き換えると、RefChangedOutsideRecord で止まる。
  - 限界 2 の固定: 別 repository の entry に、この repository の identity(`(dev, ino)` と作成時刻)を書くと、その記録の `refs/heads/x` が RefMovedSince(current = 無し)で止まる。
  - 同じ path に作り直した repository(#900 review): path はこの repository、file id は別 repository の entry は Other で、それを対象にした restore は EntryNotLoaded。inode 前提のテストは `#[cfg(unix)]`。
  - inode の再利用(#900 review 2): path も `(dev, ino)` もこの repository、作成時刻だけ違う entry は Other(EntryNotLoaded)。作成時刻を消した entry は UnknownRepository。domain 側は `same_repository` の unit で、再利用・移動・作成時刻なし・file id なしの各行を固定する。
  - 削除した worktree の remove-worktree(#900 review 3): identity は repo から取れる。別の repository の restore はその entry をまたいでも blocker なしで実行でき、自分の repository では UnknownRepository にならない。作成時刻を返さない、または秒精度の filesystem では、作成時刻を前提にするテストは理由を出してスキップする。
  - 秒精度の作成時刻(#900 review 4): unit で、ns 部が 0 の一致は Ambiguous、違えば Different であることを固定する。
  - codec unit: 往復、旧行は Absent。表形式で、正常 1 件(Known)と全違反パターン(object でない、数値、`null`、`common_dir` 無し・空・相対・型違い、`dev` / `ino` の片側、`dev` の型違い、`born_s` / `born_ns` の片側、`born_ns` = 10 億、負の `born_ns`、知らないキー)が Invalid で、行は残る。変異確認: 絶対 path の検査を外す・組の検査を外す・ns の範囲を外す → それぞれ表の該当行で落ちる。
  - 読めない identity(#900 review 5): 自分の entry の `repo_identity` を不正値に書き換えると UnknownRepository で止まる。field を消した旧形式の行は path で判定され、blocker にならない。
- #894 の変異確認: append で埋めない → integration 2 件、`(dev, ino)` を比べない → 限界 2 のテスト、分類で identity を使わない → integration 2 件、codec が書かない → codec と integration が落ちる。#900 review 2: 作成時刻を比べない → unit と inode 再利用の integration、曖昧を Mine にする → inode 再利用の integration が落ちる。#900 review 3: repo へのフォールバックを外す → 別 repository の restore が UnknownRepository で止まり、remove-worktree の integration が落ちる。#900 review 4: ns 部が 0 の一致も Same にする → unit が落ちる。#900 review 5: codec が不正値を Absent にする → codec unit と integration、分類で Invalid を旧形式として扱う → integration が落ちる。
- #886 のテスト
  - domain unit: branch 間の checkout は `from = Branch("a")`・`to = Branch("b")`、detached は `to = Detached(oid)`。
  - kagi-git integration(実 `Backend::run`): `checkout a` を含む restore は `HeadMoved { from: main, to: a }`、`checkout-commit` の revert は `{ from: a, to: Detached(<oid>) }`。
  - i18n unit: EN / JA の文言が entry・操作名・両側(短い OID)と、手で checkout する側・「#id 以降の時点へ restore」を含む。
  - 変異確認: from と to を入れ替える → domain と integration が落ちる。
- #878 review 対応(P1)のテスト
  - domain unit: 鎖が途切れると HistoryGap、別 repository の entry は除いて Unknown は blocker、記録外で変わった branch は RefChangedOutsideRecord(記録が説明する branch は除く)、動いて戻った ref が別の値にあれば RefMovedSince。
  - kagi-git integration: oplog から 1 行を消すと HistoryGap、削除・prune した worktree の entry は UnknownRepository、別 worktree で merge の conflict 中は OperationInProgress(その path)、対象の後に `git branch` で作った branch は RefChangedOutsideRecord。
- 変異確認: 鎖の検査を外す、Unknown を除外扱いにする、変化の検査を外す → それぞれ domain と integration が落ちる。in-progress を呼び出し元の worktree だけにする → integration、動いて戻った ref の検査を restores だけにする → domain が落ちる。
- #883 / #871 / #878 review 対応(P2)のテスト
  - Tier A `oplog_restore_preview_review`
    - 30 commit の履歴の深い位置に side commit をぶら下げ、PR head だけが保持する commit も用意する。branch Solo を on にして restore card を開く。
    - 消える数 = 2(deep の commit と main の最新。PR head の commit は残る)。
    - 行の箱が上限で止まり(33 行でも箱は行の合計より低い)、wheel で最後の行が箱の中に入る。
    - 実際の `Copy all`(`plan-card-copy` の probe)で、見出しと `main ←` が clipboard に入る。
  - unit: NotLoaded の見出しは「消える数」を言わない、Copy all の文字列(見出し・移動・窓外の行数・NotLoaded の理由)、Operation Log の entry コピーに記録された ref 移動(OID 全桁、`none moved`、記録なしは行なし)。
  - kagi-git integration: absorb は作業 branch の移動を、merged branch の一括削除は削除(new = 無し)を `ref_moves` に記録する。
- 変異確認: Solo の全行を使わない → 消える数 0、PR head を根にしない → 3、上限を外す → 箱が 957px、Copy all の clipboard を空にする → Tier A が落ちる。NotLoaded の見出しを `preview_heading(0)` に戻す・entry コピーから移動を外す → unit、absorb / 一括削除の記録を `None` にする → integration が落ちる。focus の変更は現状の入口では観測できないためテストなし(実ボタンのクリックで root が focus を得ることを確認した)。
