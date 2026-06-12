# 要件定義: App Shell 拡張(Bottom Panel / Status Bar / Header Toolbar / ahead-behind 常時表示)

- Date: 2026-06-12
- 発端: ユーザー要件(VSCode/Zed 風 Bottom Panel + Git 操作 Header Toolbar)
- 関連 ADR: 0007(Bottom Panel)/ 0008(Terminal)/ 0009(Toolbar 操作ポリシー)/ 0010(ahead-behind)/ 0011(Undo Commit)

## 0. AppShell レイアウト(全体像)

```
┌──────────────────────────────────────────────────┐
│ Header Toolbar(Git操作 + branch/ahead-behind)     │
├────────┬─────────────────────────┬───────────────┤
│Sidebar │ Main(commit graph)      │ Right Panel    │
├────────┴─────────────────────────┴───────────────┤
│ Bottom Panel(出し入れ可・タブ式・高さリサイズ)        │
├──────────────────────────────────────────────────┤
│ Status Bar(repo/branch/件数/↑↓/refresh時刻/開閉)   │
└──────────────────────────────────────────────────┘
```

既存の footer(直近操作1行)は Status Bar に統合する。

## 1. Bottom Panel

目的: Git 操作の実行ログ確認 / terminal で直接 git 実行 / plan・結果の確認 / 将来タブ追加(Problems, Search 等)。
メインの graph / commit panel を邪魔しない。

必須:
- 表示/非表示の切替(Status Bar のアイコン + キーボードショートカット)
- 高さリサイズ(T023 ディバイダ機構の水平版)+ 最後の高さを記憶(セッション内。永続化は later)
- 複数タブ設計: Terminal / Git Output / **Operation Log** / Problems。**MVP は Operation Log + Terminal(または ADR-0008 の結論による代替)**
- タブ切替は Status Bar から
- パネルを閉じても terminal session は破棄しない(明示 kill まで保持)
- **Git 操作結果は必ず Operation Log タブに出す**(T017 の oplog をソースとする)
- **Git 操作失敗時は Bottom Panel を自動で開き失敗理由を表示**

Terminal(詳細は ADR-0008):
- repo root を cwd にユーザーのデフォルト shell($SHELL)で起動
- terminal 内の git 操作後に app 側 state を refresh(T029 の .git watcher が既にこれを満たす)
- 起動失敗は Operation Log にエラー
- MVP は単一 session。複数タブは later

## 2. Status Bar

表示: repo 名 / branch 名 / dirty state / staged 数 / unstaged 数 / ahead↑ / behind↓ / upstream 名 /
最終 refresh 時刻 / background operation 状態。

操作:
- Terminal / Output(Operation Log)/ Problems アイコン → Bottom Panel の該当タブを開閉
- Refresh ボタン → state 再取得
- branch 名クリック → branch switcher(sidebar の branch 一覧へフォーカスで可)
- ahead/behind クリック → pull/push 候補表示(Toolbar の Pull/Push へ誘導)

## 3. Header Toolbar

操作: Pull / Push / Branch Create / Stash / Pop / Undo Commit / (Redo: ADR-0011 で要否判断) / Refresh
+ Current branch / upstream 差分表示。

**大原則(ADR-0009)**: 単なる git command ボタンにしない。全操作は OperationPlan を生成し、
実行前に「何が起きるか」を表示する(既存 plan→confirm→preflight→execute→verify パイプライン)。

| 操作 | disabled 条件 | plan 表示内容 | 備考 |
|------|---------------|---------------|------|
| Pull | upstream 未設定 | behind 数 / dirty 警告 / merge 方式 | MVP は merge pull のみ(rebase pull later)。conflict 時は Operation Log + 誘導 |
| Push | upstream 未設定(→ set-upstream flow 提案)/ ahead 0 | push される commit 一覧 | **force 禁止**。force-with-lease later |
| Branch Create | - | 既存 T014 + 「作成後 checkout するか」選択追加 | HEAD or 選択 commit 起点(既存) |
| Stash | clean | 対象ファイル一覧 / message 入力 / untracked 含むか選択 | 既存 T015 拡張 |
| Pop | stash 0 | conflict 可能性警告 / apply と pop の違い明示 | **pop は Destructive 寄り**(成功時 stash 消滅)→ plan 必須。MVP は latest のみ、設計は一覧選択に拡張可 |
| Undo Commit | push 済み / merge commit / unborn | 取り消される commit 表示 / soft reset 相当の説明 | ADR-0011。**reset --hard 禁止**。復旧情報を Operation Log に残す |
| Refresh | - | (読み取りなので plan 不要) | |

## 4. ahead/behind 常時表示

- 表示例: `main ↑2 ↓0` / `feature/foo ↑1 ↓3` / `no upstream` / `detached HEAD`
- データは既存 `UpstreamInfo`(T005)。Header Toolbar と Status Bar の両方に表示
- **fetch しない限り behind は古い可能性がある** → 最終 refresh 時刻を Status Bar に表示し、
  「ローカル情報ベース」であることを UI に明示(ADR-0010)。fetch ボタンは Pull 系の一部として later
- upstream 未設定 / detached HEAD は明示表示し、detached 時は branch 操作(Pull/Push/Undo)を disabled

## 5. MVP でやらないこと(この要件群の中で)

- rebase pull / force-with-lease / 複数 terminal タブ / Problems・Search タブの中身 /
  Redo Commit(ADR-0011 の結論次第)/ 高さ・タブ状態の永続化 / branch switcher の専用 UI

## 6. チケット分割

docs/tickets/INDEX.md の T-BP-001〜008 / T-HT-001〜010 を参照(ユーザー提案の分割を採用)。
依存の都合で実施順は INDEX に記載。
