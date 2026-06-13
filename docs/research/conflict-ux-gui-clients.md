# コンフリクト解決 UX 調査: GUI Git クライアント比較

対象: GitKraken / Fork / SourceTree / GitHub Desktop
目的: kagi(safety-first Git GUI / Rust + gpui)のコンフリクト解決 UX 設計のための競合調査。
調査手法: 公式ドキュメント・リリースノート・フィードバックボード・課題トラッカー・レビュー記事の Web 調査。実機検証はしていないため、UI 文言や挙動の細部には **推測** を明示する。
調査日: 2026-06-13

ルーブリック(各ツール共通):
1. コンフリクト表示単位(file / hunk / line / symbol)
2. 3-way か 2-way か / result(出力)ビューの有無
3. ours / theirs / base / result の提示方法(ラベルの実文言)
4. hunk 単位選択 / line 単位編集の可否
5. コンフリクトファイル一覧 / 未解決・解決済みの可視性
6. merge / rebase / cherry-pick の continue / abort / skip 連携
7. 解決ステップの undo / redo
8. binary / rename-delete / modify-delete コンフリクトの扱い
9. 巨大ファイル・大量コンフリクト時の挙動
10. 良い点 / 悪い点 / kagi に取り込むべき点 / 取り込まない方がよい点

---

## 1. GitKraken Desktop

出典:
- 公式機能ページ: https://www.gitkraken.com/features/merge-conflict-resolution-tool
- 公式ブログ(ツール解説): https://www.gitkraken.com/blog/merge-conflict-tool
- ヘルプ: https://help.gitkraken.com/gitkraken-desktop/branching-and-merging/
- フィードバック(rename conflict 要望): https://feedback.gitkraken.com/suggestions/361139/handle-rename-conflicts
- フィードバック(パフォーマンス): https://feedback.gitkraken.com/suggestions/365269/improve-performance-on-merge-conflicts
- 3-way UI 要望: https://feedback.gitkraken.com/suggestions/192482/

### 1. コンフリクト表示単位
ファイル単位でエントリ化される。コンフリクトしたファイルは Commit Panel に一覧表示され、ファイルをクリックすると専用の Merge Tool が開く(出典: 機能ページ)。Merge Tool 内では hunk(チャンク)単位と line 単位の双方で選択できる。

### 2. 3-way / 2-way・result ビュー
**3-way 構成**。画面上半分に 2 つの版を左右に並べ、下半分に「output(出力結果)」のライブプレビューを置く(出典: ブログ「Two side-by-side panels (top half) ... Output panel (bottom half)」)。3 ペインがスクロール同期する(synchronization functionality)。
※ base(共通祖先)を独立した第 4 ペインとして常時見せているかは資料からは確認できず **推測**: 一般的には current/target の 2 ソース + output 構成で、base 専用ペインは持たない可能性が高い。

### 3. ours / theirs / base / result のラベル
機能ページ上の説明では **"current"(自分のブランチ)/ "target"(マージ先に取り込むブランチ)/ "output"(結果)** という語が使われる。"ours/theirs" という生の Git 用語ではなく current/target という語彙にマップしている点が特徴(出典: 機能ページ要約)。

### 4. hunk 選択 / line 編集
両対応。
- 各コンフリクトブロック横の **チェックボックス** で hunk(チャンク)丸ごと採用。
- ハイライトされた **個別行をクリック** すると output に追加(line 単位)。
- **"Take All"** ボタンで片側全体を採用。
- output ペインで **手動編集**(任意のテキストを直接入力)も可能。
- 複数コンフリクト間は **矢印ボタン** で移動(出典: ブログ)。

### 5. ファイル一覧 / 解決状態
Commit Panel にコンフリクトファイルが列挙される。解決操作後は **"Save and Mark Resolved"**(保存して解決済みにマーク)でステージされ一覧から落ちる(出典: ブログ)。Team View 機能では共有ファイルに「衝突しそう」アイコンを事前表示する(早期検知、出典: 機能ページ)。

### 6. continue / abort / skip 連携
全コンフリクトを解決後 **"Save, mark resolved, and commit"** でマージコミットを作成して完了。手動マージへ戻す **"reset to manual merge"** がある(出典: 機能ページ)。
※ rebase/cherry-pick の continue/skip/abort ボタンの実文言は資料で未確認 **推測**: GitKraken は rebase 中のインタラクティブ UI を持つため continue/abort 相当の操作はあると思われる。

