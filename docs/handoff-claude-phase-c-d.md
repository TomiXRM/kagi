# Claude Handoff: Kagi Architecture Phase C-D (2026-06-21)

> **引き継ぎ指示書** — このドキュメントは次のClaudeセッション向けの作業引継ぎです。
> 作業ディレクトリ: `/Users/tomixrm/Dev/sandbox/git-client`

## 現在のブランチ状態

```
main (ddf33ee) — 全マージ済みPR: #52-#64
 └─ rearch/crate-split (未push) — #9 Crate分離の作業中worktree
    path: .claude/worktrees/rearch/crate-split
    状態: crates/kagi-git/Cargo.toml + src/ のコピーまで完了、ビルド未確認
```

## 完了済み（mainにマージ済み）

| PR | 内容 | ファイル | 行数変化 |
|---|---|---|---|
| #55 | Phase 0-3: 安全性+パフォーマンス+RepoSession | 全体 | -416行（デッドコード）+ 安全性fix |
| #56 | tree-sitterハイライト非同期化 | diff_view.rs, mod.rs | UIストール解消 |
| #57 | ToastStack分離 | toast_stack.rs(139行) | Rc<RefCell>+3テスト |
| #58 | RepoWorker（worker thread） | worker.rs(170行) | 3テスト |
| #61 | OpLogPanel分離 | oplog_panel.rs(116行) | Rc<RefCell>+3テスト |
| #63 | mod.rs分割(blocking_ops) + render.rs分割(render_helpers) | blocking_ops.rs(886), render_helpers.rs(2922) | mod.rs 6406→5781, render.rs 6272→3159 |
| #64 | modals.rs分割(modal_renderers) | modal_renderers.rs(3395) | modals.rs 3776→402 |

テスト: **777 passed, 0 failed**。CI gate全緑。

## 未完了タスク（10項目中3項目）

### #9 Crate分離（kagi-git crate化）— 作業中

**現状:** `rearch/crate-split` worktreeで以下まで完了:
- `crates/kagi-git/Cargo.toml` 作成済み
- `src/git/*` を `crates/kagi-git/src/` にコピー済み
- `crates/kagi-git/src/lib.rs` 作成済み（pub mod宣言 + re-export）

**残作業:**
1. `Cargo.toml`（ルート）のworkspace membersに `crates/kagi-git` を追加
2. `Cargo.toml`（ルート）のdependenciesに `kagi-git = { path = "crates/kagi-git" }` を追加
3. `src/main.rs` から `mod git;` を削除
4. `src/lib.rs` がある場合はそこからも削除（確認要: `grep 'mod git' src/main.rs src/lib.rs`）
5. `src/` 内の全 `kagi::git::` → `kagi_git::` に置換（325箇所）
6. `src/` 内の全 `crate::git::` → `kagi_git::` に置換
7. `crates/kagi-git/src/` 内の `crate::` 参照を `crate::` のまま（kagi-git crate内なのでOK）
8. **重要:** `crates/kagi-git/src/` 内のコードが `kagi_domain::` を参照しているが、`kagi::git::` 経由でre-exportされていた型は `kagi_domain::` 直接参照に変更が必要。`src/git/mod.rs` の `pub use kagi_domain::` 行を確認。
9. `headless.rs` の `kagi::git::` 参照も `kagi_git::` に変更
10. `src/git/` ディレクトリを削除（移動完了後）
11. `cargo build --workspace` → エラー修正 → `cargo test --workspace`
12. `cargo fmt --all` + CI gate確認（`grep -rnE 'git2::|Repository::open' src/ui/` が0であること）
13. コミット + push + PR作成

**注意点:**
- `src/git/mod.rs` に `pub use kagi_domain::*` のre-exportが多数ある。これらは `crates/kagi-git/src/lib.rs` に移動済み
- `src/git/` 内のコードは `super::` や `crate::` で互相参照している。crate化後も `crate::` はkagi-git crate内を指すのでOK
- `crates/kagi-git/src/mod.rs` は不要（lib.rsがエントリポイント）。`mod.rs` の内容はlib.rsに統合済みだが、重複確認要

### #1-5 Entity<T>化（CommitPanel/ConflictEditor/Sidebar/ToastStack/OpLogPanel）

**現状:** 全て独立struct/moduleとして分離済みだが、`Entity<T>`化は未実施。

