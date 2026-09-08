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
