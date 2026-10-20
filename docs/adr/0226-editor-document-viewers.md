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

Extend the existing Editor file and buffer lifecycle. `PdfSource` and the retained
`PdfViewer` entity in `kagi-ui-editor` own PDF parsing, rendering and reading state
for both Editor and Graph. Do not create a second workspace, file watcher,
settings store, PDF engine or Git owner.

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
- Keep the parsed document and retained `PdfViewer` in the existing buffer
  lifetime. Lay out all page geometries as one continuous document, but mount
  only visible pages and their immediate neighbors. Ordinary wheel scrolling
  crosses page boundaries; previous/next controls scroll to existing pages.
- Bound retained decoded pixels to 64,000,000 bytes per viewer. Every visible
  page receives a share of that budget; adjacent prefetch uses at most 10%.
  Cache entries follow the visible range plus at most two neighbors, not a fixed
  page cap that could leave visible short pages permanently loading.
  Each raster remains bounded to 4,096 pixels per axis and 16,000,000 pixels.
- Permit at most two raster jobs globally and one pending raster or coalesced
  admission retry per viewer. Completion/retry only notifies; visible rendering
  schedules subsequent work. A cached hidden tab may finish its existing job
  but cannot keep scheduling prefetch.
- Admit results only for the same live viewer, source identity, generation,
  page, zoom, backing scale and current page pixel budget. Superseded results
  cannot replace a newer document, including close/reopen and zoom ABA.
- Reuse the existing watcher and buffer identity. Unchanged content retains
  its source, viewer and reading state; changed content invalidates rasters
  and clamps the page selection to the new document.
- Provide accessible page and zoom controls using existing named Buttons and
  localized loading/error states. Zoom preserves a page-relative document point
  at the viewport center, excluding unscaled spacing. Retain useful previous
  rasters during replacement without collapsing document geometry.
- Give the PDF body its own localized, tabbable focus target and the existing
  theme focus ring. Within `KagiPdfBody`, arrows scroll both axes, Page Up/Down
  and Space/Shift+Space scroll a viewport with overlap, and Home/End or
  Cmd+Up/Down reach the continuous document's boundaries. Clamp and consume
  these actions at the bounds: reading never selects another file or commit.
  Reuse the document's scroll handle without changing source or zoom.
  File Tree and toolbar keep their own key behavior; `EditorReadOnly` remains
  the ancestor context. Release focus only when the PDF body owns it and its
  surface is hidden or reclassified. Reuse the workspace's post-paint focus
  membership guard when closing Editor removes the owner altogether; never
  reclaim focus from another pane or modal (#1680).
- Overlay the pinned shared both-axis `Scrollbar` on the document's existing
  scroll handle. Keep one scroll owner; do not add permanent gutters. The
  shared component supplies overflow detection, theme colors, system visibility
  policy and thumb dragging. Scrolling updates the visible-page status and
  raster demand without changing the source or zoom.
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
- Git Graph commit diff shows two independent read-only PDF viewers: previous
  version on the left, selected version on the right. This is a document
  comparison, not computed differences or highlighted change regions. Editor's
  separate History/snapshot presentation is not expanded by this change.
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

### Historical PDF source ownership

The existing `MainDiffRead` worker reads and parses each historical side. For a
commit comparison, old bytes come from its first parent and new bytes from the
selected commit, never a working-tree substitute. Capture Inspector rename-from
metadata in the request: a path-filtered Git delta may otherwise look added and
lose the old path. Added/deleted files show an explicit absent-side explanation;
read and parse failures remain visible failures rather than absent versions.

PDF path/signature recognition precedes ordinary image sniffing and text
projection, including ASCII PDFs whose changed hunks omit their signature.
Classification lives with the existing immutable diff-cache entry and shares
its invalidation lifetime. Session, visit, request and source guards admit the
prepared result before viewer entities are created. Render only reuses those
entities. Reopening the same accepted immutable commit/path retains reading
positions; mutable comparisons use newly accepted sources.

Recognize PDF suffixes before probing content. For signature-only files, use
`Backend::file_has_prefix` rather than copying both complete sides merely to
classify ordinary text. Worktree prefix reads use fixed stack storage; Git
prefix checks borrow the blob. Once classified as PDF, `Backend::file_bytes`
checks the 10 MiB limit before creating an application-owned byte vector.
Worktree reads also reject growth past the limit during reading. Preserve
commit/HEAD/index/worktree identity. An unstaged comparison uses stage zero or,
for a conflicted path, ours then ancestor; no conflict base means an absent side.
Share that conflict-base lookup with `unstaged_file_diff`. Fall back to HEAD only
when neither a stage-zero entry nor a conflict exists for the path, never on a
read error. Check symlink metadata before opening working-tree files: symlinks
and special files are not PDF sources, and their targets must not substitute for
Git's link contents. These checks do not sandbox filesystem races or bound
libgit2's internal blob materialization or total process memory.

PDF previews expose no text rows, hunk actions, fake line counts or Unified/Split
mode controls. Existing image and text presentations retain their own behavior.
The same read-only keyboard context and PDF body focus apply in both hosts.
Historical PDF bodies use the same root post-paint focus-retirement guard as
Editor PDFs. Closing/replacing the pane or hiding its owner restores root focus
only if that exact retired body still owns focus; another Input or modal wins.


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

The continuous-viewer amendment was exercised in ordinary macOS windows with
generated historical PDFs (3-page previous, 5-page selected and distinct 7-page
working-tree content), independently scrolled sides, added/deleted/renamed
documents and continuous Editor reading. A private clone of the user-supplied
Ritsumei repository verified an actual 32-page document at its first, middle and
last pages, independent old/new commit previews, and navigation to pages 110/111
of a longer document. Both fixtures retained HEAD, staged path/OID/mode and
the inspected PDF bytes. Personal document screenshots are not public evidence.
Native regressions additionally exercise many simultaneously visible short pages,
bounded raster caches, hidden-viewer scheduling and stale source ownership.

## References

- https://github.com/zed-industries/zed/tree/main/crates/image_viewer
- https://docs.rs/hayro/0.8.0/hayro/
- https://docs.rs/hayro-syntax/0.8.0/hayro_syntax/struct.Pdf.html
- https://docs.rs/hayro-syntax/0.8.0/hayro_syntax/enum.LoadPdfError.html
- https://docs.rs/hayro-interpret/0.8.0/hayro_interpret/struct.InterpreterSettings.html
