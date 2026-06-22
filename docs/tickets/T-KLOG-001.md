# T-KLOG-001: klog! 契約の直書き違反を解消し CI ゲートを追加

- Status: todo
- Group: anti-pattern / contract
- 仕様の正: ADR-0116 Wave 1 / ADR-0096

## スコープ

ADR-0096 は「`[kagi] …` 契約行は必ず `klog!` 経由（手書き `eprintln!` 禁止）」を
定める。だが3箇所で生 `eprintln!("[kagi] …")` が残り、単一契約チャネルの不変条件が
破れている。CI にも検出ゲートが無い。

1. `src/headless.rs:194` 付近 `eprintln!("[kagi] bottom-panel: …")`
2. `crates/kagi-git/src/ops/stash.rs:405` 付近 `eprintln!("[kagi] plan: remote stash-drop …")`
3. `crates/kagi-git/src/ops/pull_push.rs:51` 付近 `eprintln!("[kagi] plan: remote pull …")`

各々を `klog!` へ置換する。**出力バイト列は完全一致**を維持（`klog!` は
`eprintln!("[kagi] {}", …)` の薄いマクロ。ヘッドレス harness は生文字列を grep するため
1バイトも変えない）。`kagi-git` から `klog!` が使えない場合は、その crate の
ロギング経路を確認し、文字列が同一になる最小手段を採る（マクロ移設 or 再エクスポート）。

さらに `.github/workflows/ci.yml` に grep ゲートを追加:
`klog.rs` 以外で `eprintln!("[kagi]`（および `println!("[kagi]`）が出たら fail。
既存の git2 ゲート（ADR-0078）と同じ blocking job 形式に揃える。

## 完了条件

- [ ] 3箇所が `klog!` 経由になり、出力文字列は変更前と同一
- [ ] CI に `[kagi]` 直書き禁止ゲート追加（`klog.rs` を除外）
- [ ] `cargo build` + `cargo test --workspace` green、関連ヘッドレステストが従来通りパス
- [ ] `cargo fmt --check` clean
- [ ] 実装メモを末尾に追記

## 規約

- 契約行の format/wording/順序は変えない（CLAUDE.md ログ規約）
- klog! contract 行を `eprintln!` に逆流させない
