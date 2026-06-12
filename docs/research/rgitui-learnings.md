# rgitui 流用・学び調査

- 調査日: 2026-06-13 / 調査者: research subagent
- 対象: `noahbclarkson/rgitui`（`git clone --depth 1` → `/tmp/kagi-research/rgitui`、main HEAD）
- ユーザー要望（強い参考希望): ①PR/Issue 閲覧 ②ファイルごとの diff heat map 的表示 ③UI の綺麗さ
- 関連 ADR: 0031（外部流用ポリシー）, 0037（GitHub avatar）, 0026（Compare view）, 0015/0016（Inspector/diff）, 0036（color themes）, 0006（gpui-component）

## 0. ライセンス原文確認(ADR-0031 ゲート)

- **`/tmp/kagi-research/rgitui/LICENSE` = MIT License**（原文確認済）。`Copyright (c) 2026 rgitui contributors`。
- `Cargo.toml` `[workspace.package] license = "MIT"`、`publish = false`。
- **結論: MIT = Port/Adopt 可**（NOTICE/著作権表記保持義務のみ）。GPL/FSL のような汚染リスクなし。**kagi で唯一、コード移植が許可される参照元**(これまでの jj=GPL系/GitButler=FSL/Zed=GPL とは別格)。
  - ただし「許可される」と「すべき」は別。下記の依存非互換（gpui git rev）と概念純度の観点から、**実際の採用は Port 限定的・Reimplement 中心**を推奨。

### 重大な依存非互換(最重要・kagi が rgitui のコードを安易に転写できない理由)

- rgitui は **gpui を crates.io ではなく Zed git rev に固定**:
  `gpui = { git = "https://github.com/zed-industries/zed.git", rev = "f3fb4e04…" }`(`Cargo.toml` L28-31)。
  さらに `http_client` / `reqwest_client` も**同 rev の Zed 内部 crate**を直接依存。
- kagi は **gpui = "0.2.2"(crates.io)** に pin(ADR-0001/0006、zed 調査の結論)。
  → rgitui の PR/Issue・avatar コードは `http_client::HttpClient`(Zed crate)・`cx.http_client()`・`AsyncBody` に強く依存しており、**そのままコピーすると Zed git 依存を引き込む**。kagi の HTTP 方針(ADR-0037: gpui 同梱 http が 0.2.2 で使えるか調査 → 不可なら `ureq`)とは別系統。
  → **PR/Issue/heat map いずれも「ロジック設計は Port、HTTP I/O 層は kagi の http stack に置換」が必須**。MIT なので転写自体は合法だが、機械的コピーは依存衝突する。

## 1. アーキテクチャ全般

- **9 crate ワークスペース**(`Cargo.toml`):
  `rgitui`(bin) / `rgitui_workspace`(views・dialogs・GitHub・panels) / `rgitui_git`(git2 backend) / `rgitui_ui`(自前 UI 部品 30+) / `rgitui_theme` / `rgitui_graph` / `rgitui_diff` / `rgitui_ai`(Gemini) / `rgitui_settings`(keyring)。
- **backend = git2 0.20**(kagi と一致)。`rgitui_git/src/project/` が機能別ファイル分割(diff/blame/rebase/bisect/reflog/network/submodule/search/watcher/refresh/file_history…)。kagi の project module 分割の参考になる粒度。
- **非同期**: `cx.spawn(async move |this, cx: &mut AsyncApp| { … this.update(cx, …) })` パターンで全 fetch を background 実行 → 結果を entity に書き戻し `cx.notify()`。git 操作も background executor(README「All git operations run on a background executor」)。
- **状態管理**: gpui Entity + `EventEmitter<PanelEvent>`。各 panel が `is_loading / error_message / auth_required / last_fetched` を自前で持つ素朴な enum-state。kagi の plan パイプラインのような安全機構は無し(rgitui は destructive をそのまま実行する)。
- **自前 UI crate(rgitui_ui)**: kagi が gpui-component を使うのに対し rgitui は Button/Badge/Label/Icon/Modal/Toast/Tooltip/Scrollbar/TextInput/DiffStat… を**全部自前実装**。MIT なので個別 Port は可能だが、kagi は gpui-component を採用済(ADR-0006)なので**設計言語のみ参考**が妥当。

## 2. PR / Issue 閲覧(観点②)

### 構成

