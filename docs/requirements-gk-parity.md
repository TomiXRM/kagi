# 要件定義: GitKraken パリティ UI/UX 改善(第2次シェル改善)

- Date: 2026-06-12
- 発端: ユーザー要件(GitKraken 比較。原文はチケット指示に保存)
- 関連 ADR: 0012〜0019

## 目的

GitKraken の見た目のコピーではなく「作業の流れを迷わせない UI」の導入。
構成: Header Toolbar / Left Sidebar(Repository Navigator)/ Main(Graph)/ Right Panel(Commit Inspector)/ Bottom Panel / Status Bar。

## ギャップ分析(2026-06-12 実装時点)

| 要件領域 | 既に実装済み | 今回やる | later/v0.2 |
|----------|--------------|----------|------------|
| Header | ボタン群+アイコン / plan 必須 / force 禁止 / disabled 理由 / ↑↓表示(右端) | **左側に repo+branch+upstream+↑↓ 集約 / Pull↓N・Push↑N をボタンに紐付け / behind 0 で Pull disabled / Undo の対象 commit をラベル表示 / Refresh を右側へ分離 / Terminal ボタン追加 / Search・Settings 枠(無効)** | branch selector ドロップダウン |
| Sidebar | LOCAL BRANCHES / STASHES / click=jump / dblclick=checkout | **Navigator 化: REMOTE BRANCHES・TAGS セクション追加 / 折りたたみ+件数 / filter 入力 / upstream 表示 / context menu(checkout・create・delete=plan 経由)** | WORKTREES, PR/ISSUES |
| Graph | lane 列リサイズ / author・time 固定幅 / 24px row / 選択ハイライト / HEAD バッジ | **HEAD・merge commit の node 視覚区別 / 選択行の強調強化 / compact mode トグル / label→node の視覚接続** | — |
| Right Panel | metadata / changed files tree+count+highlight / diff(main pane) / binary 表示 | **Inspector 並び替え(Summary→Metadata→Actions→Files)/ copy SHA / authored・committed 両日付 / Path⇄Tree トグル(detail 側)/ 大 diff fold / renamed・deleted 表示改善** | file filter, Revert, 外部サービス連携 |
| Bottom Panel | tabs / resize+clamp / session 保持 / cwd=root / watcher refresh / 失敗時自動オープン / Operation Log(plan・結果・復旧) | **default 高さ ≤ 画面20%** | 高さの永続化(設定ファイル導入時) |
| Status Bar | branch / ↑↓ / dirty / staged / unstaged / 時刻 / toggles | **conflict 数 / stash 数 / upstream 名 / background operation 表示** | — |

注: diff viewer の「commit の diff と staged/unstaged の diff の区別」「lazy/fold」「syntax highlight」は実装済み(T-UI-003/004。highlight は later 指定だが先行済)。

## チケット(第2次。既存番号と衝突しないよう W2- 系)

| ID | 内容 | 担当方式 |
|----|------|----------|
| W2-SIDEBAR | Navigator 化一式(抽出→sections/collapse/counts/filter/upstream/remote/tags/context menu) | worktree agent |
| W2-INSPECTOR | Right Panel Inspector 化一式(抽出→並び替え/Summary/copy SHA/日付/Path-Tree toggle) | worktree agent |
| W2-HEADER | Header 再グルーピング一式(左集約/↓N↑N/Undo ラベル/Refresh 分離/Terminal ボタン) | worktree agent |
| W2-STATUS | Status Bar 拡張(conflict/stash/upstream/bg-op)+ Bottom default 20% | PM(main) |
| W2-GRAPH | node 視覚区別 / 選択強調 / compact mode / label接続 | 第2波 |
| W2-DELETE | plan_delete_branch backend(merged のみ、plan 経由) | 第2波(context menu の依存) |

## 完了条件(原文の完了条件に対応)

- Header だけで Pull/Push 判断ができる(↓N/↑N + disabled)
- Sidebar が Navigator(4セクション+filter+折りたたみ)
- Right Panel が Summary→Metadata→Actions→Files の順
- Bottom default 高さ ≤20%
- Status Bar に repo 状態サマリ一式
- 全 Git 操作が plan 経由(既存保証の維持)
