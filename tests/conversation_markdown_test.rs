//! What the Issues Thread, the composer Preview and the PR conversation are
//! actually handed (#751).
//!
//! These surfaces render remote-origin bodies, so the contract is not "the
//! pipeline produces nice text" — it is that no image node ever reaches the
//! renderer, whatever an author writes, because the renderer would fetch it.
//! Every assertion re-parses the prepared source with the same parser the
//! renderer uses, and the whole chain runs: sanitize, image rewrite, code
//! padding, HTML flattening.

use kagi_ui_editor::markdown::prepare_github_markdown;
use markdown::mdast::Node;

fn parse(source: &str) -> Node {
    markdown::to_mdast(source, &markdown::ParseOptions::gfm()).expect("markdown parses")
}

fn any(node: &Node, f: &dyn Fn(&Node) -> bool) -> bool {
    f(node) || node.children().is_some_and(|c| c.iter().any(|c| any(c, f)))
}

fn text_of(node: &Node) -> String {
    match node {
        Node::Text(t) => t.value.clone(),
        Node::InlineCode(c) => c.value.clone(),
        _ => node
            .children()
            .map(|c| c.iter().map(text_of).collect())
            .unwrap_or_default(),
    }
}

fn links(tree: &Node, found: &mut Vec<(String, String)>) {
    if let Node::Link(link) = tree {
        found.push((link.url.clone(), text_of(tree)));
    }
    for child in tree.children().into_iter().flatten() {
        links(child, found);
    }
}

/// The prepared body, plus the links a reader can click in it.
fn prepared(body: &str) -> (String, Vec<(String, String)>) {
    let out = prepare_github_markdown(body);
    let tree = parse(&out);
    assert!(
        !any(&tree, &|n| matches!(
            n,
            Node::Image(_) | Node::ImageReference(_)
        )),
        "an image reached the renderer: {out}"
    );
    let mut found = Vec::new();
    links(&tree, &mut found);
    (out, found)
}

/// Markdown images become links to their own address; an image with no alt
/// text shows the address, because an empty label is an invisible link.
#[test]
fn images_arrive_as_links() {
    let (_, links) =
        prepared("![shot](https://example.com/a.png)\n\n![](https://example.com/b.png)");
    assert_eq!(
        links,
        vec![
            ("https://example.com/a.png".to_string(), "shot".to_string()),
            (
                "https://example.com/b.png".to_string(),
                "https://example.com/b.png".to_string()
            ),
        ]
    );
}

/// The padding pass runs after the rewrite, and the rewrite escapes alt text.
/// An escaped backtick is a character: padding it would write thin spaces
/// into the author's words, and a stray pad would break the label.
#[test]
fn a_backtick_in_alt_text_survives_the_padding_pass() {
    let (out, links) = prepared(r"![a \` b \` c](https://example.com/a.png)");
    assert!(
        !out.contains('\u{2009}'),
        "escaped backticks were padded: {out}"
    );
    assert_eq!(
        links,
        vec![(
            "https://example.com/a.png".to_string(),
            "a ` b ` c".to_string()
        )]
    );
}

/// A destination is decoded by the time it is written back, so an
/// entity-encoded `>` plus image syntax used to close the `<…>` wrapper and
/// parse a brand-new image — one the renderer would have fetched.
#[test]
fn a_decoded_destination_cannot_rebuild_an_image() {
    let (_, links) = prepared(concat!(
        "![x](https://e.test/a&gt;&#32;!&#91;y&#93;",
        "&#40;https://e.test/other.png&#41;)"
    ));
    assert_eq!(
        links,
        vec![(
            "https://e.test/a> ![y](https://e.test/other.png)".to_string(),
            "x".to_string()
        )]
    );
}

