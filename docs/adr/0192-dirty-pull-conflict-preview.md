# ADR-0192: dirty Pull は確認前に fetch し、衝突するパスを plan に載せる

## Status

Accepted (2026-09-08) — issue #625。ADR-0009（Guarded）/ ADR-0129（plan note）/
ADR-0189（auto-stash pull と error modal の寿命）の続き。

## Context

dirty な working tree で Pull すると Stash & Pull の確認モーダルが出る。実測（#625、
実 GUI + 実クリック）では、衝突が確定しているケースでも safe なケースと**完全に同一**の
warning 1 件しか出ていなかった。

```
Working tree has 1 staged, 1 modified, 1 untracked. Kagi will stash these
changes, pull, then restore them. If restoration conflicts, the stash is kept.
```

確定すると pull は成功し、**stash の復元で初めて** conflict が判明する:

```
async: pull partially applied — Pull completed, but auto-stash restoration
  conflicted in: shared.txt. The stash was kept.
```

`shared.txt` という情報は事後には正確に出る。つまり計算はできていて、行われる
タイミングが確認の後だった。stash は保持されるのでデータは安全だが、「失敗するケースは
事前にモーダルに表示する。pull してから失敗しましたはコンセプトに反する」という製品原則に
反する。

原因は 2 つ重なっていた。

1. **予測が commit 同士の merge だけだった。** `predict_merge_conflict` は local tip と
   upstream tip を in-memory merge する。この fixture は fast-forward なので commit 同士は
   衝突せず、正しく「衝突なし」と答える。実際に衝突するのは *working tree の内容* と
   incoming の内容で、その組み合わせが plan に一切モデル化されていなかった。必要な計算は
   `ensure_pull_does_not_touch_dirty_paths` が既に行っているが、呼ばれるのは execute 時、
   しかも auto-stash が working tree を空にした**後**なので何も検出しない。
2. **`plan_pull` は fetch しない。** 実測ではモーダルのタイトルが
   `up to date (local knowledge; fetch may reveal more)` のままで、upstream の commit を
   そもそも知らない状態でモーダルが出ていた。fetch は `execute_pull` の step 2 にある。
   したがって 1 だけ直しても、観測された実フローでは何も予測できない。

## Decision

### 1. 衝突しうるパスを plan の note として持つ

`PullNote::RestoreConflict { paths: Vec<String> }` を追加する。件数ではなく**パス名**を
持つ（件数だけでは事後と同じ驚きが残る）。`plan_pull` は blocker が無く upstream が
解決できるときだけ、「dirty なパス」∩「HEAD..upstream で変わるパス」を計算して warning に
積む。rename は from/to 両方を dirty 側に入れる。

表示は EN/JA 両方で 1 パス 1 行に分離する。`RESTORE_CONFLICT_PATH_LIMIT`（12）を超えたら
残数を数える — plan note は確認の補助であってファイル一覧ではなく、無制限に並べると
モーダルのボタンが画面外に出る。

計算は `ops/pull_conflict.rs` に集約し、plan（事前提示）と execute（拒否）が同じ
`pull_dirty_overlap` を読む。同じ判断が二箇所で drift しないことが要点で、`ops/pull.rs` が
既に 800 行の目安を超えていた問題も feature 境界での分割で同時に解消する。

### 2. dirty Pull はモーダルを出す前に fetch する

`plan_pull` の予測はローカルが知っている origin に基づく。auto-fetch は 180s 間隔なので、
直前に upstream が動いていれば予測は漏れ、「復元は綺麗に済む」と約束したモーダルが
そのまま失敗しうる。**確認の後に驚かせない**を厳密に満たすのは確認前の fetch だけである。
fetch は working tree を変更しない読み取りなので、確認前に走らせても安全。

実装は既存の `fetch_async` をそのまま使う（新しい fetch 経路は作らない）。`KagiApp` の
`pull_modal_after_fetch` が「この fetch は Pull の確認のためだ」という 1 bit で、完了後に
`plan_and_open_pull_modal` へ繋ぐ。dirty でない Pull は従来どおり同期でモーダルを開く。

**fetch 失敗時はモーダルを出さない。** footer の `Fetch failed: …`（既存表示）が答えで、
「たった今更新に失敗した知識」に対して確定させるのは、この遅延が避けようとしている驚きそのもの。

**モーダルを開くのは reload の完了後。** `fetch_async` は ref が動いたとき `reload` を呼び、
その apply は確認モーダルを掃除する（ADR-0189。fetch が watcher を焼くのが理由）。fetch の
継続でモーダルを開くと、正しく plan されたモーダルが直後の reload apply で消える —
E2E で実際に踏んだ。よって ref が動いたときはフラグを残し、`apply_reload_data` の掃除の
**後**で開く（#309 の stash follow-up と同じ形）。ref が動かなければ reload は来ないので
その場で開く。tab が切り替わっていたらフラグを捨てる。

**開いた後の reload では replan して残す。** 上の「掃除の後で開く」だけでは足りなかった。
watcher は fetch の書き込みを検知して**別の** reload を後から届け、その apply が開いたばかりの
モーダルを消す。実測（PM の実 GUI 差し戻し、#626 review）:

