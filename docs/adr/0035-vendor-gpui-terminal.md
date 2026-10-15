# ADR-0035: Kagi-owned gpui-terminal

- Status: Accepted / Date: 2026-06-12
- 関連: ADR-0008(terminal 方式)/ ADR-0031(外部コード流用ポリシー)

## Context

gpui-terminal 0.1.0(crates.io 唯一の公開版)はマウス選択が **TODO スタブ**で、
クリップボードコピー(cmd-c)は選択前提のため実装不能。ユーザー要望(選択・copy)を
満たすには fork が必要。cmd-v paste は app 側で実装済み(SharedWriter 経由)。

## Decision

- **Kagi-owned terminal**: `vendor/gpui-terminal/` is maintained in this
  repository as Kagi's terminal frontend, selection and PTY integration boundary.
  Keep the package name and path dependency; no submodule or directory rename
  is needed to establish ownership.
- **Origin**: this package derives from Leonard Seibold's
  [`gpui-terminal` 0.1.0](https://github.com/zortax/gpui-terminal), originally
  obtained from crates.io. Track that origin when reviewing upstream changes;
  ownership does not erase the original authorship.
- **License**: retain the upstream **MIT OR Apache-2.0** terms, both
  `LICENSE-MIT` and `LICENSE-APACHE`, attribution and existing `// kagi:`
  provenance markers. Do not replace upstream notices with Kagi-only copyright.
- **Boundary**: Kagi maintains frontend rendering, selection/copy, scrollback
  navigation and PTY integration. VTE parsing and emulator state continue to
  use official `alacritty_terminal`; there is no emulator rewrite.
  Application-specific confirmation, session ownership and presentation remain
  in Kagi's application/UI code.
- **Superseded policy**: the original temporary-fork restriction and automatic
  deletion when upstream adds selection/copy no longer apply. Evaluate useful
  upstream changes explicitly rather than assuming upstream feature parity
  replaces Kagi's integration.
- **No general fork exception**: this explicitly owned terminal is not a
  precedent for copying or forking other dependencies. Do not transcribe or
  replace it wholesale with Zed's GPL terminal.

## Consequences

- Kagi is responsible for maintenance of the in-tree terminal frontend.
- Upstream comparison and adoption remain manual, with origin and licensing
  preserved. The existing source and public behavior are unchanged by this
  ownership decision.
- The historical path-dependency exception remains documented here; dependency
  pin updates and font fallback changes are separate migration slices.

## Amendment — 2026-10-13: mode-owned clipboard paste (#1403)

`TerminalView::paste_text` now owns generic bracketed-paste framing using the
existing emulator mode. It refuses unapproved plain multi-line text without
writing; Kagi owns the confirmation UI. `SharedWriter::write_input` remains
raw key transport only, replacing the former app-side clipboard write path.
This safety correction belongs to the owned terminal's generic transport
boundary; no second mode parser or Kagi-specific modal dependency is introduced.

Review: enabled bracketed payloads remove ESC and ETX, matching alacritty's
paste policy so clipboard content cannot terminate its own paste bracket.
CR/LF and all other UTF-8 bytes are retained; raw keystrokes are unfiltered.

## Amendment — 2026-10-13: plain clipboard control-byte guard (#1437)

`TerminalView::paste_text` refuses unapproved plain clipboard text containing
CR/LF or ASCII control bytes other than Tab without writing any PTY bytes.
The allocation-free `paste_contains_control_characters` predicate is shared
with the app's localized warning selection; CR/LF have the existing newline
warning and clipboard Tab remains ordinary text. Vendor modifications retain
the `kagi:` provenance marker. Approval/session ownership stays in Kagi, while
the vendor's existing mode recheck and bracketed ESC/ETX stripping are unchanged.

## Amendment — 2026-10-15: explicit terminal ownership (#1479-S2)

The Decision above supersedes the original temporary vendor-fork policy.
Selection/copy and scrollback navigation are already implemented; describing
them as upstream prerequisites or unimplemented limitations is obsolete.
This amendment changes ownership/provenance documentation only, not terminal
implementation, public behavior, dependency sources or license notices.

Remove registry-import artifacts (`.cargo_vcs_info.json`, `.cargo-ok` and the
obsolete `Cargo.toml.orig`) from the owned source package: the reserved VCS
metadata prevents `cargo package --list`, and the duplicate manifest no longer
describes this package. Preserve the imported upstream SHA in the README and
retain both license files and attribution unchanged.