/// Alt text that is itself an HTML image would re-enter the document as raw
/// HTML — after the sanitizer had already run — and be fetched after all.
#[test]
fn html_in_alt_text_cannot_become_markup() {
    let (out, _) = prepared(r#"![<img src="https://e.test/b.png">](https://e.test/a.png)"#);
    let tree = parse(&out);
    assert!(
        !any(&tree, &|n| matches!(n, Node::Html(_))),
        "raw html reached the renderer: {out}"
    );
}

/// Inline images are laid out — and fetched — by the renderer's own text
/// flow, which the block-level image plugin never saw.
#[test]
fn an_inline_image_is_a_link_with_its_sentence_intact() {
    let (out, links) = prepared("see ![x](https://e.test/x.png) here");
    assert!(out.starts_with("see ") && out.ends_with(" here"), "{out}");
    assert_eq!(
        links,
        vec![("https://e.test/x.png".to_string(), "x".to_string())]
    );
}

/// The fixture every #751 surface is checked against: hidden comments stay
/// hidden, quoted ones stay visible, and the constructs the issue names
/// survive the whole chain.
#[test]
fn the_fixture_keeps_its_constructs() {
    let out = prepare_github_markdown(include_str!("support/issues_markdown.md"));
    assert!(!out.contains("HIDDEN_COMMENT_SENTINEL"), "{out}");
    assert!(!out.contains("HIDDEN_BLOCK_COMMENT_SENTINEL"), "{out}");
    assert!(out.contains("<!-- literal-inline-comment -->"), "{out}");
    assert!(out.contains("<!-- literal-fenced-comment -->"), "{out}");

    let tree = parse(&out);
    assert!(any(&tree, &|n| matches!(n, Node::Table(_))), "table: {out}");
    assert!(
        any(
            &tree,
            &|n| matches!(n, Node::ListItem(i) if i.checked == Some(false))
        ),
        "unchecked task: {out}"
    );
    assert!(
        any(
            &tree,
            &|n| matches!(n, Node::ListItem(i) if i.checked == Some(true))
        ),
        "checked task: {out}"
    );
    assert!(
        any(
            &tree,
            &|n| matches!(n, Node::Code(c) if c.lang.as_deref() == Some("rust"))
        ),
        "rust fence: {out}"
    );
    assert!(
        any(&tree, &|n| matches!(n, Node::Code(c) if c.lang.is_none())),
        "plain fence: {out}"
    );
    // The rest of the syntax the issue lists, each as the node that makes it
    // render: a heading that stayed a heading is not the same as text that
    // happens to start with `#`.
    for (what, found) in [
        (
            "h1",
            any(&tree, &|n| matches!(n, Node::Heading(h) if h.depth == 1)),
        ),
        (
            "h2",
            any(&tree, &|n| matches!(n, Node::Heading(h) if h.depth == 2)),
        ),
        (
            "h3",
            any(&tree, &|n| matches!(n, Node::Heading(h) if h.depth == 3)),
        ),
        (
            "ordered list",
            any(&tree, &|n| matches!(n, Node::List(l) if l.ordered)),
        ),
        (
            "bullet list",
            any(&tree, &|n| matches!(n, Node::List(l) if !l.ordered)),
        ),
        ("bold", any(&tree, &|n| matches!(n, Node::Strong(_)))),
        ("italic", any(&tree, &|n| matches!(n, Node::Emphasis(_)))),
        (
            "strikethrough",
            any(&tree, &|n| matches!(n, Node::Delete(_))),
        ),
        (
            "inline code",
            any(&tree, &|n| matches!(n, Node::InlineCode(_))),
        ),
        (
            "blockquote",
            any(&tree, &|n| matches!(n, Node::Blockquote(_))),
        ),
        ("link", any(&tree, &|n| matches!(n, Node::Link(_)))),
        (
            "thematic break",
            any(&tree, &|n| matches!(n, Node::ThematicBreak(_))),
        ),
    ] {
        assert!(found, "{what} did not survive the pipeline: {out}");
    }
    // A nested list is a list inside a list item, not a flattened one.
    assert!(
        any(&tree, &|n| matches!(n, Node::ListItem(i) if i
            .children
            .iter()
            .any(|c| matches!(c, Node::List(_))))),
        "nested list: {out}"
    );
    // `#123` and `@login` have no GitHub meaning here; they must at least
    // still read as themselves.
    assert!(out.contains("#123") && out.contains("@login"), "{out}");
}

/// A fenced block is program text. The padding used to write thin spaces into
/// what the reader copies out of it.
#[test]
fn fenced_code_is_left_exactly_as_written() {
    let out = prepare_github_markdown("````md\n```rust\nlet a = `b`;\n```\n````\n");
    assert!(!out.contains('\u{2009}'), "fence padded: {out}");
    assert!(out.contains("let a = `b`;"), "{out}");
}
