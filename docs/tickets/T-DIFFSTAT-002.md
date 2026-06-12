# T-DIFFSTAT-002: commit diff / staged・unstaged diff から additions/deletions を集計する

- Status: todo
- 依存: T-DIFFSTAT-001
- 関連: requirements-diffstat.md、lane W16-DIFFSTAT

## スコープ

- `commit_diffstat(repo, &CommitId) -> Result<Vec<FileDiffStat>>`(commit vs 親。`commit_changed_files` と同じ delta 集合)
- `staged_diffstat(repo)` / `unstaged_diffstat(repo)`(Commit Panel 用)
- 集計は git2 `Patch::from_diff` + `line_stats()`(per-delta)。`Diff::stats()`(総計のみ)は使わない
- binary delta は `is_binary=true`・counts 0。rename 検出は既存 diff option を踏襲
- 性能: 呼び出し側の MAX_FILES 切り詰めの内側でのみ計算できる API 形状にする(全 delta 強制計算しない)

## 完了条件

- [ ] tempdir fixture での unit test(add/modify/delete/binary/rename 各 1 以上)
- [ ] `cargo test` 全パス、own-code warning 0

## 実装メモ (done)

- Status: done
- `commit_diffstat` / `staged_diffstat` / `unstaged_diffstat` を実装。集計は git2 `Patch::from_diff(&diff, idx)` + `Patch::line_stats()`(per-delta)。`Diff::stats()` は不使用。
- binary 判定: `Patch::from_diff` が None、または **生成後の patch.delta() の binary flag** が立つ場合に `is_binary=true`・counts 0。`diff_tree_to_tree` の default options では生成前 delta の binary flag が未populateのため、patch 生成後に再読みする方式で確実化(fixture テストで確認済み)。
- rename 検出は `DiffFindOptions::renames(true)`(diff.rs と同方針)。staged は HEAD→index(unborn は空tree)、unstaged は index→workdir(untracked include)。
- tests/diffstat_test.rs: add/modify/delete/binary/rename + staged/unstaged/untracked の 8 ケース全 green。
