# ADR-0209: PR の review thread を diff に重ねる — 行の横のバッジと、行の直下への折り畳み展開

- Status: **Accepted**
- Date: 2026-10-01
- Related: [#351](https://github.com/TomiXRM/kagi/issues/351)（第 2 の柱「review thread の重畳」。suggestion 適用は第 3 の柱で別 slice）、
  ADR-0136（PR mode の review conversation）、ADR-0124（split view）、ADR-0207（viewed 管理）、
  ADR-0172（suggestion 適用。そこで参照している REST の `pr_review_comments` 取得は本 ADR で置き換える）、
  ADR-0186（gh 取得の失敗を空として扱う契約）

## 文脈

PR mode は line comment を conversation の時系列リストとして表示している（`path:line` のテキストラベル付き）。
コメントが diff のどこに対応するかは、そのラベルを頼りに自分で探すしかなかった。取得は REST の
`GET /pulls/{n}/comments` で、どちらの面（base / head）の行番号か（`diffSide`）と、thread が古い位置に
なったか（`isOutdated`）は取れず、outdated は `original_line` に黙って置き換えていた。

## 決定

1. **取得**: REST の `pulls/{n}/comments` を、GraphQL `PullRequestReviewThread` の読み取り 1 本
   （`gh api graphql --paginate`、`path line startLine originalLine diffSide isOutdated isResolved
   viewerCanResolve` と thread 内の comments）に置き換える（`kagi_git::github::pr_review_threads`）。
   PR を開いたときの gh 呼び出し数は変わらない（`gh pr view` + この 1 本）。repository は従来どおり cwd から
   gh が解決する（`{owner}` / `{repo}` placeholder）。gh が答えられない場合は従来どおり空、GraphQL の
   `errors` は失敗として扱う（空の一覧と区別する）。thread あたり comments は先頭 100 件まで。
2. **conversation の feed は変えない**: thread の comments を平らにして従来の `ReviewComment` 一覧を作る
   （`review_thread::feed_comments`）。アンカー行は従来と同じ「現在の行、なければ元の行」なので、feed の
   表示・suggestion チップ・ADR-0172 の適用入力は同じ値を受け取る。
3. **配置（pure）**: `kagi_domain::review_thread::anchor_rows` が、thread を表示中の diff の行に対応付ける。
   `RIGHT` は head 側（new）の行番号、`LEFT` は base 側（old）の行番号で一致する最初の行。outdated
   （GitHub が `isOutdated`、または現在の `line` が無い）は `originalLine` の位置に置き、outdated として
   印を付ける（元の位置の目安であり、内容が一致する保証はない）。path が違う、または diff に現れない行の
   thread は置かない（feed には残る）。
4. **表示**: 行の間に挿入せず、行の横の gutter に件数バッジを出す（issue §5 の論点 1）。クリックで、その
   行の thread をその行の直下に折り畳み表示し、もう一度で閉じる。展開中だけリストの item 番号が 1 つずれる
   （閉じれば diff の行とリストの行が 1:1 に戻る）。split view では thread の `diffSide` に応じて左右どちら
   かの gutter に出す（context 行は両側に同じ行が描かれるが、バッジは thread の側だけ）。outdated は薄く
   表示し「Outdated / 古い位置」、解決済みは「Resolved / 解決済み」を付ける。**解決ボタンは出さない**
   （`viewerCanResolve` は読むだけ。解決は GitHub への write で、第 3 の柱以降）。
5. **対象の diff**: PR 全体の head 側 diff（merge-base..head）だけ。1 コミットを選んでいる間と Conflicts
   表示には重ねない（行番号の基準が違う）。ローカルの working tree との対応付けはしない。
6. **仕組み**: `render_diff_list` に per-row overlay（`render_helpers::row_overlay::RowOverlay`）を追加し、
   埋め込み側（PR mode）が gutter のマーカーと展開内容を渡す。展開・折り畳みは `ListState::splice` で
   item を差し込み・取り除くので、スクロール位置は保たれる（件数不一致による reset を起こさない）。
   他の diff 埋め込み（Main Diff・File History・Editor）は overlay を渡さず、見た目は変わらない。
7. **本文**: `timeline_row::body_markdown`（`prepare_github_markdown` → `sanitize_markdown_for_view`）を
   通す。path と author は `safe_text`。

## 却下した案

- **thread を行の間に常時挿入する**: 行番号とリストの行の対応が常に崩れ、AI レビューで thread が多いと
  diff が読めなくなる。
- **merge status の GraphQL（ADR-0136 の `reviewThreads{isResolved}`）に相乗りする**: 呼び出しは 1 本
  減るが、merge status は別 task・別の失敗扱いで、conversation の到着と結び付けると片方の失敗が
  もう片方を空にする。
- **resolve ボタンを出す**: write であり、plan → confirm → oplog の対象。本 slice は読み取りに限る。

## 結果

- PR を開くと、thread のある行に件数バッジが付く。クリックで行の直下に thread が開く。
- 検証:
  - kagi-domain unit: LEFT / RIGHT / context 行、同じ行の複数 thread、別 path・範囲外、outdated の判定と
    位置、feed への平坦化。
  - kagi-git unit: GraphQL の 2 ページの parse、GraphQL `errors` を失敗として扱うこと。
  - bin unit（`row_overlay`）: item 並び、pair の共有、splice 後の件数。
  - Tier A `pr_threads`: 実 PR ref fetch と 3 thread で、unified のバッジ → 実クリックで直下に展開・本文が
    描画 → outdated は薄い版 → 閉じると item 数が行数に戻る → split で LEFT / RIGHT の gutter、context 行の
    RIGHT は右だけ。