- `crates/rgitui_workspace/src/prs_panel.rs`(PR、~1730行)/ `issues_panel.rs`(Issue、構造ほぼ対称)/ `github_api.rs`(共通 GET ヘルパ + エラー整形)/ `github_device_flow.rs`(OAuth Device Flow)/ `create_pr_dialog.rs` / `markdown_view.rs`(本文/コメントを Markdown レンダ)。

### GitHub API の使い方

- **REST v3**。`Accept: application/vnd.github.v3+json` + `User-Agent: rgitui`。
- エンドポイント:
  - PR一覧: `GET /repos/{o}/{r}/pulls?state={open|closed|all}&per_page=50&sort=updated`
  - Issue一覧: `GET /repos/{o}/{r}/issues?state=…`(検索は `/search/issues`)
  - コメント: `GET /repos/{o}/{r}/issues/{n}/comments?per_page=50`(PR も issues comments で取得)
  - レビュー投稿: `POST /repos/{o}/{r}/pulls/{n}/reviews`(`{body, event: APPROVE|REQUEST_CHANGES|COMMENT}`)
- **認証方式(2 系統)**:
  1. **未認証読み取り**: public repo はトークン無しで GET。`github_get_collection_body()` は token が `Some` の時だけ `Authorization: Bearer …` を付与(`github_api.rs` L34-40)。
  2. **OAuth Device Flow**(`github_device_flow.rs`): 内蔵 `GITHUB_CLIENT_ID`、scope=`repo`。`POST login/device/code` → user_code 表示 → `POST login/oauth/access_token` を `interval` 秒で poll(`authorization_pending`/`slow_down`/`expired_token`/`access_denied` を厳密処理)。**PAT 直接入力も Settings で可**。
- **トークン保管**: OS キーチェーン(`keyring` crate v3、`rgitui_settings/src/lib.rs`)。service=`"rgitui"`、account=`ai/default` / `git/default-https` / `git/provider/{id}`。起動時に keyring → in-memory `AuthRuntimeState`(`OnceLock<RwLock<…>>`)へ解決。平文 JSON からの**レガシー移行**(`migrate_legacy_secrets`)あり。→ **kagi がトークンを扱うなら keyring + アカウント名前空間方式は良い手本**。
- **rate limit 対応**: 専用の 429/`X-RateLimit` ヘッダ処理は**無い**。代わりに:
  - **60s TTL キャッシュ**: `last_fetched: Option<Instant>`、`elapsed() < 60s` なら refetch skip(`prs_panel.rs` L276-283)。filter 変更・新規 PR 作成時は `last_fetched=None` で強制再取得。
  - **エラー整形**: 401/403/404 を `auth_required` フラグ化 → 生エラーでなく "Sign in to view…" 空状態を出す(`prs_panel.rs` L1087)。403 の OAuth App 制限 / SAML SSO 文言を短い実行可能指示に**書き換え**(`github_api.rs` `rewrite_org_restriction`)。これは UX として秀逸。
- **UI 構成**(`prs_panel.rs`):
  - toolbar: アイコン+タイトル+件数 Badge + Open/Closed/All セグメント(枠線で囲った連結ボタン群)+ "New PR" + refresh。
  - list: `uniform_list`(仮想化)。各 row 36px、state アイコン(色分け: Open=Success/Closed=Error/Merged=Accent)+ `#番号` + タイトル truncate + label Badge(最大2+「+N」)+ author + 日付 + コメント数。**選択は左 border 2px + bg**で表現。
  - **loading skeleton**: 6 行のグレー矩形プレースホルダ(`render_loading_skeleton`)。スピナーより上質に見える要因。
  - detail view: カード(elevated bg + rounded 8 + border)に title/state badge/draft/author/branch(`head -> base`)/labels/Markdown body。Open PR には Approve/Request Changes/Comment のレビュー投稿 UI。コメントは各々アバター円 + Markdown。
- **キャッシュ**: README「Browse issues and PRs (cached with 60s TTL)」。それ以上のディスクキャッシュは PR/Issue には無し(avatar は別途ディスクキャッシュあり)。

### ADR-0037 との整合性(重要な相違)

