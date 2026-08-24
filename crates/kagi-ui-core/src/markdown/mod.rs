//! Shared Markdown rendering policy for Kagi's native `TextView` surfaces.
//!
//! `gpui-component` understands Markdown image nodes, but treats every parsed
//! URL as a URI. That works for `https://…` and not for repository-relative
//! paths such as `./docs/screenshot.png`. This block plugin keeps remote image
//! loading on GPUI's asset loader and maps standalone local images to a real
//! filesystem `PathBuf` rooted in the repository.

use std::path::{Path, PathBuf};

use gpui::{
    div, img, prelude::*, px, App, ImageSource, ObjectFit, SharedString, StyledImage, Window,
};
use gpui_component::text::{markdown_ast, MarkdownNode, MarkdownParseContext, MarkdownPlugin};

/// Tallest a Markdown image is drawn. Big enough for a screenshot to stay
/// readable, small enough that one does not push the rest of the document off
/// the screen.
const MAX_IMAGE_H: f32 = 360.0;

mod extract;
mod resolve;

pub use resolve::MarkdownImageBase;

use extract::{has_uri_scheme, single_line, standalone_images};

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
    /// Usually one. A README that stacks screenshots inside a single centring
    /// `<div>` yields several, and they render as a column.
    images: Vec<BlockImage>,
}

#[derive(Clone, Debug)]
struct BlockImage {
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

impl BlockImage {
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
        let parsed = standalone_images(node);
        if parsed.is_empty() {
            return None;
        }
        let mut images = Vec::with_capacity(parsed.len());
        for image in &parsed {
            let source = if has_uri_scheme(&image.url) {
                ImageBlockSource::Remote(image.url.clone().into())
            } else {
                // One unresolvable image forfeits the whole block rather than
                // rendering a partial one — the HTML renderer at least shows
                // something for the rest.
                ImageBlockSource::Local(self.base.as_ref()?.resolve(&image.url)?)
            };
            // Sanitized here, not just at `.text()`: `alt` reaches the fallback
            // element and `title` the tooltip, both shaped as one line too.
            images.push(BlockImage {
                source,
                alt: single_line(&image.alt).into(),
                title: image.title.as_deref().map(|t| single_line(t).into()),
                link: image.link.clone().map(Into::into),
            });
        }
        let image = &parsed[0];
        let block = ImageBlock { images };
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
        // A column, because the one case with several images is a centring
        // `<div>` stacking screenshots — which is how it reads on GitHub.
        div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .children(block.images.iter().cloned().map(render_block_image))
    }
}

fn render_block_image(image: BlockImage) -> impl IntoElement {
    let alt = image.alt.clone();
    let tooltip = image.title.clone().unwrap_or_else(|| alt.clone());
    let link = image.link.clone();
    img(image.image_source())
        .object_fit(ObjectFit::Contain)
        // Constrain, never force — the same shape the diff pane's image viewer
        // uses. A fixed height would letterbox a wide screenshot correctly but
        // scale a small image *up* to match it: `ObjectFit::Contain` fills the
        // box in both directions, so a standalone shields.io badge on its own
        // line would render 360px tall. The zero-height-before-load worry that
        // motivated a fixed box is covered by `with_fallback`, which gives the
        // block its alt text until the async cache lands.
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
        })
}

