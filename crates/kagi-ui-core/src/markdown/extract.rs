//! Deciding which Markdown nodes are "an image and nothing else", and pulling
//! the image out of them.
//!
//! Split from `markdown/mod.rs` on the boundary the module doc describes: this
//! file answers *what is an image block*, `resolve.rs` answers *where does its
//! path point*, and `mod.rs` owns the plugin and the rendering.

use gpui_component::text::markdown_ast;

#[derive(Debug, PartialEq)]
pub(super) struct ParsedImage {
    pub url: String,
    pub alt: String,
    pub title: Option<String>,
    pub link: Option<String>,
    /// `width` / `height` from an HTML `<img>`, in CSS pixels. Markdown's own
    /// `![]()` syntax has nowhere to put them, so they are `None` there.
    pub width: Option<f32>,
    pub height: Option<f32>,
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
                    width: None,
                    height: None,
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
                        width: None,
                        height: None,
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
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
                width: html_length(attrs, "width"),
                height: html_length(attrs, "height"),
            }),
            // An `<img>` with no `src` is not something we can draw; leave the
            // whole block to the HTML renderer rather than dropping one image.
            None => return Vec::new(),
        }
        at = end;
    }
    out
}

/// An HTML length attribute in CSS pixels.
///
/// `width="120"` and `width="120px"` are the forms a README uses. A percentage
/// is deliberately ignored: it means "of the container", and the image is
/// already capped to the container width, so honouring it would need layout
/// this extractor does not have.
fn html_length(attrs: &str, wanted: &str) -> Option<f32> {
    let raw = html_attribute(attrs, wanted)?;
    let value = raw.trim().strip_suffix("px").unwrap_or(raw.trim());
    let parsed = value.parse::<f32>().ok()?;
    (parsed.is_finite() && parsed > 0.0).then_some(parsed)
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

    /// `<img width="120">` is how a README sizes an icon, and ignoring it
    /// meant `ObjectFit::Contain` scaled that icon up to fill the box instead
    /// (user request).
    #[test]
    fn reads_width_and_height_attributes() {
        let one = |html: &str| html_images(html).into_iter().next().unwrap();
        let img = one(r#"<img src="a.png" width="120" height="60" />"#);
        assert_eq!((img.width, img.height), (Some(120.0), Some(60.0)));
        // The `px` suffix is accepted; a percentage is not, because it means
        // "of the container" and the image is already capped to that.
        assert_eq!(one(r#"<img src="a.png" width="80px" />"#).width, Some(80.0));
        assert_eq!(one(r#"<img src="a.png" width="50%" />"#).width, None);
        // Nonsense and non-positive values fall back to the natural size.
        assert_eq!(one(r#"<img src="a.png" width="wide" />"#).width, None);
        assert_eq!(one(r#"<img src="a.png" width="0" />"#).width, None);
        assert_eq!(one(r#"<img src="a.png" width="-5" />"#).width, None);
        assert_eq!(one(r#"<img src="a.png" />"#).width, None);
    }

    #[test]
    fn parses_standalone_html_image_attributes() {
        assert_eq!(
            html_images(r#"<img src="docs/images/hero.png" width="900" alt="Kagi UI" />"#),
            vec![ParsedImage {
                url: "docs/images/hero.png".to_string(),
                alt: "Kagi UI".to_string(),
                title: None,
                link: None,
                width: Some(900.0),
                height: None,
            }]
        );
        // A centring wrapper is the common README shape and IS claimed — it
        // used to be rejected, which is why those screenshots never appeared.
        assert_eq!(
            html_images("<div align=\"center\"> <img src='nested.png'> </div>"),
            vec![ParsedImage {
                url: "nested.png".to_string(),
                alt: String::new(),
                title: None,
                link: None,
                width: None,
                height: None,
            }]
        );
        // Prose of its own means this is text that contains an image, so the
        // full HTML renderer keeps it.
        assert!(html_images("<p>See <img src='a.png'> here</p>").is_empty());
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
