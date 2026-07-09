//! Markdown preview for the Editor workspace (ADR-0120 follow-up).
//!
//! Rendering strategy: GPUI has no webview, so the document is rendered
//! natively with gpui-component's `TextView::markdown`. Mermaid code fences
//! can't be rendered in pure Rust — when the `mmdc` CLI (mermaid-cli) is on
//! PATH each ```mermaid block is rendered to a cached PNG (keyed by content
//! hash + theme) and shown via `img()`; otherwise the block falls back to
//! plain code with an install hint. The document is split around mermaid
//! fences and stacked as TextView / image segments in one scroll container.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use gpui::{img, prelude::*, px, rgb, AnyElement, Context, SharedString, Window};
use gpui_component::text::TextView;

use super::editor_workspace::EditorWorkspaceView;
use super::i18n::Msg;
use super::theme::theme;

/// Files the preview action is offered for.
pub fn is_markdown_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("md") | Some("markdown") | Some("mdx")
    )
}

/// A document slice: plain markdown, or the inside of a ```mermaid fence.
#[derive(Debug, PartialEq)]
pub enum MdSegment {
    Text(String),
    Mermaid(String),
}

/// Split a markdown document around top-level ```mermaid fences.
///
/// Line-based fence tracker, not a full CommonMark parser: it only needs to
/// know whether a ``` fence is currently open so a "```mermaid" line inside
/// another code block isn't treated as a diagram.
pub fn split_mermaid(src: &str) -> Vec<MdSegment> {
    fn fence_open(line: &str) -> Option<(char, usize, String)> {
        let t = line.trim_start();
        for ch in ['`', '~'] {
            let n = t.chars().take_while(|c| *c == ch).count();
            if n >= 3 {
                return Some((ch, n, t[n..].trim().to_ascii_lowercase()));
            }
        }
        None
    }

    let mut out = Vec::new();
    let mut text = String::new();
    // Some((fence_char, fence_len)) while inside a non-mermaid fence.
    let mut in_code: Option<(char, usize)> = None;
    // Some((fence_char, fence_len, body)) while inside a mermaid fence.
    let mut in_mermaid: Option<(char, usize, String)> = None;

    for line in src.lines() {
        if let Some((ch, n, ref mut body)) = in_mermaid {
            match fence_open(line) {
                Some((c, m, info)) if c == ch && m >= n && info.is_empty() => {
                    let code = std::mem::take(body);
                    in_mermaid = None;
                    if !text.is_empty() {
                        out.push(MdSegment::Text(std::mem::take(&mut text)));
                    }
                    out.push(MdSegment::Mermaid(code));
                }
                _ => {
                    body.push_str(line);
                    body.push('\n');
                }
            }
            continue;
        }
        if let Some((ch, n)) = in_code {
            if let Some((c, m, info)) = fence_open(line) {
                if c == ch && m >= n && info.is_empty() {
                    in_code = None;
                }
            }
            text.push_str(line);
            text.push('\n');
            continue;
        }
        match fence_open(line) {
            Some((ch, n, info)) if info == "mermaid" => {
                in_mermaid = Some((ch, n, String::new()));
            }
            Some((ch, n, _)) => {
                in_code = Some((ch, n));
                text.push_str(line);
                text.push('\n');
            }
            None => {
                text.push_str(line);
                text.push('\n');
            }
        }
    }
    // An unterminated mermaid fence renders as a diagram-in-progress rather
    // than silently vanishing.
    if let Some((_, _, body)) = in_mermaid {
        if !text.is_empty() {
            out.push(MdSegment::Text(std::mem::take(&mut text)));
        }
        out.push(MdSegment::Mermaid(body));
    }
    if !text.is_empty() {
        out.push(MdSegment::Text(text));
    }
    out
}

/// State of one mermaid block's PNG, keyed by [`mermaid_key`].
#[derive(Clone, Debug)]
pub enum MermaidState {
    Rendering,
    Ready(PathBuf),
    /// `mmdc` missing (`needs_cli == true`) or render failure.
    Failed {
        needs_cli: bool,
        detail: String,
    },
}

/// Cache key: diagram source + theme (dark renders differently).
pub fn mermaid_key(code: &str, dark: bool) -> u64 {
    let mut h = DefaultHasher::new();
    code.hash(&mut h);
    dark.hash(&mut h);
    h.finish()
}

fn mermaid_cache_dir() -> PathBuf {
    std::env::temp_dir().join("kagi-mermaid")
}