- ADR-0037 は **avatar 用途で「email を検索クエリにする user-search API は使わない(privacy + rate limit)」**と明記。
- ところが rgitui の `avatar_resolver.rs` は **`GET /search/users?q={email}+in:email` と `…+in:fullname`(氏名検索)を実際に使用**(L296-363)。さらに gravatar(`d=404`)も叩く。
  → **これは ADR-0037 が明示的に避けた方式**。rgitui を「PR/Issue は参考、avatar resolver は反面教師」と切り分けるべき。kagi は ADR-0037 通り **noreply パース + Commits API バッチ**を維持(privacy 優位)。
- 一方、PR/Issue panel 側の「未認証で public を読み、401/403/404 を `auth_required` 化してサインイン導線にする」設計は **ADR-0037 の未認証方針と完全に整合**し、そのまま設計移植してよい。

### kagi への PR/Issue 設計案

- **データ層**(rgitui_git 相当 or 新規 `github` module): git2 で remote URL から `owner/repo` を抽出(github.com のみ)。`PullRequest`/`Issue`/`Comment` を serde で parse。**HTTP は kagi の http stack(ADR-0037 で決める ureq or gpui http)に置換**。
- **認証**: v0 は未認証 + 任意 PAT(Settings に貼付)。PAT は **keyring 採用を検討**(rgitui の account 名前空間方式を Port)。Device Flow は v1 以降(client_id 取得が必要)。ADR-0037 の `KAGI_OFFLINE=1` を PR/Issue fetch にも適用し、headless テストを決定的に。
- **キャッシュ/rate limit**: rgitui の 60s TTL(`Instant` 比較)をそのまま Port。403/404→`auth_required` 空状態の UX、org 制限文言の書き換えも Port 価値大。
- **UI**: kagi の Bottom Panel(ADR-0007/0017)に "Pull Requests"/"Issues" タブを追加、`uniform_list` 仮想化(kagi は T008 で導入済の手法と同型)。detail は Compare/Inspector の隣に出すか専用パネル。Markdown は別途レンダラ要(kagi 未実装なら最小実装 or `gpui-component` の機能調査)。

#### チケット案サマリ(PR/Issue)
- **T-GHA-1**: remote URL → owner/repo 抽出 + GitHub REST client(http stack 置換、未認証 GET、`auth_required` エラー整形、60s TTL)。
- **T-GHA-2**: PR 一覧 panel(uniform_list、filter セグメント、state 色分け、loading skeleton、空状態のサインイン導線)。
- **T-GHA-3**: Issue 一覧 panel(PR と対称、`/search/issues` 検索)。
- **T-GHA-4**: detail view + Markdown 最小レンダラ + コメント取得。
- **T-GHA-5**(later): PAT を keyring 保管(account 名前空間)/ Device Flow / レビュー投稿。

## 3. diff heat map(観点③)

### 正体: `DiffStat`「5 ブロック比率バー」(`crates/rgitui_ui/src/diff_stat.rs`)

- **literal "heatmap" は存在しない**。「ファイルごとの heat map 的表示」の正体は **DiffStat = +N / −M テキスト + 5 個の小ブロックの比率バー**。
- **算出**: per-file `additions`/`deletions` は git2 の `patch.line_stats()`(`rgitui_git/src/project/diff.rs` L412)。commit 合計は files の sum(L392-393)。
- **可視化方式**(熱量=色強度のグラデーションでは**ない**):
  ```rust
  let total = added + removed;
  let green = (added * 5).div_ceil(total).min(5); // 緑ブロック数
  let red   = 5 - green;                            // 残りは赤
  // 各ブロック: div().w(px(4.)).h(px(10.)).rounded(px(1.)).bg(色)
  ```
  → 緑(追加)/赤(削除)/neutral(変更なし)の**離散 5 段ブロック**。各ファイル行の右端に同じ位置で並ぶため、commit 全ファイルを縦に見ると**色の強弱が列をなして「ヒートマップ的」に見える**(実体は per-file ミニ棒グラフ)。
- 出現箇所: detail_panel の flat ファイルリスト・tree ビュー・ヘッダ(commit 合計)。`vc_added`/`vc_deleted` テーマ色を使用。

### kagi の Inspector / Compare への適用案