/// Rewrite a Markdown document so nothing GPUI shapes as one line contains a
/// newline.
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

    fn has_image(node: &Node) -> bool {
        if matches!(node, Node::Image(_) | Node::ImageReference(_)) {
            return true;
        }
        node.children().is_some_and(|c| c.iter().any(has_image))
    }

    let Ok(root) = markdown::to_mdast(source, &markdown::ParseOptions::gfm()) else {
        return source.to_string();
    };
    let mut out = source.as_bytes().to_vec();
    let mut flatten = |position: Option<&markdown::unist::Position>| {
        if let Some(p) = position {
            for byte in &mut out[p.start.offset..p.end.offset] {
                if *byte == b'\n' || *byte == b'\r' {
                    *byte = b' ';
                }
            }
        }
    };
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match &node {
            // Raw HTML: its own text is rendered verbatim, newlines included.
            Node::Html(html) => flatten(html.position.as_ref()),
            // A paragraph holding an inline image: `inline_flow` slices the
            // paragraph's text around the image and hands a slice containing
            // the soft line break to the shaper. A soft break renders as a
            // space anyway, so joining the lines changes nothing on screen —
            // it is how GitHub lays a badge strip out too.
            Node::Paragraph(p) if has_image(&node) => flatten(p.position.as_ref()),
            _ => {}
        }
        if let Some(children) = node.children() {
            stack.extend(children.iter().cloned());
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| source.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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

        assert!(!standalone_images(&para(vec![image()])).is_empty());
        // Linked image, the `[![badge](img)](href)` shape.
        assert!(!standalone_images(&para(vec![Node::Link(Link {
            url: "https://example.com".into(),
            title: None,
            children: vec![image()],
            position: None,
        })]))
        .is_empty());
        // Two badges on consecutive lines are one paragraph — left inline.
        assert!(standalone_images(&para(vec![image(), image()])).is_empty());
        // An image with prose around it is inline text, not a block.
        assert!(standalone_images(&para(vec![
            Node::Text(Text {
                value: "see ".into(),
                position: None,
            }),
            image(),
        ]))
        .is_empty());
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

<div align="center">
<img src="docs/images/diff.png" width="900" alt="Centred screenshot, no blank lines inside the wrapper" />
</div>

<div align="center">
<img src="docs/images/a.png" width="900" alt="First of two stacked screenshots" />
<img src="docs/images/b.png" width="900" alt="Second of two stacked screenshots" />
</div>
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
            for image in standalone_images(&node) {
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
            .flat_map(|n| standalone_images(n))
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
            // The centred-wrapper shape a README uses for screenshots. Without
            // blank lines inside the wrapper the whole `<div>…</div>` is ONE
            // html node — the case that silently rendered nothing until the
            // wrapper was matched (user report).
            "Centred screenshot, no blank lines inside the wrapper",
            // And a wrapper stacking two of them, which a README also does.
            "First of two stacked screenshots",
            "Second of two stacked screenshots",
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

    /// The construct that actually crashed the preview, found by bisecting the
    /// repository README down to two lines: an inline image and a soft line
    /// break in one paragraph. `inline_flow` slices the paragraph around the
    /// image and hands the shaper a slice containing the newline.
    ///
    /// A soft break renders as a space, so joining the lines is a no-op on
    /// screen — GitHub lays a badge strip out the same way.
    #[test]
    fn flattens_a_paragraph_holding_an_inline_image() {
        let badge_strip = "[![A](https://img.shields.io/badge/a-b-blue)](https://e.com)\n\
                           [![B](https://img.shields.io/badge/c-d-red)](https://e.com)\n";
        for md in [
            badge_strip,
            "![A](https://img.shields.io/badge/a-b-blue)\ntrailing text\n",
        ] {
            let flat = flatten_html_blocks(md);
            assert!(
                !flat.trim_end().contains('\n'),
                "the image paragraph must be one line: {flat:?}"
            );
            assert_eq!(flat.len(), md.len(), "offsets must stay valid");
        }
    }

    /// Prose without an image keeps its soft breaks: the bug needs an inline
    /// image, and rewriting every paragraph would be scope this does not need.
    #[test]
    fn leaves_an_ordinary_paragraph_alone() {
        let md = "hello world\nsecond line here\n";
        assert_eq!(flatten_html_blocks(md), md);
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

#[cfg(test)]
mod readme_coverage {
    use super::*;
    use markdown::mdast::Node;
    use markdown::{to_mdast, ParseOptions};

    /// How many of the repository README's images the plugin claims, after
    /// flattening. Reads the README by path, so it is `#[ignore]`d — the
    /// `readme_shapes` sample covers the same ground in CI.
    #[test]
    #[ignore]
    fn count_claimed_in_the_repository_readme() {
        let src = std::fs::read_to_string(std::env::var("KAGI_MD_FILE").unwrap()).unwrap();
        let flat = flatten_html_blocks(&src);
        fn walk(n: &Node, out: &mut Vec<Node>) {
            out.push(n.clone());
            if let Some(c) = n.children() {
                for x in c {
                    walk(x, out);
                }
            }
        }
        let root = to_mdast(&flat, &ParseOptions::gfm()).unwrap();
        let mut all = Vec::new();
        walk(&root, &mut all);
        let claimed: Vec<String> = all
            .iter()
            .flat_map(standalone_images)
            .map(|i| i.url)
            .collect();
        println!("claimed {} images:", claimed.len());
        for u in &claimed {
            println!("  {u}");
        }
    }
}
