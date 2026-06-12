# ADR-0008: Terminal Integration Strategy

- Status: Accepted
- Date: 2026-06-12

## Context(調査結果 2026-06-12)

Bottom Panel に埋め込み terminal が必要(requirements-shell.md §1)。4方式を比較した。

| 方式 | crates.io | License | gpui 0.2.2 互換 | 提供範囲 | 工数 |
|------|-----------|---------|----------------|----------|------|
| 1. Zed crates/terminal(+view) | ✗ publish=false | **GPL-3.0** | ✗(in-tree gpui + Zed 内部 crate 群 + alacritty fork) | 全部 | L+ |
| 2. **gpui-terminal 0.1.0**(zortax) | ✓ 2025-12 | **MIT OR Apache-2.0** | **✓(`gpui = "0.2.2"` ちょうど)** | ANSI/256/truecolor・装飾・キー入力・OSC52・resize/exit callback。**selection と scrollback ナビが未実装**。PTY は外付け(generic Read/Write) | S(欠落補完で M) |
| 3. alacritty_terminal 0.26 直 | ✓ | Apache-2.0 | UI 非依存 | エンジン全部(grid/term/selection/scrollback/**自前 tty+event_loop** — portable-pty 不要)。**gpui レンダラ・入力変換・クリップボードは自作** | M〜L(実績: Zed, cosmic-term) |
| 4. PTY なし(コマンド出力パネル) | - | - | - | 対話なし(prompt・TUI・色が死ぬ) | S |

## Decision

1. **MVP(T-BP-007)は方式2: `gpui-terminal 0.1.0` + `portable-pty 0.9` を採用**
   - 理由: gpui 0.2.2 にピッタリ・ライセンス適合・工数 S。selection / scrollback の欠落は
     MVP として許容(「repo root で git を叩ける」が主目的。本格利用は外部 terminal がある)
   - 既知リスク: 単一作者・6 commits の若い crate → MIT/Apache なので**停滞時は fork/vendor 前提**で採用
2. **v0.2 で再評価**: selection / scrollback が必要になった時点で、
   (a) gpui-terminal への fork/contribute、(b) alacritty_terminal 0.26 直結(方式3)への移行を比較する。
   方式3の参照実装は cosmic-term(Apache)と gpui-terminal 自体(MIT/Apache)。**Zed の terminal は GPL のため
   コード転写禁止**(設計の参照のみ)
3. **Operation Log タブ(方式4相当)は terminal と独立に先行実装**(T-BP-004/005)。
   terminal 統合が遅延しても Bottom Panel の価値(操作ログ・失敗表示)は先に出す
4. shell はユーザーのデフォルト(`$SHELL`、無ければ `/bin/zsh`)、cwd = repo root。
   session はパネルを閉じても保持し、明示 kill(またはアプリ終了)まで生存

## Consequences

- 依存追加: gpui-terminal / portable-pty(どちらも permissive)
- terminal 内の git 操作の反映は T029 の .git watcher が既に担う(追加実装ほぼ不要)
- 0.1.0 の API 変化リスクはバージョン固定 + fork 前提で吸収
