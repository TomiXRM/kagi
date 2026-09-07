# ADR-0186: PR 取得は「空一覧」と「取得失敗」を型で分ける

- 状態: 採用（#506）
- 日付: 2026-09-07
- 関連: [#506](https://github.com/TomiXRM/kagi/issues/506)、
  [ADR-0177](0177-transport-recording-boundary.md)（未証明の結果を確定扱いしない）、
  ADR-0128（Branch Cleanup）、ADR-0137/0153

## 決定

`gh pr list` の結果は `Result<Vec<PullRequest>, PrFetchError>` を返す。
`Ok(prs)`（`Ok(vec![])` を含む）だけが repository についての**証拠**であり、
失敗は分類して返す。

| 分類 | 意味 | cache |
|---|---|---|
| `Ok(vec![…])` / `Ok(vec![])` | GitHub が答えた（0 件も答え） | 置き換える |
| `Unavailable` | GitHub remote なし / GitHub repo でない | 空にする（error 表示はしない） |
| `Auth` | 未 login / token 失効 / scope 不足（exit 4 か auth marker） | **保持** |
| `Network` | DNS / TLS / timeout / refused | **保持** |
| `Invalid` | `gh` は答えたが JSON を解析できない | **保持** |
| `Unknown` | 分類できない非ゼロ終了（rate limit、将来の `gh` 文言） | **保持** |

分類は `github_fetch::classify_gh_failure`（純粋、unit test 付き）が exit code と
stderr marker で行う。auth を最初に判定する: logged out の `gh` も
"could not determine base repository" を出すため、逆順だと token 失効で一覧を
消す元のバグを再現してしまう。**未知の文言は `Unknown` のまま**にする
（ADR-0177 と同じ規律。未証明の結果を「PR は 0 件」という確定的な答えへ
降格させない）。

cache への適用は `github_fetch::apply_pr_fetch` 一箇所に置き、sidebar の 60s
refresh と Branch Cleanup scan の両方がそれを呼ぶ。呼び出し側ごとに分岐を
書き直さないことがこの ADR の実体。

## 問題

`list_open_prs` / `list_merged_prs` は `gh` の全非ゼロ終了を `Ok(Vec::new())`
へ畳んでいた。結果として token 失効・offline・未設定・本当に 0 件がすべて
同じ値になり、`src/ui/github.rs` が「失敗なら前回の一覧を保つ」意図で書いた
`Err` 分岐へは**何も届かなかった**。60s tick ごとに前回取得できていた PR 一覧が
空で上書きされ、ユーザには「PR が 0 件」と表示された。

Branch Cleanup も同じ経路で `unwrap_or_default()` していたため、取得失敗が
「この branch は PR なしで merge された」という空の PR 列として表示されていた。

## 表示

- `Unavailable`: PR home は `PrGithubUnavailable`（「GitHub remote が設定されて
  いません」）。error ではないので tick ごとの通知は出さない。
- 失敗（`Auth`/`Network`/`Invalid`/`Unknown`）: 既存の `github_error` 経路に
  分類ごとの EN/JA 文言 + `gh` の原文を載せる。一覧が残っている場合は
  `PrFetchStale`（「前回取得できた一覧を表示しています」）を tiles の上に出す。
- Branch Cleanup: `cleanup_prs_stale` で `CleanupPrEvidenceStale` を header に
  出し、PR 列が最新でないことを明示する。

`[kagi]` の contract 行は変更しない（`github: prs=N` / `github: error: …`）。
error 行の本文だけが `auth: …` / `network: …` のように分類名を先頭に持つ。

## スコープ外

`pr_conversation` / `pr_review_comments`（review chat）も同じ `Ok(empty)` 変換を
持つが、症状は「レビューが空に見える」であり一覧の破壊ではない。同じ契約へ
寄せるのは後続。
