# ADR-0214: Operation Log パネルを一級ビューにする — 全体設計、slice 1(読むだけ)、slice 2a(ref 移動の記録)、slice 2b(op revert / restore to point)

- Status: **Accepted**(slice 1、2a、2b-1 = backend、2b-2 = UI)。2c(グラフプレビュー)は本 ADR の設計を前提に個別に Accepted にする
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
- **未決**(slice 2 以降で決める): #334 §5 の論点。working tree も戻すか、Refused / Failed / Partial の時点の扱い、`--keep` 相当、プレビューを並べるか差分ハイライトか。

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
  - 他の worktree の HEAD は走査しない。replay-onto などで checkout 先の HEAD が追従しても、それは `refs/heads/<branch>` の差分から導ける(snapshot のコストを増やさない)。
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
  - 記録しない経路: conflict の continue / skip / abort(`backend/conflict_ops.rs`)、GitHub 側の書き込み(local の ref を動かさない)、UI が直接記録する経路(fetch、worktree の lock など)。これらは `None` のまま推定で表示する。conflict 経路の記録は restore に必要になった時点で追加する。

### 5. slice 2b: op revert / restore to point(2b-1 = backend、2b-2 = UI)

- **根拠は記録だけ**: 復元先や逆適用対象は `ref_moves`(`Some`)だけを根拠にする。範囲内に `None` の entry(推定しか無い時点)があれば blocker にし、推定(§3)は使わない。
- **Operation**: `Operation::OpRevert { entry_id }`(oplog 名 `op-revert`)と `Operation::RestoreToPoint { entry_id }`(`restore-to-point`)。
  - `Backend::run` を通すので、自分の ref の移動も記録される(§4)。戻した操作もさらに戻せる。
  - triple は `ops/oplog_restore.rs`。計画は pure な `kagi_domain::ref_restore::plan`。
- **対象の範囲**
  - 同じ repository の entry(worktree の common dir が一致するもの)。branch は worktree 間で共有されるので、他の worktree の entry も含める。
  - 読むのは oplog の末尾 1000 件。target が無ければ blocker(EntryNotLoaded)。
- **revert**: 対象 entry の各 branch の移動を old に戻す(作成は削除、削除は作り直し)。同じ ref を後続の記録済み entry が動かしていれば blocker(LaterEntryMoved)。
- **restore to point**: 対象 entry の**直後**の状態に戻す(jj の `op restore` と同じく、対象自身の効果は残す)。それより新しい entry を古い順に合成し、ref ごとに「最古の old を戻し先、最新の new を期待値」とする。動いて元に戻った ref は除く。
- **HEAD**
  - branch に追従しただけの HEAD の移動(commit など、symbolic が同じ)は、その branch の移動として扱う。
  - HEAD の切替・detached を含む entry は blocker(HeadMoved)。戻すには checkout(作業ツリーに触れる)が要るため、2b の対象外。
- **その他の blocker**
  - いずれかの ref が期待値に無い(RefMovedSince。記録外での移動)。
  - 戻し先の commit が既に無い(TargetGone)。
  - merge / rebase などが進行中。
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
  - destructive なので、最初の confirm(button / Enter)で arm し、二度目で `run_recorded`。CLI / MCP には出さない。
  - **記録できなかった receipt は戻す対象にしない**: append に失敗した entry は panel 上で id が placeholder(0)になる。oplog の id は 0 始まりなので、0 は最初の実 entry も指しうる。そこで `entry_for_recording` は `Recording::Failed` の `ref_moves` を落として「記録なし」(推定表示・ボタン disabled)にする。
  - 別 repository の行を選んだ場合、plan は EntryNotLoaded(「この repository の操作にない」)になる。

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