- **そのまま Port 可**(MIT、純 gpui、HTTP/Zed 依存なし)。ロジックは ~80 行で、kagi の theme(W9)の `Added`/`Deleted` 相当色 + gpui-component の `div` で再実装も容易(**Reimplement でも低コスト**)。
- 適用先: ADR-0015 Inspector の changed-files 行 / ADR-0016 diff の各ファイル見出し / ADR-0026 Compare の差分ファイル一覧。各行に `+N −M [▮▮▮▯▯]` を出すだけで情報密度が上がる。
- **本物の heat map に強化する案**(kagi 独自): ブロック比率の代わりに **変更行数の絶対量を色の opacity/lightness にマップ**(例: `Added` 色を `a = clamp(adds / repo_p95, 0.1, 1.0)`)すれば真の heat map。rgitui は離散比率止まりなので、ここは kagi が一歩進められる差別化点。ただし YAGNI 配慮で v0 は DiffStat 同等で十分。

#### チケット案サマリ(heat map)
- **T-DST-1**: `DiffStat` 相当コンポーネント(+N/−M + 5 ブロック比率バー)を kagi theme 色で実装、Inspector/Compare のファイル行に挿入。
- **T-DST-2**(later): 絶対変更量 → opacity マップの「真 heat map」モード(repo 全体の分布で正規化)。

## 4. UI の綺麗さの正体(観点④)

「綺麗さ」は派手な装飾でなく **(a) 一貫した spacing/radius トークン (b) 単一セマンティック色からの tint 生成 (c) loading skeleton (d) Catppuccin ベースの低彩度パレット** の合わせ技。具体策をコンポーネント単位で:

| 効いている要素 | rgitui の実装 | kagi が取り入れる具体策 |
|---|---|---|
| **Tinted Badge/chip** | `badge.rs`: 1 つの `Color` から `bg = color.a=0.15` / `border = color.a=0.3` / 文字=その色を自動生成。rounded 10px・h20・px6・py1・SEMIBOLD・XSmall・truncate | kagi の Badge/ref バッジ(T008)を「1 色入力 → 0.15/0.3 alpha 派生」に統一。GitHub label の hex もこの式で chip 化(`prs_panel` `label_color`) |
| **px 間隔トークンの徹底** | gap=2/4/6/8、px=3/12/16、py=1/2/3/12/14 と少数の値を反復。row 高 36px、toolbar 32px、badge 20px、avatar 円 20px | kagi で spacing/サイズの**定数セットを固定**し全 panel で反復(マジックナンバー散乱を防ぐ) |
| **角丸の階層** | chip=10、ボタン=md/6、カード=8、空状態箱=12、ブロック=1。要素サイズに比例した radius | kagi のカード/ボタン/バッジで radius を 3 段(small6/card8/pill10+)に標準化 |
| **境界の薄さ** | `border_variant`(最も薄い境界)を区切りに多用、`border_transparent`(a=0)を非選択時に置いてレイアウトずれ防止 | 「非選択でも透明 border を確保して選択時にレイアウトが跳ねない」テクは Port 価値大 |
| **左ボーダー選択表現** | list 行は `border_l_2` + 選択時のみ accent 色 + 薄い bg。チェックや塗りつぶしより上品 | kagi の commit list / panel list 選択 UI に採用 |
| **Loading skeleton** | `render_loading_skeleton`: グレー矩形 6 行。スピナーより「速く見える」 | kagi の非同期ロード(repo open・diff prefetch・将来の PR fetch)に skeleton を導入 |
| **空状態の作法** | アイコン Large(Placeholder 色)+ 見出し SEMIBOLD Muted + 説明 XSmall。`ghost_element_background` の角丸箱で中央寄せ | kagi の空 panel(履歴なし・差分なし)を同パターンで |
| **意味付き色の単一定義** | `colors.rs`: `vc_added/modified/deleted/conflict/renamed/untracked` を**全テーマで定義**、`Color` enum が render 時に解決。`icon_color()` は icon 用スロットに分岐 | kagi theme(W9)に VC 6 色 + status 5 色のセマンティック層を確保(既にあれば追認)。`Color` enum→theme 解決の間接層は良い設計 |
| **status background = 同色 a=0.10〜0.15** | success/error/warning/info の `*_background` は本体色の低 alpha。トースト・結果バナーに使用 | kagi のトースト/結果表示の bg をこの式に統一(色を増やさず統一感) |
| **Catppuccin 基盤の配色** | Mocha/Latte/One Dark/Dracula/GitHub Dark/Cream&Blue/High Contrast を HSL 直書き + JSON テーマ(`assets/themes/*.json`)。lane 色は彩度を「boosted」 | ADR-0036 の kagi テーマ機構に Catppuccin 系プリセットを追加検討。グラフ lane は通常 VC 色より彩度を上げると視認性◎ |
| **アバター: 即時イニシャル→差し替え** | `avatar_resolver` は背景解決 + ディスク/メモリキャッシュ、解決時のみ `cx.refresh()` | ADR-0037 と同方針(ただし resolver の email 検索は採らない) |

