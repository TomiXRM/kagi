# ADR-0025: Worktree Creation UX

- Status: Accepted / Date: 2026-06-12

## Decision

- **backend は git2 の Worktree API**(`Repository::worktree(name, path, opts)`)。
  CLI fallback は不要(git2 0.21 で十分)。削除・prune は本スコープ外(later)
- **worktree は必ず新規 branch とセット**で作る(MVP)。
  git の制約(同一 branch を複数 worktree で checkout 不可)を UI から先回りで排除する。
  「既存 branch を紐づける」オプションは later
- **dialog**(Create branch dialog と同型の自前モーダル + gpui-component Input):
  - branch 名入力(create-branch と同じ validation を共用: 文字種 / 既存衝突)
  - path 入力。**default は `../<repo名>-worktrees/<branch名>`**(リポジトリの外)
  - validation: 既存 path / repo 内 path / 親 dir 不在 を blocker に
- plan(safe-create、1段階確認): predicted に「<path> に worktree + branch '<name>'
  (start point <short SHA>)」、recovery に `git worktree remove <path>` を明記
- 実行後: Repository Navigator に **WORKTREES セクションを新設**して表示
  (`repo.worktrees()` 列挙。main worktree は ✓ 表示)。reload で更新
- headless: `KAGI_PLAN_WORKTREE=<branch>:<path>` で plan ログ、
  AUTO_CONFIRM で実行ログ + `[kagi] sidebar: ... worktrees=N`

## Consequences

- kagi 自身の開発で使っている `.claude/worktrees`(agent lane)が Navigator に
  見える可能性 → 表示は `repo.worktrees()` の登録分のみなので問題なし(同じ仕組み)
- worktree 内の repo を kagi で開くケースの動作確認が必要(open_repository は
  workdir 解決済みだが fixture で検証する)
