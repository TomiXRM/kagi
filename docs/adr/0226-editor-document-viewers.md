# ADR-0226: Editor Markdown opening mode and read-only PDF viewing

- Status: Accepted
- Date: 2026-10-19
- Follows: ADR-0120, ADR-0121, ADR-0132, ADR-0142

## Context

The Editor already owns file reads, text buffers, Markdown preview, image
presentation, tab caching and watcher-driven refresh. Markdown currently opens
as source; PDF files reach the unsupported-binary placeholder. The requested
change makes Markdown's initial view configurable, defaulting to Preview, and
adds PDF viewing without PDF diffing.

Zed's image viewer is a conceptual reference for document-owned zoom and a
specialized read-only presentation. Its GPL implementation is not copied into
Kagi. Inspection of the current Zed source tree did not identify a PDF viewer.

## Decision

Extend `EditorWorkspaceView` and its existing per-buffer state. Do not create a
second workspace, file watcher, settings store, renderer service or Git owner.
Focused Editor sibling modules may hold PDF rendering and controls rather than
growing the oversized `lib.rs`.

### Markdown

`Settings::editor_markdown_default_view` reads the flat string setting
`editor_markdown_default_view`: `preview` or `source`. Missing and invalid values
select Preview. The existing Settings radio-group pattern exposes both choices
in English and Japanese.

The setting initializes a newly opened Markdown buffer, including the initial
automatic file selection. Open buffers keep their manual Source/Preview choice
when switching between cached tabs or changing the setting. A clean preview tab
replaced by a new file retains the Editor's existing replacement semantics; a
subsequent fresh open uses the then-current preference. Non-Markdown files are
unaffected. Existing Markdown rendering and image/network policies are unchanged.

The current Markdown Preview owns document keyboard context even with tree/root
focus. Its visibility predicate is shared by rendering and root dispatch:
repository-level Undo/Redo are excluded, but keyboard Save remains available for
the retained dirty text buffer. Historical panes and Source keep their own
existing context; Preview does not turn the editable buffer into a read-only PDF.

Preview renders the current retained Input, including unsaved edits, rather than
the loaded/saved `content` snapshot used for save-conflict checks. Each preview
frame reads the Input once and shares one Markdown/Mermaid segment list between
diagram preparation and body rendering. Mode/tab switches do not save or replace
the Input and its Undo history; historical panes do not prepare the hidden draft.

### PDF

Use the official `hayro` 0.8 Rust renderer, with its supported embedded base fonts
and CMaps. This avoids a platform-specific PDFium shared-library payload and a
runtime downloader. It does not promise PDFium-equivalent fidelity: unsupported
PDF constructs and fonts remain renderer limitations.

- Recognize PDF before text decoding or the binary NUL heuristic; validate the
  document with the parser. Malformed `.pdf` files are not editable text.
- Preserve the existing 10 MiB input policy. Bound the actual file read, not just
  its preceding metadata check. Reject unsuitable file types.
- Keep the parsed document, current page, zoom and bitmap in the existing buffer
  lifetime. Render only the current page off the UI thread. Validate dimensions
  and cap raster output; avoid encoding a bitmap to PNG merely to decode it again.
- Allow only one executing page raster per Editor owner and coalesce later
  navigation requests. A superseded result must not replace a newer document,
  page or zoom, including close/reopen and A-to-B-to-A transitions.
- Reuse the existing watcher and buffer identity. Unchanged content should retain
  its parsed document and image; changed content invalidates the old raster and
  clamps the page selection to the new document.
