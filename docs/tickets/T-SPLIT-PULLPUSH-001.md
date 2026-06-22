# T-SPLIT-PULLPUSH-001: ops/pull_push.rs を pull/push/fetch に分割

- Status: todo
- Group: god-file split (CLAUDE.md ≤800 LOC/file, ≤80 LOC/fn)
- 仕様の正: ADR-0116 Wave 3

## スコープ

`crates/kagi-git/src/ops/pull_push.rs`（2077行）に pull / push / fetch /
tracking-branch checkout / switch-to-latest の5責務が同居。巨大関数も集中:
`plan_push`（:1189, ~198行）、`execute_pull`（:666, ~197行）、
`plan_switch_to_latest`（:214, ~150行）、`plan_pull`（:487, ~146行）。

対応:
- `ops/pull.rs` / `ops/push.rs` / `ops/fetch.rs`（必要なら共有ヘルパ
  `ops/remote_common.rs`）へ機能境界で分割。
- `plan_/preflight_/execute_` トリプルは**同じファイルに揃えて**移す。
  `#[cfg(test)]` テストも各 op のファイルへ随伴。
- `ops/mod.rs` の `pub use` を更新し、`Backend` からの呼び出しパスを維持
  （公開 API 不変）。
- 余力があれば 80 LOC 超の plan を「状態判定 → プラン構築」の2段に割る
  （任意・振る舞い不変の範囲で）。

クリーンアップ（T-SPLIT 範囲で同時に）:
- `pull_push.rs:2049/2065` の `#[cfg(test)] fn plan_*` テスト名を `test_plan_*`
  へ改名し、トリプル棚卸し grep を汚さないようにする（同種が `stash.rs:860` にも
  あるが本チケットの対象は pull_push のみ）。

## 完了条件

- [ ] pull/push/fetch が別ファイル、各ファイル ≤800 LOC
- [ ] 各 op の plan/preflight/execute が同居、テストも随伴
- [ ] `Backend` 公開メソッド・`[kagi]` 契約行・振る舞い不変
- [ ] テスト関数の `plan_*` 命名衝突を解消（`test_` プレフィックス）
- [ ] `cargo build` + `cargo test --workspace` green、`cargo fmt --check` clean
- [ ] 実装メモを末尾に追記

## 規約

- 移動のみ。git2 ロジックの意味を変えない。T-KLOG-001 が同ファイルの :51 を触るため
  **T-KLOG-001 の後**に着手して競合回避
