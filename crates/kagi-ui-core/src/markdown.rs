//! Shared Markdown rendering policy for Kagi's native `TextView` surfaces.
//!
//! `gpui-component` understands Markdown image nodes, but treats every parsed
//! URL as a URI. That works for `https://…` and not for repository-relative
//! paths such as `./docs/screenshot.png`. This block plugin keeps remote image
//! loading on GPUI's asset loader and maps standalone local images to a real
//! filesystem `PathBuf` rooted in the repository.

use std::path::{Component, Path, PathBuf};

use gpui::{
    div, img, prelude::*, px, App, ImageSource, ObjectFit, SharedString, StyledImage, Window,
};
use gpui_component::text::{markdown_ast, MarkdownNode, MarkdownParseContext, MarkdownPlugin};

/// Tallest a Markdown image is drawn. Big enough for a screenshot to stay
/// readable, small enough that one does not push the rest of the document off
/// the screen.
const MAX_IMAGE_H: f32 = 360.0;

/// Filesystem context for resolving image paths in a repository Markdown file.
#[derive(Clone, Debug)]
pub struct MarkdownImageBase {
    repo_root: PathBuf,
    document_dir: PathBuf,
}

impl MarkdownImageBase {
    /// Build a base from the repository root and the Markdown file's repo-relative path.
    pub fn repo_file(repo_root: impl Into<PathBuf>, document: &Path) -> Self {
        Self {
            repo_root: repo_root.into(),
            document_dir: document
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .to_path_buf(),
        }
    }

    fn resolve(&self, url: &str) -> Option<PathBuf> {
        let raw = url.split(['?', '#']).next().unwrap_or(url);
        if raw.is_empty() || has_uri_scheme(raw) {
            return None;
        }

        let relative = if let Some(repo_relative) = raw.strip_prefix('/') {
            PathBuf::from(repo_relative)
        } else {
            self.document_dir.join(raw)
        };
        let relative = normalize_repo_relative(&relative)?;
        Some(self.repo_root.join(relative))
    }
}

/// Plugin applied to every Kagi Markdown `TextView`.
///
/// Remote standalone images are rendered by the same GPUI loader as ordinary
/// Markdown images. Supplying a [`MarkdownImageBase`] additionally enables
/// repository-relative images for the Editor preview.
#[derive(Clone, Debug, Default)]
pub struct MarkdownImages {
    base: Option<MarkdownImageBase>,
}

impl MarkdownImages {
    pub fn remote() -> Self {
        Self::default()
    }

    pub fn for_repo_file(repo_root: impl Into<PathBuf>, document: &Path) -> Self {
        Self {
            base: Some(MarkdownImageBase::repo_file(repo_root, document)),
        }
    }
}

#[derive(Clone, Debug)]
struct ImageBlock {
    source: ImageBlockSource,
    alt: SharedString,
    title: Option<SharedString>,
    link: Option<SharedString>,
}

#[derive(Clone, Debug)]
enum ImageBlockSource {
    Remote(SharedString),
    Local(PathBuf),
}

impl ImageBlock {
    fn image_source(&self) -> ImageSource {
        match &self.source {
            ImageBlockSource::Remote(url) => ImageSource::from(url.clone()),
            ImageBlockSource::Local(path) => ImageSource::from(path.clone()),
        }
    }
}

impl MarkdownPlugin for MarkdownImages {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "kagi-markdown-image"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let image = standalone_image(node)?;
        let source = if has_uri_scheme(&image.url) {
            ImageBlockSource::Remote(image.url.clone().into())
        } else {
            ImageBlockSource::Local(self.base.as_ref()?.resolve(&image.url)?)
        };
        let block = ImageBlock {
            source,
            alt: image.alt.clone().into(),
            title: image.title.map(Into::into),
            link: image.link.map(Into::into),
        };
        Some(
            MarkdownNode::new(self.name(), block)
                .text(image.alt.clone())
                .markdown(cx.node_source(node).unwrap_or_default().to_string()),
        )
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let block = node
            .data::<ImageBlock>()
            .expect("MarkdownImages only renders its own typed nodes");
        let alt = block.alt.clone();
        let tooltip = block.title.clone().unwrap_or_else(|| alt.clone());
        let link = block.link.clone();
        div().w_full().flex().justify_center().child(
            img(block.image_source())
                .object_fit(ObjectFit::Contain)
                // Constrain, never force — the same shape the diff pane's
                // image viewer uses. A fixed height would letterbox a wide
                // screenshot correctly but scale a small image *up* to match
                // it: `ObjectFit::Contain` fills the box in both directions,
                // so a standalone shields.io badge on its own line would
                // render 360px tall. The zero-height-before-load worry that
                // motivated a fixed box is covered by `with_fallback`, which
                // gives the block its alt text until the async cache lands.
                .max_h(px(MAX_IMAGE_H))
                .max_w_full()
                .with_fallback(move || {
                    div()
                        .text_sm()
                        .child(SharedString::from(format!("[{}]", alt)))
                        .into_any_element()
                })
                .when(!tooltip.is_empty(), |image| {
                    image.tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                    })
                })
                .when_some(link, |image, link| {
                    image.cursor_pointer().on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        cx.open_url(&link);
                    })
                }),
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedImage {
    url: String,
    alt: String,
    title: Option<String>,
    link: Option<String>,
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
        markdown_ast::Node::Html(html) => html_image(&html.value),
        _ => None,
    }
}