**Entity化が必要な理由:** `cx.notify()` が329箇所あり、毎回KagiApp全体が再描画される。子Entity化すればnotifyスコープが局所化される。

**为什么難しい:** `push_toast`（38箇所）と `record_op`（122箇所）がcxを要求する。これら全てにcxをthreadingする必要がある（計160箇所）。

**アプローチ案:**
1. `push_toast` と `record_op` のシグネチャに `cx: &mut Context<Self>` を追加
2. 全160箇所に `, cx` を追加（Python正規表現で機械的置換。ただし**複数行呼び出しの閉じ括弧前**に正確に挿入する必要がある — 前回の失敗教训: 行頭に `, cx)` が挿入された）
3. ToastStack/OpLogPanel を `Rc<RefCell<T>>` → `Entity<T>` に変更
4. `Render` implを追加（空のdivでOK、実際の描画はKagiAppのrenderがread(cx)で読む）
5. KagiAppの初期化で `cx.new(|_| ToastStack::new())` でEntity生成（lazy initパターン）

**複数行push_toastの安全な置換方法:**
```python
# 各 .push_toast( 呼び出しを見つけ、対応する閉じ ) を追跡し、
# その直前に , cx を挿入する。ポイント: call_content の最後の1文字
# （つまり ））を , cx) に置換する。
call = src[idx:pos+1]  # .push_toast( ... ) 全体
if ', cx)' not in call[-10:]:
    new_call = call[:-1] + ', cx)'  # 最後の)を, cx)に
```

### #10 headless退役

**現状:** `src/headless.rs` 1713行、48個のKAGI_* env-var hooks、26個のraw `git2::Repository::open`。

**目標:** 30個の冗長hooks削除（tests/ の統合テストと重複）。10個のUI-state hooksは残す。

**アプローチ:**
1. 各KAGI_* hookを `tests/` の対応する統合テストと照合
2. 重複するhooksを削除
3. `run_repo_flow`（1456行のgod-function）を操作単位に分解
4. `git2::Repository::open` を `kagi_git::Backend::open` に置換
5. `[kagi] ...` ログ契約行は**絶対に変更しない**（テストがgrepする）

## 安全ルール（違反禁止）

1. **`git2::` を `src/ui/` に書かない**（CI gate: `grep -rnE 'git2::|Repository::open' src/ui/` = 0）
2. **`reset --hard`, `push --force`, `git clean`, `unsafe` をコードに書かない**
3. **`[kagi] ...` ログ契約行の文言を変更しない**（headless testがgrepする）
4. **`cargo fmt --all` をpush前に必ず実行**
5. **`cargo test --workspace` が全てpassすること**
6. **mainブランチを消さない、mainのコミットを巻き戻さない**

## PR運用手順

```bash
# worktree作成
git fetch origin main
git worktree add .claude/worktrees/rearch/<name> -b rearch/<name> origin/main

# 作業 → cargo fmt → cargo test → commit
cd .claude/worktrees/rearch/<name>
cargo fmt --all
cargo test --workspace
git add -A && git -c commit.gpgsign=false commit -m "..."

# push + PR
git push -u origin rearch/<name>
gh pr create --base main --head rearch/<name> --title "..." --body "..."

# CI確認 → merge
sleep 120 && gh pr checks <N>
gh pr merge <N> --squash --delete-branch

# クリーンアップ
git worktree remove .claude/worktrees/rearch/<name> --force
git branch -D rearch/<name>
```

## レビュードキュメント（参照用）

- `docs/codebase-review.md` — 全体レビュー
- `docs/refactor-plan.md` — Phase 0-5計画
- `docs/git-safety-checklist.md` — 操作別安全契約
- `docs/performance-review.md` — パフォーマンス戦略
- `docs/architecture-cleanup-roadmap.md` — Phase A-E計画
- `docs/adr/0104`〜`0114` — ADR

## このセッションの成果サマリー

- **12個のPR** がmainにマージ済み（#52-#64）
- **3つのgod-fileを分割**: mod.rs(-625行), render.rs(-3113行), modals.rs(-3374行)
- **5つのコンポーネントを分離**: ToastStack, OpLogPanel, CommitPanelState, ConflictEditor, Sidebar
- **安全性をバックエンド保証化**: Backend::run + ADR-0104-0111
- **RepoSession + RepoWorker**: タブ単位のBackend所有 + worker thread
- **テスト**: 771→777（6つのユニットテスト追加）
