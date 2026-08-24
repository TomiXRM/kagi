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
        // Sanitized here, not just at `.text()`: `alt` reaches the fallback
        // element and `title` the tooltip, and both are shaped as one line too.
        let block = ImageBlock {
            source,
            alt: single_line(&image.alt).into(),
            title: image.title.as_deref().map(|t| single_line(t).into()),
            link: image.link.map(Into::into),
        };
        // Both of these are shaped as a single line by GPUI, which panics on a
        // newline ("text argument should not contain newlines"). A standalone
        // `<img …/>` HTML block reaches us with its trailing newline attached —
        // `html_image` trims before matching, `node_source` does not — so
        // previewing a README that centres its hero image crashed the app.
        Some(
            MarkdownNode::new(self.name(), block)
                .text(single_line(&image.alt))
                .markdown(single_line(cx.node_source(node).unwrap_or_default())),
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

/// Rewrite a Markdown document so no raw HTML block spans multiple lines.
///
/// GPUI's text system panics rather than wrapping when asked to shape text
/// containing a newline, and `gpui-component` feeds a raw HTML block's own text
/// straight through. A README that writes
///
/// ```text
/// <div align="center">
/// <img src="docs/images/hero.png" />
/// </div>
/// ```
///
/// — or any `<details>` disclosure, which is the same shape — therefore
/// **crashed the Markdown preview**. It is not this crate's bug, but it is
/// this crate's crash: every Kagi Markdown surface runs its source through
/// here first.
///
/// Newlines inside an HTML block become spaces. HTML is whitespace-insensitive
/// between tags, so the rendering is unchanged, and replacing a byte with a
/// byte keeps every later node's source offsets valid.
pub fn flatten_html_blocks(source: &str) -> String {
    use markdown::mdast::Node;

    let Ok(root) = markdown::to_mdast(source, &markdown::ParseOptions::gfm()) else {
        return source.to_string();
    };
    let mut out = source.as_bytes().to_vec();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if let Node::Html(html) = &node {
            if let Some(position) = html.position.as_ref() {
                for byte in &mut out[position.start.offset..position.end.offset] {
                    if *byte == b'\n' || *byte == b'\r' {
                        *byte = b' ';
                    }
                }
            }
        }
        if let Some(children) = node.children() {
            stack.extend(children.iter().cloned());
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| source.to_string())
}

/// Collapse a string to something safe to shape as one line.
///
/// Every run of whitespace containing a newline becomes a single space, and
/// the result is trimmed. GPUI's text system panics rather than wrapping, so
/// anything handed to it has to be single-line by construction.
fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
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
        assert_eq!(html_image("<div><img src='nested.png'></div>"), None);
    }
}

/// Markdown-image behaviour against a document shaped like a real README.
///
/// The plugin's own `parse` needs a `MarkdownParseContext` only the renderer
/// can build, so these drive the same AST through the same `standalone_image`
/// / `single_line` / `resolve` functions that `parse` calls. That is enough to
/// catch the class of bug that crashed the preview: a node whose text carries
/// a newline into GPUI's shaper.
#[cfg(test)]
mod readme_shapes {
    use super::*;
    use markdown::{to_mdast, ParseOptions};
    use markdown_ast::Node;

