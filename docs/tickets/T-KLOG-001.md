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

## 実装メモ（2026-06-22 完了）

### 判断: チケットの参照パスは stale だった
チケット本文・ADR-0116 は3箇所を `crates/kagi-git/src/ops/stash.rs:405` /
`crates/kagi-git/src/ops/pull_push.rs:51` と記載していたが、リポジトリを grep した
結果、`crates/kagi-git/` には `[kagi]` 文字列は1つも存在しなかった。実体は **bin 側**の
以下にあった（行番号は一致）:

- `src/headless.rs:194` `eprintln!("[kagi] bottom-panel: open height={} tab={}", …)`
- `src/ui/operations/stash.rs:405` `eprintln!("[kagi] plan: remote stash-drop index={index} blockers=0")`
- `src/ui/operations/pull_push.rs:51` `eprintln!("[kagi] plan: remote pull branch={branch} behind={behind} ahead={ahead}")`

3箇所とも bin クレート内なので `#[macro_export]` の `klog!` がそのまま使える（3ファイル
とも既に他の `klog!` 呼び出しを持っていた）。よって「kagi-git から klog! が使えない」
懸念は発生せず、置換を完遂できた。kagi-git 側の現状維持・理由メモは不要。

### 置換（出力バイト列は完全一致）
`klog!` は `eprintln!("[kagi] {}", format_args!(…))` の薄いマクロ。`[kagi] ` を除いた
本体だけを渡すよう置換。headless.rs は複数行 `eprintln!` を1行 `klog!` に畳んだが、
format 文字列・引数順は同一で出力は不変。

### CI ゲート
`.github/workflows/ci.yml` に blocking job `invariant-klog-single-channel` を追加
（git2 ゲートと同形式）。`grep -rnE '(eprintln|println)!\("\[kagi\]' --include='*.rs'`
で `src/klog.rs` 以外にヒットしたら fail。

注意点: このパターンは **同一行**に `eprintln!("[kagi]` がある場合のみ一致する。
リポジトリには `eprintln!(` と `"[kagi]"` が別行に分かれた multiline 形式が多数残るが、
それらは ADR-0096 の機械変換でマクロ化されておらず本パターンには一致しないため、
ゲートは今回の3箇所修正後 green になる（ローカル grep 確認済み: ヒット 0 件）。
multiline 形式の掃除はスコープ外（将来チケット）。除外パス追加や置換先送りは不要。

### 検証結果
- `cargo build --workspace`: 成功
- `cargo test --workspace`: **791 passed / 0 failed**
- `cargo fmt --check`: clean（差分なし）
- CI ゲート grep をローカル実行: `src/klog.rs` 以外にヒット 0 件（green 見込み）
- `git diff` で契約行の出力文字列が変わっていないことを確認（3箇所とも本体不変）