```
[kagi] fetch: start
[kagi] plan: pull blockers=0 warnings=2      ← モーダルはここで開く
[kagi] refreshed (external change)           ← 約 0.5 秒後にここで消える
```

押しても何も出ないのは Pull が実行不能なのと同じである。よって **auto-stash 確認が開いている
間の reload は、掃除せず `replan_pull_modal` で内容を作り直す**。ADR-0189 の意図（無効化された
plan を confirm させない）は replan で満たす — 消すのではなく最新にする。plan が「もう pull する
ものが無い」または失敗を返した場合も既存モーダルを**残す**（ユーザーのカーソル下で窓を空に
しない）。実行時の安全は `Backend::run` の preflight が担保する。ユーザーが Cancel か
Stash & Pull を押すまで消えない。

error 状態のモーダル（ADR-0189）はこれまでどおり保持し、clean な Pull の確認は従来どおり
reload で無効化する（clean な Pull は fetch しないので自分で reload を起こさない）。

**incoming の定義は `merge-base..upstream`。** HEAD の tree を upstream の tree に直接 diff すると、
diverged 時に「local commit だけが変えたパス」（逆方向の差分）も incoming として数え、upstream が
持ち込んでいない衝突を警告してしまう（実測: `RestoreConflict { paths: ["mine.txt"] }`)。fast-forward
では merge-base == HEAD なので挙動は同じ。merge-base が無い（無関係な履歴）場合は HEAD に落とす。

**表示は `note_path_list` + localized summary。** パスは prose のカンマ列ではなく行として描く
（#454 が checkout overlap で入れた仕組みに合流）。warning 側の描画も blocker と同じ分岐にした。
summary は `Msg::PlanRestoreConflictSummary`（EN/JA）で件数だけを持ち、パスは行に出る。
`message_en` / `note_ja` の本文はそのまま CLI・oplog・テスト用の fallback として残る。

**予測は実際に merge して確かめる（#626 review）。** 「同じパスが両側で変更された」は
conflict ではない。upstream が先頭行、ローカルが末尾行を触った場合、`git stash pop` は
自動 merge に成功する（実 git で確認）。それを「conflict します」と断定すると当たらない
警告になり、ユーザーは警告を読まなくなる — #625 で直した信頼が別の形で壊れる。

重なった各パスについて `git2::merge_file` で 3-way content merge を実行する。
ancestor = HEAD の blob（ユーザーの編集の基準）、ours = upstream tip の blob
（pull が working tree に置くもの）、theirs = working tree のバイト列。`merge_file` は
メモリ上で完結し、loose object も ref も書かない — plan が object を書くと watcher が
発火し、今回直した「モーダルが消える」を再発させる。

判定は 3 分岐:

| 判定 | 条件 | note |
|---|---|---|
| clean | `is_automergeable()` | note を出さない |
| conflict | merge が conflict marker を生む | `RestoreConflict`（断定） |
| 不明 | binary、blob が取れない、**mode 変更（upstream 側も working tree 側も。`chmod +x` は content merge では判定できない）**、type 変更（symlink 化など）、片側の追加/削除（add/add・delete vs edit は content merge ではなく pop 自体が拒否する） | `RestoreConflictPossible`（可能性） |

EN/JA とも断定と可能性で文面を分ける（"will conflict" / "may conflict"、
「conflict します」/「conflict する可能性があります」）。modal の summary も別 `Msg`。

**保留中の Pull 確認は `SessionId` で持つ（#626 review）。** 素の bool では tab A が fetch を
待っている間に tab B の reload がそれを消し、「押しても何も起きない」が別経路で再発する
（CLAUDE.md の state ルール: タブ固有の状態は SessionId で持つ）。`pending_pull_confirm:
Option<SessionId>` を、要求した tab が表示されているときだけ消費する。reload 側はこの状態に
一切触らない（触る必要が無くなった: モーダルは fetch の継続で即開き、以降の reload は replan）。

**確認前 fetch の失敗は modal + oplog に残す（#626 review）。** footer と toast だけでは
消えると何が起きたか分からない。CLAUDE.md の「User-facing errors must surface via the oplog
and a modal」に従い、`AppNotice`（ユーザーが閉じるまで残り、reload で消えない唯一の modal）で
伝え、ADR-0149 の non-run op 経路で `fetch` の `Failed` を oplog に永続化する。Pull の確認
モーダルは出さない — 更新に失敗した知識に対して確定させてはいけないため。

### 2b. 保留中の確認は「その fetch task」に配送する（#626 review 3 周目）

3 度「押しても何も起きない」を再発させたので、分岐を塞ぐのをやめて状態を列挙した。

要求は `open_pull_modal` で作られ、**その fetch task のクロージャの中**を旅する
(`fetch_async_for(silent, pull_confirm, cx)`)。global flag は持たない — 無関係な fetch が
消費することも、reload が落とすこともできない。fetch が既に走っている場合だけは例外で、
**同じ repo を更新している in-flight fetch** に相乗りする（別 repo の fetch に相乗りすると
「fetch してから確認」が嘘になる）。相乗り先が無ければローカル知識で即 plan する。

