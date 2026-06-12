# W2-HEADER: Header Toolbar 再グルーピング(worktree レーン)

- Status: in-progress / 依存: ADR-0013
- 原文要件: requirements-gk-parity.md(要件1)

## スコープ(mod.rs の render_header_slot / toolbar 領域のみ)

1. レイアウト: 左 = repo名・current branch・`→ upstream名`・`↑a ↓b` / 中央 = Pull・Push・Branch・Stash・Pop・Undo・**Terminal**(Bottom Panel の Terminal タブを開くボタン)/ 右 = **Refresh**(操作群から分離)+ Search・Settings(disabled 表示のみ、later)
2. **Pull ボタンに `↓N`、Push ボタンに `↑N` を表示**(0 は表示なし)。**behind=0 のとき Pull を disabled**(理由 footer: "nothing to pull (behind=0)")— toolbar_state の pull_on 判定変更
3. **Undo の明示**: 有効時、HEAD commit の summary を `Undo "<summary 16字>"` 形式でラベル or 併記。無効時は従来の理由表示
4. 右端の旧 `branch ↑↓` 表示は左側へ移動(重複させない)
5. ログ更新: `[kagi] toolbar: pull=.. (behind=N) push=.. (ahead=N) ...` 形式に拡張

## 完了条件
- cargo test 全パス + 警告 0 / fixture 3状態(dirty/clean/detached)の toolbar ログ
- behind=0 で pull=off になる(fixture main は behind0 → off / feature/two checkout で on)
- worktree ブランチにコミット。push はしない

## 触ってよい: src/ui/mod.rs(header/toolbar 領域と toolbar_state)/ docs/tickets/W2-HEADER.md
## 触ってはいけない: 他すべて(sidebar / detail panel / commit panel 領域に触れない — 並行レーンあり)
