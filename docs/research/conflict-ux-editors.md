# コンフリクト解決 UX 調査: 既存マージエディタ 5 種

対象: VSCode マージエディタ (3-way / 2022〜) / JetBrains (IntelliJ 系) マージツール / Meld / KDiff3 / Beyond Compare

目的: kagi (safety-first Git GUI, Rust + gpui) のコンフリクト解決 UX 設計に向け、各ツールの提示単位・ラベル・操作モデル・Git 連携・失敗事例を横断比較する。

各ツール共通ルーブリック:
1. コンフリクト提示単位 (file/hunk/line/symbol)
2. 3-way vs 2-way / 結果ビューの有無と編集可否
3. ours/theirs/base/result の提示 (正確なラベル)
4. hunk 単位選択 / 行単位編集 / 組み合わせ (両方採用・順序)
5. コンフリクトファイル一覧 & 未解決/解決の進捗可視化
6. Git operation の continue/abort/skip 連携
7. 解決ステップの undo/redo
8. binary / rename-delete / modify-delete コンフリクト
9. 巨大ファイル / 大量コンフリクト時の挙動
10. 良い点 / 悪い点 / kagi に取り込むべき点 / 取り込まない方がよい点

---

## 1. VSCode マージエディタ (3-way, 2022 導入)

2022 年 (VSCode 1.69〜70 前後) に「3-way merge editor」がデフォルト化され、強い反発を招いた。公式ドキュメント: <https://code.visualstudio.com/docs/sourcecontrol/merge-conflicts> / <https://code.visualstudio.com/docs/sourcecontrol/overview>

### 提示単位
- 基本は **hunk (conflict block) 単位**。各コンフリクトブロックに対し CodeLens / チェックボックスで採否を選ぶ。
- Result パネルは行単位で直接編集可能なので、実質 line 単位の手編集も可能。

### 3-way vs 2-way / 結果ビュー
- **3-way**。3 ペイン構成:
  - **Incoming (左)**: マージしてくるブランチの変更
  - **Current (右)**: 現在のブランチの変更
  - **Result (下)**: 保存される最終結果。**直接編集可**
  (公式: "Incoming (left)", "Current (right)", "Result (bottom): the merged result that will be saved")
- 旧来の **インラインコンフリクトマーカー (`<<<<<<<` 表示) ビュー** もフォールバックとして残存。`git.mergeEditor` 設定で切替。

