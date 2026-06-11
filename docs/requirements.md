# 要件定義 — コミットグラフ中心の安全な Git GUI クライアント

## 1. プロダクトの目的

GitKraken のコピーではなく、**「コミットグラフ中心の安全な Git GUI クライアント」** を作る。

### 最重要価値

1. **常時可視化** — CLI では把握しづらい以下を常に画面に出す:
   - ブランチの分岐構造(commit DAG)
   - HEAD の現在位置(attached / detached)
   - local branch と remote tracking branch の差分(ahead / behind)
   - 未 push / 未 pull の状態
2. **事故の予防** — rebase / merge / cherry-pick / reset / checkout の事故を減らす
3. **事前明示** — すべての Git 操作の実行前に「何が起きるか」を plan として表示する
4. **非破壊** — ユーザーのローカルリポジトリを壊さないことを最優先にする

### ターゲットユーザー

- Git の基本概念は理解しているが、CLI でのブランチ操作・履歴操作に不安があるエンジニア
- 「いま自分のリポジトリがどういう状態か」を一目で知りたい人

## 2. リリース段階別 要件表

### MVP

| ID | 要件 | 備考 |
|----|------|------|
| M-01 | ローカル repo をパス指定で開ける | bare repo は対象外 |
| M-02 | working tree status を表示できる | staged / unstaged / untracked / conflicted |
| M-03 | commit graph を表示できる | lane 割り当て + merge edge 描画 |
| M-04 | local branch / remote branch / tag / HEAD を graph 上に表示できる | ref バッジ表示 |
| M-05 | commit 選択で metadata(author, date, message, parents)と changed files を表示できる | |
| M-06 | file diff を表示できる | unified 表示で十分。syntax highlight は later |
| M-07 | checkout branch ができる(plan 確認付き) | dirty working tree の場合は警告して中断 |
| M-08 | branch 作成ができる | 任意の commit / branch を起点に |
| M-09 | stash push / list / apply ができる | drop / pop は v0.2(apply は非破壊なので先行) |
| M-10 | cherry-pick が dry-run preview 付きでできる | conflict 予測を事前表示 |
| M-11 | すべての Git 操作は実行前に plan を表示する | OperationPlan → 確認 → 実行 → 検証 |
| M-12 | 操作前後の repo 状態をログに残し、失敗時に復旧手順を表示する | operation log |

### v0.2

| ID | 要件 | 備考 |
|----|------|------|
| 2-01 | commit 作成(stage/unstage、commit message 入力) | |
| 2-02 | merge(fast-forward / no-ff、conflict 検出と表示) | conflict 解決 UI は最小限 |
| 2-03 | fetch / pull / push(force push なし) | 認証は ssh-agent / credential helper 依存 |
| 2-04 | stash pop / drop(plan + 確認付き) | |
| 2-05 | branch rename / delete(merged branch のみ delete 可) | unmerged delete は v1.0 |
| 2-06 | 最近開いた repo の記憶(ここで SQLite or 設定ファイル導入) | |
| 2-07 | diff の syntax highlight | |
| 2-08 | graph の検索(message / author / sha) | |

### v1.0

| ID | 要件 | 備考 |
|----|------|------|
| 1-01 | interactive rebase(reorder / squash / drop)を plan preview 付きで | 開始前に自動 backup ref を作成 |
| 1-02 | conflict 解決 UI(ours / theirs / 手動編集) | |
| 1-03 | reset(soft / mixed)を危険操作ガード付きで | hard は要 backup ref + 二重確認 |
| 1-04 | unmerged branch delete / tag delete(reflog 案内付き) | |
| 1-05 | undo 機能(直近操作の復元。reflog ベース) | |
| 1-06 | submodule の表示(操作は later) | |
| 1-07 | 複数 repo のタブ切り替え | |

### later(やる予定が立つまで凍結)

- GitHub / GitLab / Bitbucket / Azure DevOps 連携、PR 表示
- Team View / cloud sync / AI merge
- 複数 repo workspace、認証管理 UI
- force push(永久に出さない可能性もある)
- git clean / reset --hard の GUI 提供(出すとしても v1.0 の危険操作ガード設計後)
- GPG 署名、worktree、LFS、blame、file history

## 3. MVP で「やらないこと」(明示的非要件)

- ホスティングサービス連携・PR 表示・Team View・AI merge・cloud sync
- 複数 repo workspace・認証管理
- force push / reset --hard / git clean
- **destructive operation の自動実行**(いかなる場合も確認なしで履歴・作業ツリーを失う操作をしない)

## 4. 安全ルール(全フェーズ共通の不変条件)

1. ユーザーの既存 repo に対する破壊的操作は禁止
2. テスト用 fixture repo 以外で `reset --hard` / `clean` / `force push` を実行しない
3. Git 操作は必ず dry-run または事前 plan を出す
4. 操作前後で repo 状態(HEAD, branch tips, status)を取得し、差分をログに残す
5. 失敗時に復旧手順(reflog 等)を表示する
6. 開発中の動作検証は生成したサンプル repo(fixture)上でのみ行う

## 5. 非機能要件

| 項目 | 目標 |
|------|------|
| graph 表示 | 10,000 commits の repo で初回表示 2 秒以内、スクロール 60fps(仮想化リスト必須) |
| 応答性 | Git 操作中も UI が固まらない(Git I/O はバックグラウンド実行) |
| プラットフォーム | macOS 先行(GPUI の成熟度都合)。Linux は best-effort |
| クラッシュ安全 | アプリがクラッシュしても repo は一切壊れない(libgit2/CLI のトランザクション単位に依存) |