- 補足: rgitui は `cx.notify()` の churn を抑える工夫(`configure` の idempotent ガード、avatar の NotFound 時は refresh しない)が随所にあり、**「重い再描画を出さない」ことが体感の滑らかさに寄与**。kagi でも notify 発火条件を絞るのは有効。
- screenshot は `assets/screenshots/main.png` 1 枚のみ(コードからの推測が中心。視覚詳細は実機/スクショで要確認)。

## 5. 提案分類テーブル(ADR-0031 の流儀)

| 候補 | 分類 | 理由 | コスト | リスク |
|---|---|---|---|---|
| PR/Issue REST フロー(未認証 GET + auth_required 化 + 60s TTL + 403 文言書換) | **Port** | MIT。設計が秀逸。HTTP I/O のみ kagi http stack に置換すれば移植可 | 中 | 低(Zed http 依存を持ち込まないこと) |
| トークン keyring 保管(account 名前空間 + レガシー移行) | **Port** | MIT、`keyring` crate は単独依存可。kagi が PAT を持つなら良い土台 | 小〜中 | 低(依存 1 追加) |
| OAuth Device Flow | **Study only → later Port** | MIT。ただし自前 GitHub App client_id が必要。v0 は PAT で十分 | 中 | 低 |
| `DiffStat` 比率バー(heat map の正体) | **Port or Reimplement** | MIT・純 gpui・~80行。Inspector/Compare に即適用。再実装も低コスト | 小 | 低 |
| 真 heat map(opacity 正規化) | **Reimplement(kagi 独自拡張)** | rgitui に無い。kagi の差別化余地 | 中 | 低 |
| UI 部品の tint 派生 Badge / skeleton / 左border選択 / 空状態作法 | **Reimplement(設計言語)** | kagi は gpui-component 採用済。コードでなく「式・トークン・作法」を取り込む | 小 | 低 |
| セマンティック色レイヤ(`Color` enum→theme 解決, VC6/status5) | **Study/Reimplement** | kagi theme(W9)に同型があれば追認、無ければ概念採用 | 小 | 低 |
| Catppuccin 系テーマプリセット + JSON テーマ | **Study** | ADR-0036 と整合。配色値は参考、コードは kagi 機構で | 小 | 低 |
| `avatar_resolver`(email/name の `/search/users`+gravatar) | **Reject** | **ADR-0037 が明示的に避けた privacy/rate-limit 方式**。kagi は noreply+Commits API を維持 | 0 | 中(採ると ADR 違反) |
| 自前 UI crate 全体(rgitui_ui 30+ 部品) | **Reject(全面流用)/ Study(個別)** | kagi は gpui-component 採用済(ADR-0006)。個別に良い式だけ Reimplement | 0 | 低 |
| gpui を Zed git rev 固定 + Zed http_client 依存 | **Reject** | kagi は crates.io gpui 0.2.2 pin。依存系統が別 | 0 | 高(導入すると依存衝突) |
| destructive をそのまま実行する操作系 | **Reject** | kagi の plan→confirm→preflight→execute→verify→oplog 安全パイプラインと非整合 | 0 | 高 |

## 6. 確認できなかった事項 / 注意

- 視覚的な「綺麗さ」はコード(spacing/色/radius)から推定。screenshot は 1 枚のみで、アニメーション(splash、graph の Bezier)の質感は実機未確認。
- rgitui の gpui git rev(`f3fb4e04…`)と kagi の crates.io 0.2.2 の API 差分は未照合。**コードを Port する際は kagi 側 API で書き直す前提**(機械コピー不可)。
- PR/Issue の per_page 上限超(ページネーション)は rgitui では 50 件固定で未対応。kagi で大量 PR を扱うなら追加設計が要る。
- MIT 表記保持義務: rgitui からコードを Port する場合、当該ファイル/NOTICE に原典著作権表記(`Copyright (c) 2026 rgitui contributors`, MIT)を残すこと(ADR-0031 §2)。
