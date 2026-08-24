//! Deciding which Markdown nodes are "an image and nothing else", and pulling
//! the image out of them.
//!
//! Split from `markdown/mod.rs` on the boundary the module doc describes: this
//! file answers *what is an image block*, `resolve.rs` answers *where does its
//! path point*, and `mod.rs` owns the plugin and the rendering.

use gpui_component::text::markdown_ast;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ParsedImage {
    pub url: String,
    pub alt: String,
    pub title: Option<String>,
    pub link: Option<String>,
}

/// Every image in a block that is *only* images — empty when the node is
/// anything else. Usually one; a centring `<div>` stacking two screenshots is
/// the case that makes it a list.
pub(super) fn standalone_images(node: &markdown_ast::Node) -> Vec<ParsedImage> {
    match node {
        // Straight to the multi-image reader: routing HTML through the
        // single-image helper first silently kept only the first `<img>` of a
        // wrapper that stacks two screenshots.
        markdown_ast::Node::Html(html) => html_images(&html.value),
        _ => standalone_image(node).map(|i| vec![i]).unwrap_or_default(),
    }
}

fn standalone_image(node: &markdown_ast::Node) -> Option<ParsedImage> {
    match node {
        markdown_ast::Node::Paragraph(paragraph) => {
            let [child] = paragraph.children.as_slice() else {
                return None;
            };
            match child {
                markdown_ast::Node::Image(image) => Some(ParsedImage {
                    url: image.url.clone(),
                    alt: image.alt.clone(),
                    title: image.title.clone(),
                    link: None,
                }),
                markdown_ast::Node::Link(link) => {
                    let [markdown_ast::Node::Image(image)] = link.children.as_slice() else {
                        return None;
                    };
                    Some(ParsedImage {
                        url: image.url.clone(),
                        alt: image.alt.clone(),
                        title: image.title.clone(),
                        link: Some(link.url.clone()),
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Parse an HTML block whose entire content is one `<img …>`, which is how a
/// README states an explicit width — usually wrapped for centring:
///
/// ```text
/// <div align="center">
/// <img src="docs/images/diff.png" width="900" alt="…" />
/// </div>
/// ```
///
/// The wrapper is matched too, not just a bare tag. Without that, the most
/// common screenshot shape in a README fell through to the generic HTML
/// renderer, which resolves `src` as a URI and so drew nothing for a
/// repository-relative path (user report). Our own renderer centres the image
/// anyway, so the `<div align="center">` is honoured rather than lost.
///
/// A block with any visible text of its own, or more than one image, is left
/// to the full HTML renderer.
fn html_image(value: &str) -> Option<ParsedImage> {
    html_images(value).into_iter().next()
}

/// Every `<img>` in an HTML block whose visible content is nothing but images.
fn html_images(value: &str) -> Vec<ParsedImage> {
    let tag = value.trim();
    if !tag.starts_with('<') || !tag.ends_with('>') || has_text_outside_tags(tag) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(rel) = tag[at..].find("<img") {
        let start = at + rel;
        let Some(close) = tag[start..].find('>') else {
            break;
        };
        let end = start + close + 1;
        let attrs = &tag[start + 4..end - 1];
        match html_attribute(attrs, "src") {
            Some(url) => out.push(ParsedImage {
                url,
                alt: html_attribute(attrs, "alt").unwrap_or_default(),
                title: html_attribute(attrs, "title"),
                link: None,
            }),
            // An `<img>` with no `src` is not something we can draw; leave the
            // whole block to the HTML renderer rather than dropping one image.
            None => return Vec::new(),
        }
        at = end;
    }
    out
}

/// Whether anything outside `<…>` tags is more than whitespace.
fn has_text_outside_tags(html: &str) -> bool {
    let mut depth = 0usize;
    for ch in html.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            c if depth == 0 && !c.is_whitespace() => return true,
            _ => {}
        }
    }
    false
}

fn html_attribute(attrs: &str, wanted: &str) -> Option<String> {
    let bytes = attrs.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        while at < bytes.len() && (bytes[at].is_ascii_whitespace() || bytes[at] == b'/') {
            at += 1;
        }
        let name_start = at;
        while at < bytes.len()
            && (bytes[at].is_ascii_alphanumeric() || matches!(bytes[at], b'-' | b'_'))
        {
            at += 1;
        }
        if name_start == at {
            return None;
        }
        let name = &attrs[name_start..at];
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at == bytes.len() || bytes[at] != b'=' {
            continue;
        }
        at += 1;
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let (value_start, value_end) = match bytes.get(at).copied() {
            Some(quote @ (b'\'' | b'"')) => {
                at += 1;
                let start = at;
                while at < bytes.len() && bytes[at] != quote {
                    at += 1;
                }
                let end = at;
                at += usize::from(at < bytes.len());
                (start, end)
            }
            Some(_) => {
                let start = at;
                while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
                    at += 1;
                }
                (start, at)
            }
            None => return None,
        };
        if name.eq_ignore_ascii_case(wanted) {
            return Some(attrs[value_start..value_end].to_string());
        }
    }
    None
}

/// Collapse a string to something safe to shape as one line.
///
/// Every run of whitespace containing a newline becomes a single space, and
/// the result is trimmed. GPUI's text system panics rather than wrapping, so
/// anything handed to it has to be single-line by construction.
pub(super) fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn has_uri_scheme(value: &str) -> bool {
    let Some((scheme, _)) = value.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_alphabetic())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_uri_schemes_without_mistaking_relative_paths() {
        assert!(has_uri_scheme("https://example.com/a.png"));
        assert!(has_uri_scheme("data:image/png;base64,AAAA"));
        assert!(!has_uri_scheme("images/a.png"));
        assert!(!has_uri_scheme("./images/a.png"));
    }

    /// The crash this guards: a standalone `<img …/>` block arrives with its
    /// trailing newline, and GPUI panics when asked to shape text containing
    /// one. Anything handed to `MarkdownNode` must survive that.
    #[test]
    fn single_line_strips_every_newline() {
        assert_eq!(
            single_line("<img src=\"a.png\" />\n"),
            "<img src=\"a.png\" />"
        );
        assert_eq!(single_line("alt\nover\r\ntwo lines"), "alt over two lines");
        assert_eq!(single_line("  padded  "), "padded");
        assert_eq!(single_line(""), "");
        assert!(!single_line("a\nb").contains('\n'));
    }

    #[test]
    fn parses_standalone_html_image_attributes() {
        assert_eq!(
            html_image(r#"<img src="docs/images/hero.png" width="900" alt="Kagi UI" />"#),
            Some(ParsedImage {
                url: "docs/images/hero.png".to_string(),
                alt: "Kagi UI".to_string(),
                title: None,
                link: None,
            })
        );
        // A centring wrapper is the common README shape and IS claimed — it
        // used to be rejected, which is why those screenshots never appeared.
        assert_eq!(
            html_image("<div align=\"center\"> <img src='nested.png'> </div>"),
            Some(ParsedImage {
                url: "nested.png".to_string(),
                alt: String::new(),
                title: None,
                link: None,
            })
        );
        // Prose of its own means this is text that contains an image, so the
        // full HTML renderer keeps it.
        assert_eq!(html_image("<p>See <img src='a.png'> here</p>"), None);
        // Two stacked screenshots in one centring div is a normal README
        // shape; both are claimed and render as a column.
        assert_eq!(
            html_images("<div align=\"center\"><img src='a.png'><img src='b.png'></div>")
                .iter()
                .map(|i| i.url.as_str())
                .collect::<Vec<_>>(),
            vec!["a.png", "b.png"]
        );
    }
}