/// Parse a standalone HTML `<img …>` block, which is common in READMEs that
/// need an explicit width. The full HTML renderer remains responsible for all
/// other HTML; this only extracts the image attributes needed by our loader.
fn html_image(value: &str) -> Option<ParsedImage> {
    let tag = value.trim();
    if !tag.starts_with("<img") || !tag.ends_with('>') {
        return None;
    }
    let attrs = &tag[4..tag.len() - 1];
    Some(ParsedImage {
        url: html_attribute(attrs, "src")?,
        alt: html_attribute(attrs, "alt").unwrap_or_default(),
        title: html_attribute(attrs, "title"),
        link: None,
    })
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

fn has_uri_scheme(value: &str) -> bool {
    let Some((scheme, _)) = value.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_alphabetic())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

fn normalize_repo_relative(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => out.push(part),
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_paths_relative_to_the_markdown_file() {
        let base = MarkdownImageBase::repo_file("/repo", Path::new("docs/guide/readme.md"));
        assert_eq!(
            base.resolve("../images/screen.png"),
            Some(PathBuf::from("/repo/docs/images/screen.png"))
        );
        assert_eq!(
            base.resolve("/assets/logo.png#dark"),
            Some(PathBuf::from("/repo/assets/logo.png"))
        );
    }

    #[test]
    fn rejects_repo_escape_and_uri_sources() {
        let base = MarkdownImageBase::repo_file("/repo", Path::new("README.md"));
        assert_eq!(base.resolve("../secret.png"), None);
        assert_eq!(base.resolve("https://example.com/image.png"), None);
        assert_eq!(base.resolve("data:image/png;base64,AAAA"), None);
    }

    #[test]
    fn detects_uri_schemes_without_mistaking_relative_paths() {
        assert!(has_uri_scheme("https://example.com/a.png"));
        assert!(has_uri_scheme("data:image/png;base64,AAAA"));
        assert!(!has_uri_scheme("images/a.png"));
        assert!(!has_uri_scheme("./images/a.png"));
    }

    /// The plugin only claims a paragraph that is *nothing but* an image, so a
    /// row of shields.io badges — consecutive lines, therefore one paragraph
    /// with several children — keeps `gpui-component`'s inline text flow. This
    /// is what stops a README's badge strip from becoming a stack of blocks.
    #[test]
    fn only_a_paragraph_that_is_nothing_but_an_image_becomes_a_block() {
        use markdown_ast::{Image, Link, Node, Paragraph, Text};
        let image = || {
            Node::Image(Image {
                url: "docs/a.png".into(),
                alt: "a".into(),
                title: None,
                position: None,
            })
        };
        let para = |children: Vec<Node>| {
            Node::Paragraph(Paragraph {
                children,
                position: None,
            })
        };

        assert!(standalone_image(&para(vec![image()])).is_some());
        // Linked image, the `[![badge](img)](href)` shape.
        assert!(standalone_image(&para(vec![Node::Link(Link {
            url: "https://example.com".into(),
            title: None,
            children: vec![image()],
            position: None,
        })]))
        .is_some());
        // Two badges on consecutive lines are one paragraph — left inline.
        assert!(standalone_image(&para(vec![image(), image()])).is_none());
        // An image with prose around it is inline text, not a block.
        assert!(standalone_image(&para(vec![
            Node::Text(Text {
                value: "see ".into(),
                position: None,
            }),
            image(),
        ]))
        .is_none());
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
        assert_eq!(html_image("<div><img src='nested.png'></div>"), None);
    }
}
