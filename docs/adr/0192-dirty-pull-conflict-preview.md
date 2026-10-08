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

### 2. clean / dirty の両方で確認前に fetch する（#1087）

`plan_pull` の予測はローカルが知っている origin に基づく。auto-fetch は 180s 間隔なので、
直前に upstream が動いていれば dirty path の予測が漏れるだけでなく、clean な Pull でも
古い `behind=0` を根拠に「すでに最新です」と誤表示する。確認または最新の案内を出す前に、
**その要求を所有する session の fetch 成功を待つ**。バックエンドの `plan_pull` は純粋な
ローカル計画のままにし、ネットワークアクセスを隠して追加しない。

fetch は remote-tracking refs を変更する **write** である。既存の admitted
`fetch_async_for` の lease・実行・verify・oplog 経路を使い、新しい UI 直書き経路を作らない。
確認前の fetch は HEAD、index の staged content、working tree を変更しない。
保留中の要求は `FetchFlight` の owner / visit 付き waiter が持つ。global flag は持たない。

**fetch 失敗時はモーダルを出さない。** footer の `Fetch failed: …`（既存表示）が答えで、
「たった今更新に失敗した知識」に対して確定させるのは、この遅延が避けようとしている驚きそのもの。

**fetch 由来の確認を、同じ checkout の reload で消さない。**
fetch と watcher は複数の reload を届けるため、配送後の clean 確認にも
`PullPlanModal::fetch_owner` として session / visit の provenance を付ける。
同じ所有者・visit、同じ HEAD と working-tree digest、未実行・エラー無しの local 確認だけを
`apply_reload_data` が `replan_pull_modal` で更新して残す。snapshot が一致していても、その採取後に
live checkout が変わりうるため、**再計画した plan / digest も旧確認と照合する**。HEAD / digest
の不一致や clean → auto-stash への変化では置き換えず無効化する。replan が NoOp / 失敗の場合も
clean 確認を消す。

既存の auto-stash 確認と error 状態の保持は維持する。auto-stash の再計画が NoOp / 失敗を
返した場合は既存確認を残し、実行時の `Backend::run` の preflight が安全を担保する。
direct な blocker 確認・remote 確認には local fetch の provenance を付けない。

**後から始めたユーザー操作を優先する。** 待機中に別のモーダルを開けば、その後閉じても
古い Pull 要求を復活させない。`FetchFlight` が displacement を保持し、完了時は確認を
押し付けず再試行の案内にする。改めて明示した Pull だけが同じ owner の flight に再参加できる。

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

**保留中の Pull 確認は `(SessionId, visit)` で持つ（#992 review、2026-10-04 改訂）。**
要求はその fetch の waiter に格納する。別 tab への離脱では同じ session の
`visit` が進む（`Sessions::depart`）。完了時に現在の visit と一致する waiter
だけが確認を開ける。旧 visit の提案を帰還時に復活させない。

**確認前 fetch の失敗は toast + oplog に残す（#626 review、2026-09-20 改訂）。** toast は
短い通知に限定し、完全なエラーは ADR-0149 の non-run op 経路で `fetch` の `Failed` として
oplog に永続化する。記録済みの結果を閉じるだけの `AppNotice` は表示しない。Pull の確認
モーダルも出さない — 更新に失敗した知識に対して確定させてはいけないため。

### 2b. 保留中の確認は「その fetch task」に配送する（#626 review 3 周目）

3 度「押しても何も起きない」を再発させたので、分岐を塞ぐのをやめて状態を列挙した。

要求は `open_pull_modal` で作られ、`fetch_async_for(silent, pull_confirm, cx)` の
owner / visit 付き waiter としてその fetch flight に配送される。global flag は持たない —
無関係な fetch が消費することも、reload が落とすこともできない。
既に走っている **同じ repo の fetch** にだけ相乗りする。相乗り先が無く、新しい fetch を
admit できなければ stale なローカル知識で Ready / 最新を案内しない。

完了時の配送規則（全ケースを 1 箇所で決める）:

| 完了時の状態 | 配送 |
|---|---|
| fetch 失敗 | fetch 自体が oplog に一度だけ記録し、現在の visit にだけ短い toast を表示。modal は作らない |
| 成功・要求元 tab の同じ visit が表示中・他の modal 無し | plan して確認モーダルを開く |
| 成功・要求元 tab が離脱済み／閉じられた | waiter を破棄し、帰還後も配送しない |
| 成功・別の modal が開いている | 要求を取り消す（新しいユーザー操作が勝つ）|

失敗した fetch の receipt は waiter から追加記録しない。別 tab 表示中の失敗も
元の repository の oplog に一度だけ永続化する。古い visit の modal/footer/toast
は表示しない。ただし旧 visit の fetch がまだ走っている間に新 visit で Pull を要求したら、
新 visit の waiter にだけ失敗の短い footer/toast を配送する（#992 review）。

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

`execute_pull` の step 2 の fetch はそのまま。実行する local Pull では二重 fetch になるが、
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
- clean / dirty ともに、確認または「すでに最新です」の表示までネットワーク往復が必要になる。
  失敗・admission 拒否・別 owner の flight は最新の証拠にしない。
- `ADR-0189` の reload 無効化は維持する。例外は既存の error / auto-stash 確認と、
  同じ session / visit・未変更 checkout に対する local fetch 由来の確認だけ。
- SSH remote の cached `behind=0` も最新の証拠にしない。既存の live identity probe と
  明示確認へ進み、確認前に remote fetch / pull を暗黙実行する経路は追加しない。
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
