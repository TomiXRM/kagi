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

## Amendment — 2026-10-13: safe paste and focus exit (#1403, #1406, #1407)

- Clipboard text goes to the vendored `TerminalView`, whose alacritty emulator
  already tracks shell output's DECSET/DECRST `?2004`, including split output.
  Enabled mode sends exactly one `ESC[200~` / `ESC[201~` pair around the
  UTF-8 payload with ESC and ETX removed, matching alacritty's paste safety
  policy. Embedded ESC could otherwise end bracketed input and execute the
  following lines; ETX could interrupt the receiving shell. CR/LF and all
  other payload bytes are unchanged. Raw keys never use this filtering path.
- With bracketed paste disabled, text containing LF or CR is held in the single
  `active_modal` slot. The localized warning shows the clipboard contents;
  only an explicit Paste button approves sending. Root Return has no default
  and retains the sheet; Escape/Cmd-./Cancel send no text and dismiss it. Approval freezes the owning session and shell
  spawn generation, so a restarted shell or switched/closed tab cannot inherit it.
  Confirmation consults the current mode again.
- Control-Shift-Tab or Command-Shift-J in `Terminal` focuses its existing
  bottom-panel Terminal tab. Left/Right then Return switches panels using the
  established manual-activation selector. The terminal subtree's localized
  accessibility description documents this route. Escape remains shell input.
- Hiding a focused terminal yields immediately to the visible graph root;
  closing from another focus owner leaves that owner alone. The fully clipped
  render path also yields terminal focus, and hiding/reopening retains the PTY,
  process, output and spawn generation.
- Tier A `terminal_paste_bracketed`, `terminal_focus_exit`,
  `terminal_hide_focus` uses a real isolated PTY with a capture program and
  production Cmd-V/key handlers at 940×660 / 1392×883, 100% / 167%, EN / JA.
  Native VoiceOver and physical keyboard behavior remain Tier B evidence.

## Amendment — 2026-10-13: plain-paste control-byte approval (#1437)

- Extend the existing plain-paste approval boundary to ASCII C0 controls
  (`U+0000`–`U+001F`) except Tab, and DEL (`U+007F`), including single-line
  ESC/ETX, backspace and NUL. CR/LF still require the existing newline approval.
  Clipboard Tab remains ordinary text, including indentation or tabular data,
  and never replaces the newline warning. UTF-8 multibyte text, including
  Japanese, is not a control byte.
- The control-character warning explicitly says invisible control characters
  are present and may interrupt programs or execute commands, in EN and JA.
  The displayed warning and AlertDialog accessibility description share the
  same text. The title no longer incorrectly calls a single-line paste
  “multiple lines”. Clipboard contents remain available for inspection.
- The guard runs before locking or writing the PTY. Cancel writes zero bytes;
  explicit Paste sends the original plain payload once. It retains the single
  active-modal owner, session/spawn identity check and current-mode recheck.
  Enabled bracketed paste still removes ESC/ETX, and raw Tab/Shift-Tab still
  bypass clipboard approval.
- Tier A `terminal_paste_control` drives production Cmd-V into the real capture
  PTY after DECRST 2004, covering ETX, ESC sequences, NUL and DEL with
  zero bytes before approval/after Cancel; explicit approval, ordinary Japanese
  text/clipboard Tab sent once without approval, and stale control-paste
  approval after respawn. It runs at 940×660 /
  1392×883, 100% / 167%, EN / JA alongside the `terminal_` neighbors.
  Renderer-fed warning assertions are not native VoiceOver evidence; physical
  keyboard, IME and visible warning inspection remain PM-owned Tier B.