/// Render one mermaid block to a cached PNG. Blocking — run on the
/// background executor.
pub fn render_mermaid_png(code: &str, dark: bool) -> Result<PathBuf, MermaidState> {
    let key = mermaid_key(code, dark);
    let dir = mermaid_cache_dir();
    let out = dir.join(format!("{key:016x}.png"));
    if out.exists() {
        return Ok(out);
    }
    let fail = |needs_cli: bool, detail: String| MermaidState::Failed { needs_cli, detail };
    std::fs::create_dir_all(&dir).map_err(|e| fail(false, e.to_string()))?;
    let input = dir.join(format!("{key:016x}.mmd"));
    std::fs::write(&input, code).map_err(|e| fail(false, e.to_string()))?;
    let mut cmd = std::process::Command::new("mmdc");
    cmd.arg("-i").arg(&input).arg("-o").arg(&out).args([
        "-b",
        "transparent",
        "--scale",
        "2",
        "--quiet",
    ]);
    if dark {
        cmd.args(["-t", "dark"]);
    }
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(fail(true, String::new()));
        }
        Err(e) => return Err(fail(false, e.to_string())),
    };
    if !output.status.success() || !out.exists() {
        let detail = String::from_utf8_lossy(&output.stderr)
            .lines()
            .last()
            .unwrap_or("mmdc failed")
            .to_string();
        return Err(fail(false, detail));
    }
    Ok(out)
}

/// The preview body: stacked TextView / diagram segments in one scroll
/// container. Replaces the code editor in the center pane while preview
/// mode is on.
pub fn render_markdown_preview(
    view: &EditorWorkspaceView,
    window: &mut Window,
    cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    use gpui_component::ActiveTheme as _;

    let content = view.content.clone().unwrap_or_default();
    let dark = theme().dark;
    // One text size for the whole preview, tracking kagi's zoom
    // (`rem_size_px` = 16 × zoom). Out of the box the three size sources
    // disagree: paragraphs inherit the window rem (16px×zoom), headings
    // scale off a fixed 14px base, and code blocks use the gpui-component
    // theme's fixed 13px mono size (user-reported mismatch). Pin body,
    // heading base, and code blocks to the same zoom-tracking base.
    let base = px(super::theme::rem_size_px() * 0.875);
    let tv_style = gpui_component::text::TextViewStyle {
        heading_base_font_size: base,
        // Space above headings (fork knob) — headings otherwise sit flush
        // against the previous block (user-reported).
        heading_top_gap: gpui::rems(0.9),
        // A little air around body paragraphs too, split evenly above and
        // below (fork knobs; the bottom half adds onto paragraph_gap).
        paragraph_top_gap: gpui::rems(0.175),
        paragraph_bottom_gap: gpui::rems(0.175),
        code_block: gpui::StyleRefinement::default().text_size(base),
        is_dark: dark,
        highlight_theme: cx.theme().highlight_theme.clone(),
        ..Default::default()
    };
    let mut col = gpui::div()
        .id("ews-md-preview")
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .text_size(base)
        // gpui's default line height is φ (1.618); the inline-code highlight
        // paints the FULL line box, so at φ the `code` background reads as
        // oversized vertical padding (user-reported). 1.5 keeps prose
        // readable while trimming the box.
        .line_height(gpui::relative(1.5));
    for (ix, seg) in split_mermaid(&content).into_iter().enumerate() {
        match seg {
            MdSegment::Text(t) => {
                // TextView's inner wrapper is `size_full()`, which sizes it
                // to the scroll container instead of its content — the
                // overflow never registers and the pane can't scroll.
                // Overriding the height back to `auto` (via its `Styled`
                // refinement) lets the segment take its intrinsic height so
                // the outer `overflow_y_scroll` works.
                col = col.child(
                    TextView::markdown(
                        ("ews-md-seg", ix),
                        SharedString::from(pad_inline_code(&t)),
                        window,
                        cx,
                    )
                    .selectable(true)
                    .style(tv_style.clone())
                    .h(gpui::Length::Auto),
                );
            }
            MdSegment::Mermaid(code) => {
                col = col.child(render_mermaid_block(view, ix, &code, dark));
            }
        }
    }
    col.into_any_element()
}

/// Pad inline code spans with thin spaces (U+2009) inside the backticks.
///
/// The renderer paints inline code as a bare text-run background with zero
/// horizontal padding (the glyph range only), which looks cramped
/// (user-reported). There is no styling hook for it, so the padding is
/// injected into the text itself: CommonMark strips one *regular* leading
/// and trailing space from a code span, but a thin space survives and
/// widens the highlight. Selection/copy picks up the thin spaces — accepted
/// trade-off. One line only; multi-line code spans are left untouched.
fn pad_inline_code(src: &str) -> String {
    const PAD: char = '\u{2009}';
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '`' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i] == '`' {
            i += 1;
        }
        let delim = i - start;
        // Find a closing run of exactly `delim` backticks on the same line.
        let mut j = i;
        let mut close = None;
        while j < chars.len() && chars[j] != '\n' {
            if chars[j] == '`' {
                let s = j;
                while j < chars.len() && chars[j] == '`' {
                    j += 1;
                }
                if j - s == delim {
                    close = Some(s);
                    break;
                }
            } else {
                j += 1;
            }
        }
        if let Some(cs) = close {
            for _ in 0..delim {
                out.push('`');
            }
            out.push(PAD);
            out.extend(&chars[i..cs]);
            out.push(PAD);
            for _ in 0..delim {
                out.push('`');
            }
            i = cs + delim;
        } else {
            out.extend(&chars[start..i]);
        }
    }
    out
}