### 7. undo / redo
AI 提案後に **"Accept, tweak lines manually, or reset"** で戻せる(出典: 機能ページ)。output への行追加も再選択で調整可能。
※ 解決ステップ単位の汎用 undo スタックがあるかは未確認 **推測**: グローバルな Undo(GitKraken の特徴機能)はコミット等の操作向けで、Merge Tool 内の細粒度 undo とは別物の可能性。

### 8. binary / rename-delete / modify-delete
**弱点として既知**。rename + 変更を、旧名への変更とコンフリクトさせると「deleted file conflict + new file conflict」として見え、Merge Tool で解決しづらい(出典: rename conflict フィードバック)。binary の専用扱いは資料で確認できず **推測**: テキスト前提の行マージ UI のため、binary は「ours/theirs どちらか選択」のみに縮退すると思われる。

### 9. 巨大ファイル・大量コンフリクト
**パフォーマンス問題が報告**: 変更ファイル数が多いと、コンフリクトファイルをクリックして処理する速度が極端に遅い(出典: パフォーマンスフィードバック)。

### 10. 評価
- 良い点: 3-way + output ライブプレビューが 1 画面に収まり外部ツール不要。current/target という人間語ラベル。hunk/line/Take All/手動編集の選択粒度が豊富。3 ペインのスクロール同期。AI 自動解決(説明付き)。
- 悪い点: in-app output エディタは **有料ライセンス専用**。rename/delete 系が苦手。大量ファイル時に重い。
- kagi に取り込むべき: ①"current/target" のような **人間語ラベル**(ours/theirs を避ける)、②output ライブプレビューと **スクロール同期**、③hunk チェックボックス + 行クリック + Take All の **多段粒度**。
- 取り込まない方がよい: ①コア機能の有料ゲーティング(safety-first を掲げる kagi では基本機能は無償であるべき)、②AI 自動解決を初期スコープに入れること(kagi の安全性志向に対し、自動書き換えは検証コストが高い)。

---

## 2. Fork (Mac / Windows)

出典:
- リリースノート(Win): https://fork.dev/releasenoteswin
- リリースノート総覧: https://git-fork.com/releasenotes
- 1.0.73 ブログ: https://fork.dev/blog/posts/fork-1.0.73/
- 1.0.79 ブログ: https://fork.dev/blog/posts/fork-1.0.79/
- Tracker(解決状態を一覧表示要望): https://github.com/fork-dev/Tracker/issues/1304

### 1. コンフリクト表示単位
ファイル単位でリスト化し、ファイルを開くとコンフリクトブロック単位で解決していく。手動編集は line 単位まで可能(出典: 1.0.73)。

### 2. 3-way / 2-way・result ビュー
**両方を選べる**。デフォルトの 2-way ビューに加え、1.0.73(2019-02-01)で **"Alternative 3-column layout in merge conflict resolver"**(3 カラムの代替レイアウト)を追加(出典: リリースノート)。result の手動編集に対応。

### 3. ours / theirs / base / result のラベル
1.0.73 で **"For merge conflicts show branch names instead of ours/theirs"**(ours/theirs の代わりにブランチ名を表示)を導入(出典: リリースノート)。さらに 1.0.79 で **theirs-ours の表示順を変更**(出典: 1.0.79)。生 Git 用語ではなく実ブランチ名で提示する設計。

### 4. hunk 選択 / line 編集
1.0.73 で **"Improved merge conflict resolver with manual editing support"** と **「end result can be edited manually(結果を手動編集可能)」**、加えて **"Option to resolve multiple conflicts at once"**(複数コンフリクトを一括解決)を追加(出典: 1.0.73)。ブロック採用 + 手動編集の両対応。

