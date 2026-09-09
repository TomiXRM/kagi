# #627 P5 — 実行経路と mixed backend

## 実施条件

- macOS arm64
- release binary: `target/release/examples/p5_driver`
- fixture: synthetic、tracked files `1,000`、commits `3`、depth `3`、seed `627`
- backend ごとに `KAGI_LOG_DIR` を別 temporary directory に設定
- `working_tree_status` は hardened CLI `git status` が index stat cache を書き戻すため、両 backend で iteration ごとに pristine copy を作成
- `--iterations 3`。warm-up は未実施。D1 の最終統計には計画どおり 11 iteration が必要

## D1 — `working_tree_status`、1k files

| backend | 実行経路 | wall time (ms) | status 結果 |
|---|---|---|---|
| libgit2 | `Backend::working_tree_status` | `63.642`, `96.644`, `60.691` | clean (`staged/unstaged/untracked/conflicted = 0`) |
| CLI | hardened `run_git status --porcelain=v2 -z` | `28.651`, `107.031`, `30.728` | clean (`porcelain_entries = 0`) |

最初の dual-backend probe で別 executor が実行されたことを確認した。CLI は `run_git` を通るため、#649 hardening override が有効である。

3 sample は外れ値を含み、1k/5k/20k/50k の傾き・11 iteration median をまだ算出していない。したがって `0.5 ms / 1k files` の D1 判定、CLI 採用閾値、D2 syscall 分類はいずれも未判定。