fn render_mermaid_block(
    view: &EditorWorkspaceView,
    ix: usize,
    code: &str,
    dark: bool,
) -> AnyElement {
    let key = mermaid_key(code, dark);
    match view.mermaid.get(&key) {
        Some(MermaidState::Ready(path)) => gpui::div()
            .id(("ews-mmd-img", ix))
            .w_full()
            .flex()
            .justify_center()
            .child(img(path.clone()).max_w_full())
            .into_any_element(),
        Some(MermaidState::Failed { needs_cli, detail }) => {
            let hint = if *needs_cli {
                SharedString::from(Msg::EditorWorkspaceMermaidNeedsCli.t())
            } else {
                SharedString::from(format!(
                    "{}: {detail}",
                    Msg::EditorWorkspaceMermaidFailed.t()
                ))
            };
            mermaid_code_block(ix, code, Some(hint))
        }
        Some(MermaidState::Rendering) | None => mermaid_code_block(
            ix,
            code,
            Some(SharedString::from(Msg::EditorWorkspaceMermaidRendering.t())),
        ),
    }
}

/// Fallback / in-progress presentation: the diagram source as a code block
/// with a status line underneath.
fn mermaid_code_block(ix: usize, code: &str, note: Option<SharedString>) -> AnyElement {
    let mut block = gpui::div()
        .id(("ews-mmd-code", ix))
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded_md()
        .bg(rgb(theme().bg_base))
        .border_1()
        .border_color(rgb(theme().surface))
        .child(
            gpui::div()
                .font_family(super::MONO_FONT)
                .text_color(rgb(theme().text_main))
                .whitespace_nowrap()
                .overflow_hidden()
                .child(SharedString::from(code.trim_end().to_string())),
        );
    if let Some(note) = note {
        block = block.child(
            gpui::div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(note),
        );
    }
    block.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_extracts_mermaid_fences() {
        let src = "# Title\n\n```mermaid\ngraph TD; A-->B;\n```\n\ntail\n";
        let segs = split_mermaid(src);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0], MdSegment::Text("# Title\n\n".into()));
        assert_eq!(segs[1], MdSegment::Mermaid("graph TD; A-->B;\n".into()));
        assert_eq!(segs[2], MdSegment::Text("\ntail\n".into()));
    }

    #[test]
    fn mermaid_inside_code_fence_is_not_a_diagram() {
        let src = "````\n```mermaid\ngraph TD;\n```\n````\n";
        let segs = split_mermaid(src);
        assert_eq!(segs.len(), 1);
        assert!(matches!(&segs[0], MdSegment::Text(t) if t.contains("```mermaid")));
    }

    #[test]
    fn unterminated_mermaid_fence_still_yields_segment() {
        let src = "text\n```mermaid\ngraph TD;\n";
        let segs = split_mermaid(src);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[1], MdSegment::Mermaid("graph TD;\n".into()));
    }

    #[test]
    fn no_mermaid_is_single_text_segment() {
        let src = "# Just\n\ntext\n";
        assert_eq!(split_mermaid(src).len(), 1);
    }

    #[test]
    fn key_differs_by_theme_and_content() {
        assert_ne!(mermaid_key("a", true), mermaid_key("a", false));
        assert_ne!(mermaid_key("a", true), mermaid_key("b", true));
        assert_eq!(mermaid_key("a", true), mermaid_key("a", true));
    }

    #[test]
    fn pad_inline_code_adds_thin_spaces() {
        assert_eq!(pad_inline_code("a `b` c"), "a `\u{2009}b\u{2009}` c");
        // double-backtick spans use the same-length closing run
        assert_eq!(pad_inline_code("``a`b``"), "``\u{2009}a`b\u{2009}``");
        // unterminated span left alone
        assert_eq!(pad_inline_code("a `b c"), "a `b c");
        // fenced blocks are not passed through this fn in practice, but a
        // backtick run with no same-line closer stays untouched
        assert_eq!(pad_inline_code("```\nlet x;\n```"), "```\nlet x;\n```");
    }

    #[test]
    fn markdown_path_detection() {
        assert!(is_markdown_path(Path::new("README.md")));
        assert!(is_markdown_path(Path::new("a/b/Doc.MARKDOWN")));
        assert!(!is_markdown_path(Path::new("main.rs")));
        assert!(!is_markdown_path(Path::new("md")));
    }
}