### ours/theirs/base/result ラベル
- 3-way ペイン側: **Incoming / Current / Result**。Base は three-dot メニューから任意表示。
- 旧インラインビューの CodeLens: **Accept Current Change / Accept Incoming Change / Accept Both Changes / Compare Changes**。
- 「Incoming = theirs / Current = ours」だが、git の `--ours/--theirs` とラベルが一致せず混乱を生んだ (Issue #166105: "Accept" は実際には「採用」ではなく「その結果に置換」なので "Set to Incoming / Set to Current" にすべき、という提案 <https://github.com/microsoft/vscode/issues/166105>)。

### hunk 選択 / 行編集 / 組み合わせ
- 各ブロックで **Accept Incoming / Accept Current / Accept Combination / Ignore**。
- **Accept Combination (両方採用)** に順序オプションあり: **Accept Combination (Incoming First)** / **Accept Combination (Current First)** (<https://dev.to/adiatiayu/how-to-resolve-merge-conflicts-using-the-merge-editor-feature-on-vs-code-pic>)。
- Result ペインを直接編集して任意の解決を作れる。

### ファイル一覧 & 進捗可視化
- Source Control ビューの "Merge Changes" にコンフリクトファイル一覧。
- Result エディタ右に **conflict count indicator (残未解決数)**。全解決後 **Complete Merge** ボタンでステージ。

### Git operation 連携
- **Complete Merge** でステージ+エディタクローズ。abort/skip は SCM/ターミナル経由が中心で、マージエディタ内のボタンとしては弱い (continue 相当のみ手厚い)。

### undo/redo
- Result はエディタなので標準の Undo/Redo (Ctrl+Z) が効く。採否トグルも編集として巻き戻せる。

### binary / rename-delete / modify-delete
- テキスト前提。binary や submodule conflict では Accept ボタンが出ない事例が報告 (Issue #164900 submodule で解決不能 <https://github.com/microsoft/vscode/issues/164900>)。modify/delete・rename/delete のような「片側削除」系はマージエディタが想定する 3 テキスト構造に乗らず、UI が機能しない/分かりにくいとの不満。

### 巨大ファイル / 大量コンフリクト
- **重大な弱点**。大ファイルで読み込みに数分〜十数分かかる報告多数:
  - Issue #157166 "Merge Editor is extremely slow" <https://github.com/microsoft/vscode/issues/157166>
  - Issue #192469 "takes many minutes to load" <https://github.com/microsoft/vscode/issues/192469>
  - Issue #206013 544kb の min.js で十数分 <https://github.com/microsoft/vscode/issues/206013>
- 5 コミット超の rebase で 1 ファイルあたり 2 分超との報告も。

### 良い点
- 3 ペイン + 編集可能 Result + 残コンフリクト数カウンタという「標準形」を確立。
- Accept Combination の順序指定 (Incoming/Current First) は明快で実用的。
- インラインマーカーと 3-way の二段構え (軽い衝突は CodeLens、重いものは専用エディタ)。

### 悪い点 (= 反面教師の宝庫)
- **デフォルト強制変更**への大反発。「旧ビューに戻したい」が殺到 (Issue #157610 <https://github.com/microsoft/vscode/issues/157610>, #159516 "the new merge conflict resolution interface is bad" <https://github.com/microsoft/vscode/issues/159516>)。
- **ラベルの曖昧さ**: "Accept" の意味、Incoming/Current が ours/theirs のどちらか分からない (#166105)。
- **マウス必須**でキーボード操作が貧弱、accept のあたり判定が分かりにくい (#158523 "Can't accept... without using mouse" <https://github.com/microsoft/vscode/issues/158523>)。
- **巨大ファイルで実用不能**な性能。`git.mergeEditor:false` が効かない不具合報告も (#166950 <https://github.com/microsoft/vscode/issues/166950>)。

### kagi に取り込むべき点
- **Result = 編集可能ペイン + 残未解決数の常時表示 + 全解決後に初めて "完了/continue" を有効化** という安全フロー。
- **両方採用に順序を明示**する選択肢 (Incoming First / Current First)。

### 取り込まない方がよい点
- **既存メンタルモデルを壊す一括デフォルト強制**。kagi は旧来の inline マーカー理解とも橋渡しできる退路を必ず残す。
- **Incoming/Current のような git 用語と乖離したラベル**。kagi は git の ours/theirs/base と対応を明示する (推測: 「自分の変更 (ours)」「相手の変更 (theirs)」のように二重表記)。
- **大ファイルで全文を重い 3 ペインに展開する設計**。仮想化・遅延描画必須。

---

## 2. JetBrains IntelliJ IDEA 系マージツール

公式: <https://www.jetbrains.com/help/idea/resolve-conflicts.html> / ガイド <https://www.jetbrains.com/guide/java/tutorials/resolving-git-merge-conflicts/resolving-merges/>

### 提示単位
- **change (hunk) 単位**が基本。中央ペインは完全な編集可能エディタなので **行単位手編集**も可能。

### 3-way vs 2-way / 結果ビュー
- **3-way**。中央ペインが結果 (編集可)。
  - **左ペイン**: read-only ローカルコピー (your/working branch)
  - **中央ペイン**: フル機能エディタ = 解決結果 (初期内容は base リビジョン)
  - **右ペイン**: read-only リポジトリ側 (incoming)
- comparison ボタンで Base / Middle / Left / Right の各版を見比べ可。

### ours/theirs/base/result ラベル
- ファイル一覧ダイアログ "Files Merged with Conflicts" の 3 ボタン: **Accept Yours / Accept Theirs / Merge**。
- マージダイアログ内: **Accept Left / Accept Right** (ガター)、コンテキストメニュー **Resolve using Left / Resolve using Right**。
- 用語が「Yours/Theirs」(ファイル一覧) と「Left/Right」(マージ画面) で揺れる点に注意 (推測: ユーザは Left=ours, Right=theirs を都度確認する必要)。

### hunk 選択 / 行編集 / 組み合わせ
- 各 change のガターで **矢印 (accept)** か **X (ignore)**。左右どちらの矢印も押せば両方採用 (順序は押下順)。
- ツールバー: **Apply All Non-Conflicting Changes** / **Apply Non-Conflicting Changes from the Left Side / from the Right Side**。
- **Resolve simple conflicts** (単純な衝突を 1 クリックで自動解決) = いわゆる "magic wand"。
- 中央ペイン直接編集で任意解決。

### ファイル一覧 & 進捗可視化
- 最初に "Files Merged with Conflicts" ダイアログでコンフリクトファイル群を提示。閉じると Commit ツールウィンドウ Local Changes に **Merge Conflicts ノード** + Resolve リンクが残る。
- マージダイアログ上部に残 change 数や色分け (modified/deleted/added/conflicting)。

### Git operation 連携
- IDE の VCS フローに統合。各ファイル解決→Apply で確定し、最終的に commit へ。CLI mergetool としても使える (`idea merge` チュートリアル <https://www.jetbrains.com/help/idea/tutorial-use-idea-as-default-command-line-merge-tool.html>)。abort は VCS メニュー / Git ログ経由。

### undo/redo
- 中央ペインはエディタなので標準 Undo/Redo。accept 操作も取り消し可。

### binary / rename-delete / modify-delete
- modify/delete 等はファイル一覧ダイアログで「Accept Yours/Theirs」レベルの粗い選択にフォールバックする (推測: 3 テキストが揃わない衝突はファイル単位採否)。binary は差分表示不可で採否のみ。

### 巨大ファイル / 大量コンフリクト
- IntelliJ のエディタ基盤に乗るため VSCode ほどの致命的な遅延報告は目立たない (推測)。`Apply All Non-Conflicting Changes` と `Resolve simple conflicts` で大量衝突を一気に削減できる設計。

### 良い点
- **左=自分/中央=結果/右=相手** の物理レイアウトが直感的で、結果が常に中央という一貫性。
- **キーボードショートカット完備** (Accept Left/Right に Alt+Shift+←/→、Mac は Ctrl+Shift+←/→ <https://www.plugin-dev.com/intellij-use/version-control/diff-tool-keyboard/>)。
- **非衝突変更の一括適用 / 単純衝突の自動解決**で手数を激減。

### 悪い点
- **Yours/Theirs と Left/Right の用語不統一**。
- 多段ダイアログ (ファイル一覧→マージ画面) で文脈遷移がやや重い。

### kagi に取り込むべき点
- **「中央 = 結果」を不変の中心に置く 3 ペイン物理メタファ**。
- **Apply All Non-Conflicting / Resolve simple conflicts** に相当する「自動で安全に片付くものは先に片付ける」機能 (safety-first と相性良)。
- **accept/ignore のフルキーボード操作**。

### 取り込まない方がよい点
- **同一概念に複数ラベル (Yours/Theirs vs Left/Right)** を混在させる設計。kagi では用語を 1 セットに統一する。

---

## 3. Meld

公式概念整理: GNOME GitLab Issue #937 <https://gitlab.gnome.org/GNOME/meld/-/work_items/937> / 解説 <https://lukas.zapletalovi.com/posts/2012/three-way-git-merging-with-meld/>

### 提示単位
- **change (chunk) 単位**。各 chunk に矢印アイコンで採用。中央が編集可エディタなので行手編集も可。

### 3-way vs 2-way / 結果ビュー
- **3-way** (3 ペイン)。中央が編集可能な MERGED 出力。
  - **左 (LOCAL)**: read-only (save ボタン無効)
  - **中央 (BASE → 実際は MERGED 編集用)**: 編集可
  - **右 (REMOTE)**: read-only
- (`git mergetool` で `$LOCAL $BASE $REMOTE $MERGED` を渡す典型構成)

### ours/theirs/base/result ラベル
- **LOCAL / BASE / REMOTE** の git 生用語をそのまま列見出しに使用。
- → **これが UX 上の最大の弱点**。Issue #937 の報告者は「どちらが LOCAL でどちらが REMOTE か毎回覚えられず、別の git 可視化ツールを開いて照合している」「自分だけではない (SO や blog に同種の混乱が多数)」と訴え、**列見出しに info bar / tooltip で意味を補足すべき**と提案。

### hunk 選択 / 行編集 / 組み合わせ
- 各 chunk の **矢印アイコン** で左→中央 / 右→中央へコピー。両方押せば結合 (順序は操作順)。
- 中央ペイン直接編集で任意解決。

### ファイル一覧 & 進捗可視化
- Meld 単体は基本 1 ファイルのマージビュー中心。複数ファイルは `git mergetool` が 1 つずつ順に開く方式 (一覧 UI は弱い)。

### Git operation 連携
- `git mergetool` バックエンドとして起動され、中央を保存→終了で当該ファイル解決扱い。continue/abort は git CLI 側。Meld 自身は git の状態機械を持たない。

### undo/redo
- エディタベースのため Undo/Redo 可。

### binary / rename-delete / modify-delete
- テキスト diff 前提。binary は扱えず、modify/delete 等は片側欠落で 3 ペインが崩れる (推測: ほぼ手編集対応)。

### 巨大ファイル / 大量コンフリクト
- 軽量だが、多ファイル時の「1 つずつ開く」フローは進捗が見えにくい。

### 良い点
- **シンプル・軽量**で、3 ペイン+矢印という素直なモデル。
- 中央が常に編集可で手編集に強い。

### 悪い点
- **LOCAL/BASE/REMOTE という生 git 用語**をそのまま見せて混乱を量産 (#937)。
- **コンフリクトファイル一覧 / 全体進捗が乏しい** (git mergetool 任せ)。

### kagi に取り込むべき点
- 反面教師として: **ラベルに必ず文脈説明 (info bar / tooltip / 二重表記) を付ける** という #937 の提案そのもの。

### 取り込まない方がよい点
- **生 git 用語 (LOCAL/BASE/REMOTE) の素出し**。
- **全体進捗・ファイル一覧を外部 (git CLI) 任せにする**設計。kagi はアプリ内で一覧と進捗を持つべき。

---

## 4. KDiff3

公式ハンドブック (Merging And The Merge Output Editor Window): <https://kdiff3.sourceforge.net/doc/merging.html> / <https://docs.kde.org/stable5/en/kdiff3/kdiff3/merging.html>

### 提示単位
- **conflict (line group) 単位**。summary 列で 1 行ごとの出所も表示。直接編集で行単位も可。

### 3-way vs 2-way / 結果ビュー
- **3-way + 専用 Output エディタ**。上に 3 入力ペイン、下に編集可能な **Merge Output Editor Window**。
  - **A**: Base
  - **B**: Local (自ブランチ)
  - **C**: Remote (相手)
  - **Output (下)**: マージ結果 (編集可)

### ours/theirs/base/result ラベル
- **A / B / C** という抽象ラベル (A=Base, B=Local, C=Remote)。ボタンバーも **A / B / C**。
- summary 列: 各行の出所を A/B/C で表示、コンフリクトは赤の **"?"** と **`<Merge Conflict>`**、手編集行は **"m"**。

### hunk 選択 / 行編集 / 組み合わせ
- 各コンフリクトでボタン **A / B / C** を押して採用源を選択。**順番に複数ボタンを押せば結合** (押下順で並ぶ)。
- 全体一括: 「すべて A/B/C」「残りの未解決のみ A/B/C」「white-space 衝突のみ」など粒度別一括選択、"Automatically solve simple conflicts" で自動解決に戻す。
- Output を直接編集 / コピペで任意解決。

### ファイル一覧 & 進捗可視化
- **"Go to prev/next unsolved conflict" ボタン**で未解決間を移動。summary 列で残コンフリクトを赤 "?" で俯瞰。white-space 衝突を別グループ化。
- ディレクトリ比較モードで複数ファイルのマージ一覧を持つ (単一ファイルモードより一覧性高い)。

### Git operation 連携
- `git mergetool` バックエンド。**全コンフリクト解決まで保存不可** ("Saving is disabled until all conflicts are resolved")。保存→終了で当該ファイル完了。continue/abort は git CLI。

### undo/redo
- Output エディタで標準的な編集取り消し可 (推測: フル undo スタックは限定的)。

### binary / rename-delete / modify-delete
- テキスト中心。binary 非対応。modify/delete 系は A/B/C のいずれかが空となり手編集寄り。

### 巨大ファイル / 大量コンフリクト
- 動作は比較的軽量。**「未解決のみ一括選択」「simple conflicts 自動解決」**で大量衝突を効率処理できる設計が強み。

### 良い点
- **未解決コンフリクト数の可視化 + prev/next ナビ + 全解決まで保存禁止**という安全設計。
- **粒度別の一括解決** (全部 A / 残りのみ B / ws のみ など) が非常に強力。
- summary 列で「どの行がどこ由来か」が常時見える。

### 悪い点
- **A/B/C という抽象ラベル**は base/local/remote を暗記する必要があり初学者に不親切。
- 古い Qt UI で取っ付きにくい。

### kagi に取り込むべき点
- **「全コンフリクト解決まで continue/保存を無効化」** = safety-first の核心そのもの。
- **未解決数 + prev/next 未解決ナビ**。
- **粒度別一括解決 (残り未解決のみ ours、空白衝突のみ自動 等)**。
- **各行の出所インジケータ (A/B/C 相当)** を結果ペインに常時表示。

### 取り込まない方がよい点
- **A/B/C のような無意味記号ラベル**。kagi は意味のある語 (base/自分/相手) を使う。

---

## 5. Beyond Compare (Scooter Software, 商用)

公式: Text Merge Overview <https://www.scootersoftware.com/v4help/viewtextmerge.html> / Using Text Merge <https://www.scootersoftware.com/v4help/using_text_merge.html>

### 提示単位
- **section (difference) 単位**。出力ペインは編集可で行単位手編集も可。

### 3-way vs 2-way / 結果ビュー
- **2 または 3 ペイン入力 (read-only) + 編集可能 Output ペイン**。
  - **左**: 一方の版 (Local)
  - **中央 (任意)**: 共通祖先 = 古い版 (Base)
  - **右**: もう一方の版 (Remote)
  - **Output**: 編集可能な結果
- 3-way ⇔ 2-way の切替が可能 (中央 base を付けるか否か)。

### ours/theirs/base/result ラベル
- **Left / Center / Right** + **Output**。コマンドは **Take Left / Take Center / Take Right**。
- 色分け: 左変更=teal、右変更=magenta、両側同一行の衝突=red。Output で手編集した箇所=yellow。

### hunk 選択 / 行編集 / 組み合わせ
- 各 section で **Take Left / Take Center / Take Right** (ツールバー・ポップアップ・Output 横ボタン)。
- **両方採用**: Edit メニューの **Take Left then Right** (順に取り込み)。Take Center then Right 等の派生も議論あり。
- Output 直接編集可 (編集すると yellow 表示)。Take を再実行すれば入力へ戻せる。

### ファイル一覧 & 進捗可視化
- 表示フィルタで「conflicts のみ / 片側変更のみ」に絞り込み可能。red セクションで未解決衝突を識別。
- mergetool 経由では **複数ファイルが 1 つずつ順に開く** (保存→次ファイル)。

### Git operation 連携
- `git mergetool` で **bcomp/bcompare** をバックエンドに。`-savetarget=$MERGED` で出力先指定。**Output を保存→終了**で当該ファイル解決、全ファイル処理後に `git merge --continue` (チュートリアル <https://beyondcompare.gitbook.io/project/git/untitled>)。abort は git CLI。

### undo/redo
- Output エディタで標準的な undo/redo。Take 操作も取り消し可。

### binary / rename-delete / modify-delete
- 別途 binary/hex/folder compare モードは持つが、Text Merge は基本テキスト。modify/delete は片側ペイン欠落で手編集寄り (推測)。

### 巨大ファイル / 大量コンフリクト
- 商用らしく大ファイルに比較的強い (推測)。表示フィルタで conflicts のみ表示し大量差分を捌ける。

### 良い点
- **Left/Center/Right + Take 系コマンド + 編集可 Output** が明快。色分け (teal/magenta/red/yellow) で出所が一目瞭然。
- **3-way⇔2-way 切替**、**表示フィルタ (conflicts only)** で柔軟。
- 手編集箇所を yellow でマークし「機械採用 vs 人手編集」を区別。

### 悪い点
- **商用 / 有償**。複数ファイル進捗の一覧性は mergetool 任せで弱い。
- continue/abort はアプリ外 (git CLI)。

### kagi に取り込むべき点
- **採用源を色分け** (左=色1/右=色2/衝突=赤) + **手編集箇所を別色 (yellow 相当)** でマークし、"人が触った箇所" を可視化。
- **表示フィルタ「conflicts のみ表示」** で大量差分のレビューを軽量化。
- **両方採用に明示コマンド (Take Left then Right 相当)**。

### 取り込まない方がよい点
- **解決の進捗・ファイル一覧・continue/abort を外部 git CLI に丸投げ**する構造。kagi はアプリ内で一気通貫に持つ。

---

## 横断まとめ (kagi 設計への含意)

| 観点 | 採用すべき定石 | 避けるべき失敗 |
|---|---|---|
| ラベル | base/自分(ours)/相手(theirs)/結果 を意味語で、必要なら二重表記・tooltip | LOCAL/REMOTE 生語 (Meld)、A/B/C 記号 (KDiff3)、Incoming/Current の曖昧さ (VSCode) |
| レイアウト | 中央/下が常に編集可能な結果ペイン (JetBrains/KDiff3/BC) | — |
| 両方採用 | 順序明示 (Incoming/Current First, Take Left then Right) | 順序不明な「両方」だけ |
| 進捗 | 残未解決数カウンタ + prev/next ナビ + 全解決まで continue 無効化 (KDiff3 が模範) | 進捗を git CLI 任せ (Meld/BC) |
| 一括解決 | 非衝突一括・単純衝突自動・残りのみ一括 (JetBrains/KDiff3) | — |
| 性能 | 仮想化・遅延描画必須 | 大ファイルで全文展開し数分フリーズ (VSCode) |
| 移行 | 旧来の inline マーカー理解への退路を残す | デフォルト一括強制変更 (VSCode の反発) |
| 出所可視化 | 各行の由来 + 人手編集箇所を色/記号で区別 (BC の yellow, KDiff3 の summary 列) | — |

### 主要 1 次ソース (VSCode 反発)
- 旧ビューへ戻したい: <https://github.com/microsoft/vscode/issues/157610>
- 「新 UI は悪い」: <https://github.com/microsoft/vscode/issues/159516>
- Accept ラベルの曖昧さ: <https://github.com/microsoft/vscode/issues/166105>
- マウス必須問題: <https://github.com/microsoft/vscode/issues/158523>
- 性能: <https://github.com/microsoft/vscode/issues/157166>, <https://github.com/microsoft/vscode/issues/192469>, <https://github.com/microsoft/vscode/issues/206013>
- `git.mergeEditor:false` が効かない: <https://github.com/microsoft/vscode/issues/166950>
- Meld ラベル混乱: <https://gitlab.gnome.org/GNOME/meld/-/work_items/937>