- Provide accessible previous/next-page and zoom controls using existing named
  Buttons, with localized loading, boundary and error states. Zoomed content is
  scrollable rather than immediately fitted back to the viewport.
  Same-page zoom preserves the document point at the viewport center, with
  ordinary layout clamping at the new scroll bounds. Retain the same page's
  bitmap while its new raster is pending so a loading placeholder cannot
  collapse scroll geometry. Page navigation clears the bitmap and starts at
  the top; failures release any retained bitmap and show the existing error.
  The request identity remains the only admission rule for completed rasters;
  completion does not restore an older scroll position (#1678).
- Give the PDF body its own localized, tabbable focus target and the existing
  theme focus ring. Within `KagiPdfBody`, arrows scroll both axes, Page Up/Down
  and Space/Shift+Space scroll a viewport with overlap, and Home/End or
  Cmd+Up/Down reach the current page's boundaries. Clamp and consume these
  actions at the bounds: reading never selects another file or changes pages.
  Reuse the page's scroll handle without changing zoom or raster identity.
  File Tree and toolbar keep their own key behavior; `EditorReadOnly` remains
  the ancestor context. Release focus only when the PDF body owns it and its
  surface is hidden or reclassified. Reuse the workspace's post-paint focus
  membership guard when closing Editor removes the owner altogether; never
  reclaim focus from another pane or modal (#1680).
- PDF presentation has no writable text Input, dirty state or save request.
  Keyboard Save/Undo/Redo must not fall through to unrelated Git operations from
  the foreground PDF surface. Explicit Git menu commands remain separate.
  Size-limit placeholders also own `EditorReadOnly`, even when the bounded read
  stops before PDF classification. Protect the accepted read-only surface, not
  merely a successfully constructed PDF document.
  During classification without an existing text Input, `EditorLoading` also
  excludes these repository-level bindings: an extensionless document may be
  a PDF. A retained text Input keeps keyboard Save during watcher refresh.
  A current read discarded because typing made the buffer dirty clears its
  own loading flag; a superseded read cannot clear a newer request's flag.
  This provisional keyboard context does not classify normal files as PDFs.
  Once loaded, the accepted content owns its classification, not the current
  filename suffix. Renaming an active or cached dirty text buffer to `.pdf`
  preserves its text Input and keyboard Save until a later accepted read
  classifies the on-disk file; rename must not strand unsaved edits.
  Reclassification releases the obsolete text Input during Render, when its
  focus can be returned to the workspace root. Only focus owned by that Input
  is transferred; another pane or modal keeps its focus. Pending Input events
  cannot dirty the newly accepted read-only PDF.
- Working-tree PDF viewing is the scope. Do not add PDF diffs or historical PDF
  snapshot rendering to the existing diff/history pipelines.
- Password-required, invalid-file and unsupported-encryption errors are distinct.
  Password entry is not part of this change; documents accepted with an empty
  password may open normally.
  Error, loading and unsupported-diff explanations use the readable secondary
  body role (`text_sub`), not a faint/disabled label role. Keep these essential
  statuses legible without changing the theme palette or disabled controls (#1665).

The renderer's standard-font substitutes do not provide universal Japanese
fallback. Reuse Kagi's existing bundled Japanese-font owner where supported,
rather than duplicating embedded font assets. Actual multilingual output must be
verified; GPUI font registration alone is not evidence of PDF font support.

Hayro 0.8 classifies some names containing `Sans`/`Serif` as standard Latin
fonts before invoking the fallback resolver. That public query does not retain
the original CID/name context; do not globally replace Helvetica to disguise
this limitation. Unembedded Japanese faces are therefore not universally
supported. The supported AdobeJapan1 fixture uses canonical CIDs for 日本語;
its raster is compared per character against independent, font-free Noto
outlines rather than accepting arbitrary dark pixels or text extraction.

Raster density follows the window's backing scale, independently of logical
zoom, and cached pages invalidate when moving between display densities.
Loading, error and page-status text wraps within the viewer. The page status
owns a full-width row so large UI text cannot clip its readonly indication.
Page and zoom actions use the existing compact icon Buttons; full localized
names remain in accessibility labels and hover help. The reset action keeps its
`100%` label. At 940×660 with Japanese UI text at 167%, controls must not consume
the document viewport: with the bottom panel closed, retain at least 200 logical
pixels of visible document height.

## Distribution and limits

Ship `THIRD-PARTY-NOTICES.txt` with the application's license in macOS, Linux,
Windows, Debian and AppImage distributions. Preserve the renderer licenses and
the separate notices for embedded Foxit fonts, Adobe CMaps and color profiles;
source-only attribution is insufficient for binary distributions.

Committed embedded-font fixtures contain only the existing OFL Noto font.
The encrypted fixture contains font-free geometry. Do not redistribute test
PDFs containing subsets of proprietary system fonts.

Input-size and raster-size caps do not sandbox PDF interpretation, bound all
decoded embedded resources or preempt an executing CPU render. Do not claim a
hostile-document sandbox or guaranteed render deadline.

## Verification contract

Native scenarios and ordinary windows must cover initial Markdown Preview,
Source preference persistence, explicit per-tab switching, dirty-buffer
retention, PDF page/zoom behavior, malformed/password-protected input, readonly
keyboard ownership, stale render rejection and watcher refresh. Compare actual
rendered output for a multilingual multipage PDF and a rotated/cropped PDF.
Inspect generated distribution payloads for the required legal materials.

Verification on macOS and Linux arm64 exercised actual PDF raster output,
including independent CoreGraphics inspection of the Japanese fallback and
outline fixtures. Native scenarios cover Markdown defaults and readonly PDF
ownership in EN/JA, including the compact 167% viewport and localized icon-control
names. Ordinary windows cover persisted Source preference, Japanese text,
multipage navigation, zoom/reset, scrolling, wrapped errors and the complete
rotated/cropped page. The Japanese 940×660 / 167% window retains a readable PDF
viewport with the compact controls. An active, isolated Linux window also
verifies localized hover help for zoom and the abbreviated reset action.
The signed macOS app and archive, Linux tar, Debian package, AppDir and companion
ZIP preserve byte-identical license/notice files. Windows packaging and a
completed AppImage are not locally exercised; the local AppDir/ZIP run had no
`appimagetool` and is not an AppImage-runtime claim.

## References

- https://github.com/zed-industries/zed/tree/main/crates/image_viewer
- https://docs.rs/hayro/0.8.0/hayro/
- https://docs.rs/hayro-syntax/0.8.0/hayro_syntax/struct.Pdf.html
- https://docs.rs/hayro-syntax/0.8.0/hayro_syntax/enum.LoadPdfError.html
- https://docs.rs/hayro-interpret/0.8.0/hayro_interpret/struct.InterpreterSettings.html