### 5. ファイル一覧 / 解決状態
コンフリクトファイル一覧を持つ。ただし **「解決済みかどうかをリストで示してほしい」という要望が Tracker に上がっており**(Issue #1304)、解決状態の可視性は弱かった時期がある(出典: Tracker #1304)。

### 6. continue / abort / skip 連携
rebase 継続に対応(1.0.72 で submodule リポジトリの **"Continue rebase" ボタン**の不具合を修正、という記述あり、出典: リリースノート)。
※ cherry-pick/abort/skip の文言は未確認 **推測**: Fork は rebase/cherry-pick の continue 系操作をツールバー/バナーで提供していると思われる。

### 7. undo / redo
※ 資料で明示確認できず **推測**: result が通常のテキストエディタとして編集可能なため、エディタ内 undo は効くが、解決ステップ単位の専用 undo スタックは未確認。

### 8. binary / rename-delete / modify-delete
※ 専用扱いの記述は資料で確認できず **推測**: テキスト中心の resolver のため、binary・delete 系は片側選択へ縮退すると思われる。

### 9. 巨大ファイル・大量コンフリクト
ネイティブアプリで高速という一般評価("fast and friendly")。一括解決オプションが大量コンフリクト緩和に寄与する(出典: 1.0.73)。具体的な巨大ファイル挙動は未確認 **推測**。

### 10. 評価
- 良い点: **ブランチ名ラベル**(ours/theirs を排した最良の実装例)。2-way/3-way の切替。手動編集 + 一括解決。ネイティブで軽快。クリーンな UI という高評価。
- 悪い点: 解決状態の一覧可視性が弱かった(要望 #1304)。3-way は「代替」レイアウト扱いで主役でない。
- kagi に取り込むべき: ①**実ブランチ名ラベリング**(`feature/x` ← → `main` のように表示)、②2-way ↔ 3-way の **ユーザー切替**、③複数コンフリクト一括解決オプション。
- 取り込まない方がよい: 解決状態をリストに出さない設計(まさに不満点なので kagi は最初から解決済み/未解決を明示すべき)。

---

## 3. SourceTree (Atlassian)

出典:
- 外部マージツール設定(公式系手順): https://medium.com/@ryanmaulanaputra/how-to-resolve-merge-conflict-in-sourcetree-using-vs-code-9365d9b79af0
- 解説: https://medium.com/@mwagnerdev/resolve-conflicts-with-sourcetree-d1728df0e7f6
- ラベル反転バグ(SRCTREE-1670): https://jira.atlassian.com/browse/SRCTREE-1670
- 複数解決バグ(Community): https://community.atlassian.com/forums/Sourcetree-questions/Bug-Unable-to-resolve-multiple-merge-conflicts-using-Resolve/qaq-p/2913012

### 1. コンフリクト表示単位
ファイル単位。コンフリクトファイルは **オレンジ色の三角警告(感嘆符付き)** アイコンで一覧上に表示される(出典: Medium 解説)。実際のブロック/行解決は外部マージツールに委譲。

### 2. 3-way / 2-way・result ビュー
**SourceTree 自体には本格的な内蔵 3-way マージエディタが無い**(かつての内蔵ビューアは限定的)。実用上は **外部マージツールを起動**して解決する設計。P4Merge / TortoiseMerge / VS Code / Beyond Compare 等を「Diff/Merge」設定で指定(出典: Medium 各記事)。result ビューは外部ツール側に依存。

### 3. ours / theirs / base / result のラベル
右クリック「Resolve Conflicts」配下に **"Resolve Using Mine"** / **"Resolve Using Theirs"** がある。**重大な UX 落とし穴**: ラベルが実挙動と反転していた既知バグ(SRCTREE-1670)。"Resolve Using Theirs..." が自分の変更を適用し、"Resolve Using Mine..." が相手の変更を適用していた。39 票・36 ウォッチを集め、**2023-12-22 に Fixed** とされる(出典: SRCTREE-1670)。外部ツール側のラベルはツール依存。

### 4. hunk 選択 / line 編集
内蔵では hunk/line の対話的選択は弱く、**"Resolve Using Mine/Theirs"(ファイル丸ごと片側採用)** が主。細粒度の選択・編集は外部ツールに依存(出典: Medium 各記事)。

### 5. ファイル一覧 / 解決状態
コンフリクトファイルにオレンジ三角アイコン。**"Mark Resolved"** で保存し一覧から除外、全ファイルで繰り返す(出典: Medium 解説)。

### 6. continue / abort / skip 連携
**"Restart Merge"**(マージやり直し)を「Resolve Conflicts」メニューから選べる。マージツールを誤って閉じても再起動可能(出典: Medium 解説)。
※ rebase/cherry-pick の continue/skip の文言は未確認 **推測**: SourceTree は rebase 中の操作を別 UI で提供。

### 7. undo / redo
※ 内蔵の解決 undo は弱い **推測**。実質「Restart Merge」でやり直す運用。

### 8. binary / rename-delete / modify-delete
※ 専用 UI の明示記述なし **推測**: "Resolve Using Mine/Theirs"(片側採用)で対処する運用と思われる。

### 9. 巨大ファイル・大量コンフリクト
**複数コンフリクトの一括解決がうまく動かないバグ**が報告されている(Community: "Unable to resolve multiple merge conflicts using Resolve using Theirs")。大量時の信頼性に難。

### 10. 評価
- 良い点: 任意の外部マージツールに委譲できる柔軟性。"Mark Resolved" / "Restart Merge" という分かりやすい安全操作。コンフリクトアイコンの一覧視認性。
- 悪い点: **内蔵 3-way エディタが実質無い**(外部ツール必須)。**ラベル反転という致命的 UX バグの歴史**(SRCTREE-1670)。一括解決バグ。
- kagi に取り込むべき: ①**"Restart Merge"(マージやり直し)という安全な巻き戻し操作**(safety-first と相性◎)、②コンフリクトファイルの **明確な状態アイコン**。
- 取り込まない方がよい: ①**Mine/Theirs のような曖昧で反転しうるラベル**(Fork のブランチ名方式を採るべき)、②解決を外部ツールに丸投げする設計(kagi は内蔵 3-way を持つべき)。

---

## 4. GitHub Desktop

出典:
- 公式ブログ(1.5 でコンフリクト解決): https://thenextweb.com/news/github-desktop-1-5-makes-it-easy-to-resolve-frustrating-merge-conflicts
- 課題 #6216(外部エディタ未設定時の説明欠如): https://github.com/desktop/desktop/issues/6216
- 課題 #6060(既定エディタ無いとマージ不能): https://github.com/desktop/desktop/issues/6060
- 課題 #21500(rebase がコンフリクトで詰む): https://github.com/desktop/desktop/issues/21500
- 課題 #13756(解決済みなのに未解決と言われる): https://github.com/desktop/desktop/issues/13756
- 既定エディタ設定ドキュメント: https://docs.github.com/en/desktop/configuring-and-customizing-github-desktop/configuring-a-default-editor-in-github-desktop

### 1. コンフリクト表示単位
ファイル単位。マージ時に **「Resolve conflicts before merging origin/master into master」** というタイトルのダイアログが開き、コンフリクトファイルの一覧を表示する(出典: #6216)。**行/hunk の選択 UI は内蔵していない** — 実際の編集は外部エディタ内で行う。

### 2. 3-way / 2-way・result ビュー
**内蔵の 3-way/2-way マージエディタは無い**。GitHub Desktop はファイル一覧を出し、各ファイルを **外部エディタで開く**だけ(出典: ブログ 1.5)。result は外部エディタ上のファイルそのもの。

### 3. ours / theirs / base / result のラベル
内蔵 UI には ours/theirs ペインが無い。外部エディタで開いた際の **Git コンフリクトマーカー** に依存: `<<<<<<<` の上が「desktop 側(現在のローカル)」、`=======` を挟んで下が「remote 側」、`>>>>>>>`(出典: ブログ 1.5 / コミュニティ解説)。素の Git マーカー文化をそのまま露出。

### 4. hunk 選択 / line 編集
**内蔵では不可**。各ファイルに **"Open in Editor"** ボタンがあり、外部エディタ(VS Code 等)で手動編集する(出典: #6216)。

### 5. ファイル一覧 / 解決状態
ダイアログにファイル一覧があり、解決が済むと表示が更新され、全解決で **"Continue merge"** が活性化(推測を含むが #13756 で「解決済みなのに未解決扱い」になる不具合が報告されており、状態判定が不安定な場面がある)。

### 6. continue / abort / skip 連携
ダイアログに **"Continue merge"** と **"Abort merge"** がある(出典: 検索要約 / #6216 文脈)。ただし **rebase はコンフリクトで詰む既知問題**: 手動解決が必要な場面で外部エディタ起動プロンプトもブランチ選択も出ず、rebase がスタックして実質使えない(出典: #21500)。

### 7. undo / redo
内蔵の解決 undo は無く、外部エディタ依存。誤ったら **"Abort merge"** でやり直す運用 **推測**。

### 8. binary / rename-delete / modify-delete
binary は GitHub Web 同様 **「競合する行変更」以外は内蔵で解決不可**の制約が近い思想 **推測**。GitHub Desktop はそもそも行マージ UI が無いため、これらは外部エディタ/CLI 任せ。

### 9. 巨大ファイル・大量コンフリクト
内蔵処理が無いぶん軽い反面、**既定エディタが未設定だと "Open in Editor" が無効化されてユーザーが詰む**(出典: #6060, #6216)。ダイアログが小さく大量ファイルで扱いづらいとの指摘もある(#7115)。

### 10. 評価
- 良い点: 学習コストが低い。コンフリクトファイル一覧 + "Open in Editor" + "Continue/Abort merge" という最小フローは分かりやすい。初心者を Git マーカーに自然に触れさせる。
- 悪い点: **内蔵マージエディタが無い**(外部エディタ必須、未設定だと操作不能 #6060)。生の `<<<<<<<` マーカーを露出。**rebase がコンフリクトで詰む**(#21500)。状態判定の不具合(#13756)。
- kagi に取り込むべき: ①**"Continue merge" / "Abort merge" の明確な二択バナー**(操作の出口が常に見える)、②ファイル一覧 + 各ファイルアクションというシンプル骨格。
- 取り込まない方がよい: ①**内蔵エディタを持たず外部依存にする設計**(kagi は内蔵 3-way 必須)、②生 Git マーカーをそのまま見せること、③rebase 時に解決導線が消える設計(kagi は merge/rebase/cherry-pick で同一の解決 UI を出すべき)。

---

## 横断まとめ(kagi 設計への示唆)

| 観点 | GitKraken | Fork | SourceTree | GitHub Desktop |
|---|---|---|---|---|
| 内蔵 3-way エディタ | あり(有料) | あり(2/3-way 切替) | 実質なし(外部委譲) | なし(外部委譲) |
| result ライブプレビュー | あり(同期) | あり(手動編集) | 外部依存 | 外部依存 |
| ours/theirs ラベル | current/target | **ブランチ名** | Mine/Theirs(反転バグ歴) | 生 Git マーカー |
| hunk/line 粒度 | hunk+line+Take All | ブロック+手動 | 片側採用中心 | なし(外部) |
| 解決状態の可視性 | Mark Resolved | 弱い(要望あり) | アイコン+Mark Resolved | 一覧(不具合歴) |
| abort/continue | commit/reset | Continue rebase | Restart Merge | Continue/Abort merge |

### kagi が採るべき設計(opinionated)
1. **ラベルは実ブランチ名**(Fork 方式)。ours/theirs/Mine/Theirs は反転事故(SRCTREE-1670)を生むので避ける。3-way の中央に base を置き、左右にブランチ名を出す。
2. **内蔵 3-way + result ライブプレビュー + スクロール同期**(GitKraken 方式)を無償で。外部ツール必須(SourceTree/GitHub Desktop)は safety-first に反する(未設定で詰む)。
3. **粒度は hunk チェックボックス + 行クリック + 片側全採用 + 手動編集** の 4 段(GitKraken+Fork の良いとこ取り)。
4. **解決状態を一覧に常時可視化**(未解決 N / 解決済み M)。Fork の不満点を最初から潰す。
5. **merge / rebase / cherry-pick で同一の解決 UI と Continue/Abort/Skip バナー**を出す(GitHub Desktop の rebase 詰み #21500 を避ける)。出口操作を常時表示。
6. **"Restart/Abort" による安全な巻き戻し**(SourceTree の Restart Merge 思想)を一級市民に。kagi の安全性志向に最も合致。
7. **binary / rename-delete / modify-delete に専用 UI**(片側選択を明示)。全 GUI が弱い領域なので差別化点になりうる。
8. **大量ファイル/巨大ファイルでの応答性**(GitKraken の重さ #365269 を反面教師に)。仮想化リスト前提で設計。
9. AI 自動解決は **初期スコープ外を推奨** — kagi の安全性検証コストに対し費用対効果が読めない(取り込むなら「提案 → 人間が承認」の非破壊フローに限定)。

> 注: 各ツールの未確認・推測箇所(base ペインの有無、undo スタックの粒度、binary 専用 UI の有無など)は実機検証で確定すべき。本調査は公開資料ベースであり、UI 文言はバージョン差で変動しうる。
