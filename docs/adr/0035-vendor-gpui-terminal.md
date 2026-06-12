# ADR-0035: gpui-terminal の一時 vendor fork

- Status: Accepted / Date: 2026-06-12
- 関連: ADR-0008(terminal 方式)/ ADR-0031(外部コード流用ポリシー)

## Context

gpui-terminal 0.1.0(crates.io 唯一の公開版)はマウス選択が **TODO スタブ**で、
クリップボードコピー(cmd-c)は選択前提のため実装不能。ユーザー要望(選択・copy)を
満たすには fork が必要。cmd-v paste は app 側で実装済み(SharedWriter 経由)。

## Decision

- **in-tree vendor**: `vendor/gpui-terminal/` にソースを直接コミットし、
  Cargo.toml を path 依存に切替。**git submodule は採らない**:
  - 並列 worktree agent(`git worktree add`)が submodule init なしで即ビルドできることが必須
  - codex の sandbox はネットワーク不可のことがあり、submodule fetch が前提になると lane が壊れる
  - 一時 fork(下記 exit 条件)に対して submodule の運用コストが見合わない
- ライセンス: 上流は **MIT OR Apache-2.0**(原文確認済み)。LICENSE 2 ファイルと
  README を vendor 内に保持し、改変箇所には `// kagi:` コメントで出自を示す
- **改変ポリシー**: 変更は選択・コピー機能(と必要最小限のバグ修正)に限定。
  kagi 固有の見た目・機能は app 側(src/ui/terminal.rs)に置き、vendor は汎用のまま保つ
  (upstream に PR できる形を維持する)
- **exit 条件(一時 fork の解消)**: 上流が選択/コピーを公開したら crates.io 依存へ戻し
  vendor を削除する。可能なら本実装を upstream PR として提案する

## Consequences

- repo にクレート1個分のソースが入る(小規模、許容)
- 上流更新の追従は手動(diff は小さく保たれる前提)
- Cargo.toml の「変更禁止」規約に vendor path 切替の例外履歴が1件付く(本 ADR が根拠)
