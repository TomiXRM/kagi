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

## Amendment (#751, 2026-09-22): conversation surfaces show images as links

The surfaces listed above are not all alike. The Editor preview renders a
document from the user's own repository; the GitHub conversation surfaces
render bodies written by anyone on GitHub. Those are exactly the five call
sites of `timeline_row::body_markdown`: the Issue's own body and its comments
(`issues_thread.rs`), the New Issue and Reply composer Preview
(`issues_composer.rs`), the PR description, reviews and comments plus line
comments (`pr_conversation.rs`), and the PR comment composer Preview
(`pr_page.rs`). On all of them, every image URL in a stranger's comment became
an HTTP request the moment the body was drawn — the "existing privacy
characteristic" above, applied to text the user did not write. (The Issues
list and the PR home table draw titles and metadata, not Markdown bodies.)

- The conversation surfaces render images as **links**: `![alt](url)` is
  rewritten to `[alt](url)` (or to the autolink `<url>` when there is no alt
  text) before the source reaches `TextView`, so the address stays visible and
  clickable and no image node is left for the renderer to load. `![alt][ref]`
  keeps the destination from its definition. The evidence is structural: the
  prepared source re-parses with no `Image`/`ImageReference` node, and the
  image plugin is no longer installed on these surfaces.
- The rewrite is on the source, in `kagi_ui_core::markdown::images_as_links`,
  not on the `MarkdownImages` plugin: the plugin only sees standalone image
  *blocks*, while an image inside a sentence is laid out — and fetched — by
  `gpui-component`'s inline flow. `MarkdownImages` is therefore no longer
  installed on those surfaces.
- The Editor preview is unchanged: remote and repository-relative images still
  render there, through the same plugin and the same loader.
- `kagi_ui_editor::markdown::prepare_github_markdown` is the one entry point
  those five call sites render from, so the policy cannot hold on the Issues
  side and not the PR side, or on a thread but not its composer's Preview.

## Amendment 2 (#751, 2026-09-22): conversation bodies draw with `calt` off

Tier B found literal `<!--` and `-->` — in an inline code span and inside a
fenced block — drawn as `<!—` and `—>`. The source was right; the shaping was
not. Both bundled families (Inter for prose, JetBrains Mono for code blocks)
ship contextual alternates, and `calt` ligates those hyphen runs. In a Git
client, code that does not say what the author typed is a defect.

- The same five `body_markdown` call sites draw with
  `kagi_ui_editor::markdown::literal_text_features()` — `calt = 0` — the
  typography half of the same policy `prepare_github_markdown` is the source
  half of.
- **The feature is off for the whole body, prose included.** It is not scoped
  to code, and cannot be: `gpui-component` renders an inline code span as a
  `HighlightStyle` over the paragraph's own text runs, and `gpui::HighlightStyle`
  carries no font-feature field, so no per-run hook reaches one. The hook that
  does is the text style of the element that owns the document; the fenced
  block inherits it because its own refinement sets family and size but never
  features. The accepted cost is that prose stops ligating too: every
  character in the body is drawn as the character in the source.
- `TextViewStyle::code_block` is deliberately left alone. Setting the same
  feature there would be dead weight: the block already inherits it, and two
  places to change is how the Issues side and the PR side drift apart.
- The Editor preview keeps its own typography, as it keeps its own image
  policy above.
