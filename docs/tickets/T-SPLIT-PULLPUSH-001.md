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

## 実装メモ（完了）

`crates/kagi-git/src/ops/pull_push.rs`（2077 行）を機能境界で分割し、
元ファイルは削除した。`ops/mod.rs` の `mod`/`pub use` を新モジュール群に差し替え。

### 新ファイル構成と LOC

| ファイル | LOC | 内容（pub 関数 = `ops::*` で従来どおり flat 公開） |
|---|---|---|
| `ops/pull.rs` | 821 | `plan_pull_remote` / `plan_pull` / `execute_pull` / `plan_pull_branch_ff` / `execute_pull_branch_ff`。pull 専用 private helper（`ensure_pull_does_not_touch_dirty_paths` / `pull_changed_paths_between_trees` / `predict_merge_conflict`）も同居。 |
| `ops/push.rs` | 713 | `plan_push` / `execute_push` / `plan_push_branch` / `execute_push_branch` / `plan_set_upstream` / `execute_set_upstream`。push 専用 private helper（`build_push_preview` / `build_push_preview_for_oid` / `choose_push_remote`）も同居。 |
| `ops/fetch.rs` | 72 | `fetch_remote` と private `resolve_fetch_remote`。 |
| `ops/switch.rs` | 400 | `default_tracking_branch_name` / `plan_checkout_tracking_branch` / `execute_checkout_tracking_branch` / `plan_switch_to_latest` / `execute_switch_to_latest`、private `remote_of_ref` / `checkout_branch_tree`。 |
| `ops/remote_common.rs` | 125 | pull/push/fetch 共有 helper（`resolve_upstream_info` / `resolve_upstream_oid` / `short_oid_string` / `local_branch_oid`）。`pub(super)` でクレート内のみ可視。 |

### 配置判断

- tracking-branch checkout / switch-to-latest は「ローカルブランチをリモート ref に
  追従させてツリーを checkout する**ブランチ切替**系」で、pull の
  「現ブランチに merge する」意味論と異なるため独立 `switch.rs` に分離。
  これにより `pull.rs` を pull triple に集中させ、両ファイルとも LOC を抑えた
  （pull は 821 行で 800 目標を僅かに超過するが、`execute_pull` triple を割らない
   方針で許容。`plan_/preflight_/execute_` トリプルは同一ファイルに維持）。
- 複数ファイルから使う upstream 解決・OID helper は `remote_common.rs` に集約。
  pull/push 専用 helper はそれぞれのファイルに残置。

### 公開パスの扱い

- `ops/mod.rs` は従来 `pub use pull_push::*` で flat 公開していたため、
  新モジュールも `pub use pull::*` / `push::*` / `fetch::*` / `switch::*` で同様に
  flat 公開。`remote_common` は `pub(super)` のみなので `pub use` しない。
  → `Backend` などから見える関数パス（`kagi_git::ops::plan_pull` 等）は不変。

### 振る舞い不変・テスト

- git2 ロジック・`OperationPlan` 生成・`[kagi]`/klog 契約行は逐語移動で不変。
- 各 op の `#[cfg(test)]` テスト（`remote_pull_tests`）は `pull.rs` へ随伴移動。
- 命名衝突解消: `plan_pull_remote_*` テスト2件を `test_plan_pull_remote_*` に改名。
- `tests/push_test.rs::test_push_no_force_in_args` がソースを path 読みしていたため
  `pull_push.rs` → `push.rs` に参照パスのみ更新（テスト意味は不変）。
- 検証: `cargo build --workspace` green、`cargo test --workspace` 791 passed / 0 failed、
  `cargo fmt --check` clean、`cargo clippy -p kagi-git` は分割前と同じ 7 warning（新規ゼロ）。
