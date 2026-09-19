# T-PR-LAZY-FETCH — PR 一覧の三段階取得(zed 規模で 504 にならない)

- 発端: zed で `gh pr list --json <21 fields>` が GitHub GraphQL の処理タイムアウト
  (HTTP 504)になる。レートリミットではない。GitHub はクエリの簡素化・分割を推奨。
- 方針決定: w5:p0(omp)。設計レビュー: w5:p19(codex)。実装: w5:p1C(codex)。
- 参照: ADR-0186(一覧 fetch の契約)、ADR-0200(PR ワークスペース)。

## 決定(実装で変えない)

### 1. 取得は三段階

| 段階 | いつ | フィールド |
| --- | --- | --- |
| L1 一覧 | 60s ticker / 手動更新 / タブ切替、`--limit 100` 維持 | `number,title,headRefName,headRefOid,baseRefName,isDraft,author,updatedAt,createdAt,labels,assignees,reviewRequests,reviewDecision,url,isCrossRepository` |
| L2 状態 | 開いた PR(優先)+ 可視行(デバウンス) | `number,headRefOid,statusCheckRollup,mergeable` |
| L3 詳細 | 開いた PR のみ | `number,headRefOid,updatedAt,body,changedFiles,additions,deletions` |

- `headRefOid` は L1 に残す(merge の `--match-head-commit` の固定、L2/L3 の head 照合)。
  SHA 未取得の PR は merge を許可しない。
- L2/L3 は `gh pr view -R <base_repo> <n> --json …`(個別)。同時実行 2、可視窓 ±少量の
  先読み、同一 PR・同一段階の in-flight は重複させない。
- 504 は一覧(L1)に限り、1〜2 秒 + jitter で **1 回だけ**再試行。`fetch_json` に無条件
  retry を入れない(全 read の一斉再試行を避ける)。レートリミット応答は別扱い(既存)。

### 2. 所有と merge 規則(ADR-0186 の細分化)

- 一覧は「PR の集合・順序・L1 フィールド」を所有。詳細キャッシュは L2/L3 を所有。
  表示は両者を合成する。**L1 レスポンスを、省略フィールドが空/0/空配列の完全な
  `PullRequest` として扱わない**(現行 `apply_pr_fetch` の丸ごと置換はそのままでは
  取得済み body/checks/統計を消す)。
- 一覧成功: 100 件を置換。詳細キャッシュは「省略されたから」では消さない。
- 一覧失敗: 最後の成功一覧と詳細キャッシュを保持(Unavailable による一覧クリアは維持)。
- 詳細成功: その要求が求めたフィールド群だけ更新。正常な空 body / 空 checks / 0 files は
  有効値として反映する。
- 詳細失敗: その PR の前回成功値を保持し、失敗/古さを別に記録。次回可能時刻を持つ
  (失敗通知→再描画→即再取得のループを防ぐ)。
- head 変更: 古い head の checks/統計を現在値として使わない(表示するなら stale 明示)。
  同じ head でも CI は進む: 期限と明示 refresh を持つ。
- 完了の適用先: 開始時の `(session, repo_path, base_repo, number, 段階, generation)` を
  固定。`active_session()` / `self.repo_path` から取り直さない(§10 の既往バグ)。
- generation: 一覧は一覧の、詳細は PR×段階ごと。古い完了は新しい成功を上書きしない。
  一覧から消えた PR の遅延詳細は一覧へ再挿入しない(開いているタブ用には保持可)。
- 開いている `PrTab.pr` と一覧の PR コピーは、owner 指定の**一箇所**の適用処理で両方に反映。

### 3. 可視行駆動の起動

- UI は行番号ではなく**可視行の PR 番号集合**を controller に通知(ソート/フィルタで
  行番号が変わる)。契機: 初回レイアウト完了 / スクロール / リサイズ / フィルタ・ソート
  変更 / 一覧更新 / PR タブを開く。スクロールだけでは初回分が始まらない点に注意。
- render は要素構築のみ。可視範囲はレイアウト後にまとめて通知し、controller が予約。
  `render_item` ごとに spawn しない。
- controller: 150〜250ms デバウンス、同じ集合なら no-op、最大待ち時間あり。開いた PR は
  デバウンスを待たず優先。画面外へ出た未開始要求はキューから外す。開始済み read は
  owner/generation が有効ならキャッシュへ。

### 4. PrAttention の未確定

- 未取得 checks を `CiState::None`、統計を 0 に畳んで `attention()` に渡さない
  (reviewDecision=Approved だけで Ready に進む現行ロジックが CI 未取得を Ready と誤表示)。
- 未取得 / 取得中 / 取得済み / 前回値あり・更新失敗 を区別し、不足中は「判定待ち」。
  取得済み情報だけで確定できる NeedsYou(ChangesRequested 等)は先に出してよい。
  Ready は L2 が揃うまで確定させない。
- needs-you フィルタと可視行取得の循環に注意: 未判定行はフィルタで隠さず「判定待ち」と
  して表示し取得対象に残す。件数は確定分と未判定分を区別。

## 受け入れ条件

- zed(`zed-industries/zed`)で PR ホームが 504 にならず一覧が出る(L1 のみで)。
- 開いた PR のページに checks / mergeable / body / ±・files が後追いで埋まる。
- ホームのテーブルで、可視行の checks 列がスクロールに応じて埋まる。未取得行は
  「判定待ち」で、needs-you 判定に偽装した値を使わない。
- 一覧 fetch が成功しても、取得済みの詳細が消えない(テスト)。
- 一覧 fetch の 504 が 1 回だけ再試行され、2 回目失敗で最後の成功一覧が残る(テスト)。
- 既存: `cargo test --workspace` 緑、`uv run --project ci check-all` `::error 0`、
  GUI E2E `workspace_mode_toolbar` / `github_evidence_*` PASS。

## 連絡方法(herdr)

- 実装者 w5:p1C → 進捗・質問・完了報告は **自ペインの出力に書く**(omp が
  `herdr pane read w5:p1C` で回収する)。1 コミットごとに `[report] <hash> <要旨>` の
  1 行を出すこと。
- 設計の疑問は w5:p19(レビュアー)に **直接** 聞いてよい:
  `herdr pane send-text w5:p19 '<質問>' && herdr pane send-keys w5:p19 enter`、
  返答は `herdr pane read w5:p19`。
- 決定事項(本票の「決定」節)を変える必要が出たら、変えずに w5:p0(omp)へ
  `herdr pane send-text w5:p0 '[ask] …'` で相談する。
- 完了時: 全コミット hash と検証結果を自ペインに `[done]` で出し、w5:p19 にレビューを
  依頼(`herdr pane send-text w5:p19 'レビュー依頼: …'`)、指摘は自分で直して再依頼、
  `no further findings` を得たら w5:p0 に `[done]` を送る。
