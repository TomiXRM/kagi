# ADR-0142: Native Markdown image rendering

- Status: Accepted
- Date: 2026-08-24
- Follows: ADR-0120 (workspace panes), ADR-0121 (UI crate boundaries)

## Context

Kagi renders Markdown natively with `gpui-component::TextView` in the Editor
preview, GitHub PR descriptions/conversations, and release notes. The parser
recognises image syntax, but passes every image destination as `SharedUri`.
Remote HTTP images can use GPUI's image loader; repository-relative paths such
as `![shot](../images/shot.png)` cannot, because they are neither embedded
application assets nor URLs and are not resolved against the Markdown file.

The Editor already has the two pieces needed to resolve them safely: the
repository root and the open file's repo-relative path. Image decoding and
asynchronous repainting already belong to GPUI's image asset loader; Kagi must
not duplicate that cache or perform filesystem reads in a render handler.

## Decision

`kagi-ui-core::markdown::MarkdownImages` is the shared Markdown image policy.
It is installed as a block plugin on every Kagi `TextView::markdown` surface.

- Absolute URI images continue through GPUI's asynchronous resource loader.
- The Editor supplies `repo_root + open_path`; standalone Markdown image blocks
  and standalone HTML `<img>` blocks resolve relative to the document directory.
  A leading `/` means repository root, not filesystem root.
- Lexical `..` traversal outside the repository is rejected.
- Failed images render their alt text instead of an empty gap.
- Image links and titles remain clickable/visible as tooltips.
- Inline images embedded within prose keep `gpui-component`'s native layout.
  Repository-relative resolution initially targets standalone image blocks,
  the conventional shape for screenshots and diagrams. Extending the upstream
  inline image resolver is preferable to replacing its text-flow engine.

Editor-specific Mermaid splitting and rendering stays in
`kagi-ui-editor::markdown`; only the cross-surface image policy moves to core.

## Consequences

- README/docs screenshots render in Editor Markdown preview without a webview.
- PR and release-note Markdown share the same remote-image fallback and link
  behaviour.
- No image bytes, cache, or duplicated UI state is added to `KagiApp` or
  `EditorWorkspaceView`.
- Network images retain their existing privacy characteristic: viewing remote
  Markdown may request its image URLs through GPUI's configured HTTP client.