完了時の配送規則（全ケースを 1 箇所で決める）:

| 完了時の状態 | 配送 |
|---|---|
| fetch 失敗 | oplog に `fetch` Failed を必ず記録。要求元 tab が表示中なら notice modal、そうでなければ park |
| 成功・要求元 tab 表示中・他の modal 無し | plan して確認モーダルを開く |
| 成功・要求元 tab が**非表示** | その tab 用に park し、次にその tab がアクティブになった時に配送 |
| 成功・別の modal が開いている | 要求を取り消す（新しいユーザー操作が勝つ）|
| 要求元 tab が閉じられた | tab と一緒に破棄 |

park は `pending_pull_confirm: HashMap<SessionId, PullConfirmDelivery>`。「Pull を押して
tab を離れ、戻ってくる」が成立するのはこれのため。答えは要求した tab のものなので、
別 repo の上に開かずに待つ。

### 2c. 確認の約束は stash の前に照合する（#626 review 3 周目）

モーダル表示後に外部 editor が別の重なるパスを保存すると、`pull_blocking` は先に stash
するため `Backend::run` の preflight も execute 時の `ensure_pull_does_not_touch_dirty_paths`
も**空の tree** を見て素通りし、復元だけが事前表示なしで conflict していた。

`PullPlanModal::dirty_digest`（表示時の `WorktreeDigest`）を確認に束縛し、stash の**前**に
再取得した digest と再 plan した restore note 集合を照合する。どちらかが動いていれば
`auto_stash_plan_stale`（EN/JA）で拒否し、stash も pull もしない。plan 側の
`worktree_digest` は使わない — あれは「execute 時に tree が動いていたら拒否」の意味で、
stash-first の pull は意図的に tree を空にするため（checkout の preflight が誤発火した）。

### 2d. reload は plan を無効化するが、入力は無効化しない（#626 review 3 周目）

`apply_reload_data` の掃除は `CreateBranch` / `CreateWorktree`（plan preview を持たない
純粋な入力モーダル）を消さないようにした。dirty Pull が確認前に fetch する以上、kagi 自身の
fetch とそれが起こす watcher reload が「入力中の branch 名」を消してしまう。reload が
無効化するのは *plan* であり、ユーザーが打っている文字ではない。plan を持つモーダルは
従来どおり全て掃除する。tab / repo 切替時は `reset_per_repo_ui` が引き続き消す。

### 3. execute 側の fetch は残す

`execute_pull` の step 2 の fetch はそのまま。dirty Pull では二重 fetch になるが、
blast radius を抑える判断である。理由:

- `execute_pull` は UI 以外（CLI / MCP / 他の caller）からも呼ばれ、その安全性は
  「実行直前に自分で fetch し、fetch 後の tip に対して dirty path を再検査する」ことに
  依存している。UI が事前に fetch したことを execute が前提にすると、その保証が UI 経由の
  呼び出しだけに縮む。
- 確認からの確定までにユーザーが任意の時間をかけられる。execute の fetch は「確定した瞬間の
  upstream」を見る最後の関門で、plan の予測は表示の鮮度、execute の fetch は実行の正しさを
  担保する別の役割である。
- 冗長な fetch のコストは、no-op fetch 1 回分（ref 更新なし）。

## Consequences

- overlap fixture では確定前のモーダルに `shared.txt` が出る。safe fixture では衝突 note が
  出ない。GUI E2E `pull_auto_stash_overlap_preview` が実 UI で両方を assert する。同 scenario は
  **fetch を一切しない** repository で走るので、パス名が出ること自体が「UI が先に fetch した」
  証拠になる。
- dirty Pull はモーダル表示までネットワーク往復ぶん遅くなる。ローカル remote では実測 128ms
  規模、実 remote では回線次第。dirty でない Pull は不変。
- `ADR-0189` の「確認モーダルは reload で無効化される」は維持する。例外は error 状態と、
  この fetch 由来の reload が自分で開き直す 1 ケースだけ。
- 予測はあくまで予測。plan 時点の upstream と working tree に基づくので、確定までに外部が
  さらに動けば結果は変わりうる。実行の安全は execute 側の再検査が担保する。

## Alternatives considered

- **ローカル知識だけで予測する（fetch しない）** — 差分は最小で `predict_merge_conflict` と
  一貫するが、実測されたフローでは upstream tip を知らないまま出るモーダルが残り、症状が
  そのまま再発する。#625 の目的を満たさない。
- **plan_pull 自身が fetch する** — 予測は正確になるが、plan は純粋な読み取りとして
  CLI/MCP/背景処理からも呼ばれ、そのすべてにネットワーク往復と lease 取得を持ち込む。
  fetch は UI の入口に置くほうが影響範囲が小さい。
- **衝突を blocker にする** — 確定を拒否すれば失敗はしないが、stash は保持され conflict UI に
  誘導される（データは安全）ため、拒否は過剰。ADR-0009 の Guarded 方針どおり warning にして
  ユーザーに選ばせる。