    /// Every construct a README actually uses, including the ones that bit us.
    const SAMPLE: &str = r#"<div align="center">

<img src="assets/icon/icon_256x256.png" width="120" alt="Kagi icon" />

# kagi

[![Release](https://img.shields.io/github/v/release/o/r)](https://github.com/o/r/releases)
[![Stars](https://img.shields.io/github/stars/o/r)](https://github.com/o/r/stargazers)
![Platform](https://img.shields.io/badge/platform-macOS-blue)

<img src="docs/images/hero.png" width="900" alt="A very long alt describing the screenshot in one line" />

</div>

## Screenshots

![Repo-relative screenshot](docs/images/shot.png)

![Root-relative screenshot](/docs/images/shot.png "With a title")

[![Linked screenshot](docs/images/shot.png)](https://example.com)

Some prose with an ![inline image](docs/images/shot.png) inside it.

- ![in a list](docs/images/shot.png)

![remote](https://example.com/a.png)

<img
  src="docs/images/wide.png"
  alt="An img tag split over several lines"
/>
"#;

    fn nodes(md: &str) -> Vec<Node> {
        fn walk(node: &Node, out: &mut Vec<Node>) {
            out.push(node.clone());
            if let Some(children) = node.children() {
                for c in children {
                    walk(c, out);
                }
            }
        }
        let root = to_mdast(md, &ParseOptions::gfm()).expect("parse");
        let mut out = Vec::new();
        walk(&root, &mut out);
        out
    }

    /// The regression. A standalone `<img …/>` block arrives with its trailing
    /// newline; handing that to GPUI panicked with "text argument should not
    /// contain newlines" the moment a README was previewed.
    #[test]
    fn nothing_the_plugin_produces_contains_a_newline() {
        let mut claimed = 0;
        for node in nodes(SAMPLE) {
            let Some(image) = standalone_image(&node) else {
                continue;
            };
            claimed += 1;
            // `parse` also feeds `.markdown(cx.node_source(node))`, which is
            // the raw source slice for the node — trailing newline included.
            // That is where the panic came from, so the test has to slice the
            // document the same way rather than only checking AST fields.
            let source = node
                .position()
                .map(|p| SAMPLE[p.start.offset..p.end.offset].to_string())
                .unwrap_or_default();
            assert!(
                source.contains('\n') || !source.is_empty(),
                "sanity: a node source was empty"
            );
            for field in [
                single_line(&image.alt),
                single_line(&image.url),
                single_line(image.title.as_deref().unwrap_or("")),
                single_line(image.link.as_deref().unwrap_or("")),
                single_line(&source),
            ] {
                assert!(!field.contains('\n'), "newline survived in {field:?}");
                assert!(
                    !field.contains('\r'),
                    "carriage return survived in {field:?}"
                );
            }
        }
        assert!(
            claimed >= 6,
            "sample should exercise the block path: {claimed}"
        );
    }

    /// A badge strip is consecutive lines, so Markdown makes it ONE paragraph
    /// with several children — the plugin must leave it to the inline renderer
    /// or a README's header turns into a column of 360px blocks.
    #[test]
    fn badge_strip_and_inline_images_stay_inline() {
        let claimed: Vec<String> = nodes(SAMPLE)
            .iter()
            .filter_map(standalone_image)
            .map(|i| i.alt)
            .collect();

        for inline_only in ["Release", "Stars", "Platform", "inline image"] {
            assert!(
                !claimed.iter().any(|a| a == inline_only),
                "{inline_only:?} is inline and must not become a block: {claimed:?}"
            );
        }
        for block in [
            "Kagi icon",
            "Repo-relative screenshot",
            "Root-relative screenshot",
            "Linked screenshot",
            "remote",
        ] {
            assert!(
                claimed.iter().any(|a| a == block),
                "{block:?} should be a block: {claimed:?}"
            );
        }
    }

    /// The crash the preview actually hit, on `main` as well as here: GPUI
    /// panics on a newline, and `gpui-component` renders a raw HTML block's own
    /// text. README's centred screenshots and `<details>` disclosures are both
    /// multi-line HTML blocks, so previewing one aborted the process.
    #[test]
    fn no_html_block_survives_with_a_newline() {
        let flattened = flatten_html_blocks(SAMPLE);
        for node in nodes(&flattened) {
            if let Node::Html(html) = &node {
                assert!(
                    !html.value.contains('\n') && !html.value.contains('\r'),
                    "HTML block still spans lines: {:?}",
                    html.value
                );
            }
        }
        // Byte-for-byte the same length, so every other node's source offsets
        // still point where they did.
        assert_eq!(flattened.len(), SAMPLE.len());
        // And the surrounding Markdown is untouched.
        assert!(flattened.contains("## Screenshots"));
        assert!(flattened.contains("![Repo-relative screenshot](docs/images/shot.png)"));
    }

    /// A `<details>` disclosure is the same shape and just as common.
    #[test]
    fn flattens_a_details_block() {
        let md = "<details>\n<summary><b>macOS</b></summary>\n\nbody text\n\n</details>\n";
        let flat = flatten_html_blocks(md);
        for node in nodes(&flat) {
            if let Node::Html(html) = &node {
                assert!(!html.value.contains('\n'), "{:?}", html.value);
            }
        }
        assert!(flat.contains("body text"), "prose must survive: {flat:?}");
    }

    /// The paths a README uses resolve where a reader expects, and nothing
    /// resolves outside the repository.
    #[test]
    fn readme_paths_resolve_inside_the_repository() {
        let base = MarkdownImageBase::repo_file("/repo", Path::new("docs/guide/readme.md"));
        assert_eq!(
            base.resolve("images/shot.png"),
            Some(PathBuf::from("/repo/docs/guide/images/shot.png"))
        );
        assert_eq!(
            base.resolve("/assets/logo.png"),
            Some(PathBuf::from("/repo/assets/logo.png"))
        );
        assert_eq!(base.resolve("../../../etc/passwd"), None);
        assert_eq!(base.resolve("https://example.com/a.png"), None);
    }
}

#[cfg(test)]
mod real_readme {
    use super::*;
    use markdown::mdast::Node;
    use markdown::{to_mdast, ParseOptions};

    /// The actual file that crashed the preview.
    #[test]
    #[ignore]
    fn flattens_the_repository_readme() {
        let src = std::fs::read_to_string(std::env::var("KAGI_MD_FILE").unwrap()).unwrap();
        let before = count_multiline_html(&src);
        let after = count_multiline_html(&flatten_html_blocks(&src));
        println!("multi-line HTML blocks: {before} -> {after}");
        assert!(before > 0, "README should exercise this");
        assert_eq!(after, 0);
    }

    fn count_multiline_html(md: &str) -> usize {
        fn walk(n: &Node, out: &mut usize) {
            if let Node::Html(h) = n {
                if h.value.contains('\n') {
                    *out += 1;
                }
            }
            if let Some(c) = n.children() {
                for x in c {
                    walk(x, out);
                }
            }
        }
        let root = to_mdast(md, &ParseOptions::gfm()).unwrap();
        let mut n = 0;
        walk(&root, &mut n);
        n
    }
}
