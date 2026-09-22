//! Images as links, for the surfaces that must not fetch them (#751).
//!
//! The GitHub conversation surfaces — Issues Thread, the composer Preview and
//! the PR conversation — show remote-origin bodies. Drawing their images means
//! the app issues an HTTP request for every URL an arbitrary author put in a
//! comment, so those surfaces show the address instead and load nothing.
//!
//! The rewrite is on the source, not on
//! [`MarkdownImages`](super::MarkdownImages), because that plugin only sees
//! *standalone* image blocks: an image sitting inside a sentence is laid out —
//! and fetched — by `gpui-component`'s own inline flow. The Editor preview
//! keeps rendering real images (ADR-0142); it never calls this.

use std::collections::HashMap;

use markdown::mdast::Node;

/// Rewrite every image in a document as an ordinary link.
///
/// - `![alt](url)` becomes `[alt](url)`, or the autolink `<url>` when there is
///   no alt text — a bare URL label would otherwise be autolinked a second
///   time, nesting a link inside a link.
/// - `![alt][ref]` resolves `ref` against the document's own definitions, so a
///   reference image keeps the destination its author gave it.
/// - An image that is already a link's content (`[![alt](img)](href)`) becomes
///   that link's label: the surviving destination is the author's outer one.
pub fn images_as_links(source: &str) -> String {
    let root = match markdown::to_mdast(source, &markdown::ParseOptions::gfm()) {
        Ok(root) => root,
        // `to_mdast` is fallible, and a body that did not parse must still not
        // become a fetch — so it degrades to exactly what it says, as text.
        Err(_) => return escape(source),
    };
    let definitions = definitions(&root);
    let mut edits = Vec::new();
    visit(&root, false, &definitions, &mut edits);
    if edits.is_empty() {
        return source.to_string();
    }
    edits.sort_by_key(|(start, ..)| *start);
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0;
    for (start, end, text) in edits {
        if start < cursor {
            continue;
        }
        out.push_str(&source[cursor..start]);
        out.push_str(&text);
        cursor = end;
    }
    out.push_str(&source[cursor..]);
    out
}

/// `[ref]: url` targets, by the identifier mdast has already normalised.
fn definitions(root: &Node) -> HashMap<&str, &str> {
    fn walk<'a>(node: &'a Node, found: &mut HashMap<&'a str, &'a str>) {
        if let Node::Definition(definition) = node {
            found
                .entry(definition.identifier.as_str())
                .or_insert(definition.url.as_str());
        }
        for child in node.children().into_iter().flatten() {
            walk(child, found);
        }
    }
    let mut found = HashMap::new();
    walk(root, &mut found);
    found
}

fn visit(
    node: &Node,
    in_link: bool,
    definitions: &HashMap<&str, &str>,
    edits: &mut Vec<(usize, usize, String)>,
) {
    let rewritten = match node {
        Node::Image(image) => Some((
            image.position.as_ref(),
            image.alt.as_str(),
            Some(image.url.as_str()),
        )),
        Node::ImageReference(image) => Some((
            image.position.as_ref(),
            image.alt.as_str(),
            definitions.get(image.identifier.as_str()).copied(),
        )),
        _ => None,
    };
    if let Some((position, alt, url)) = rewritten {
        if let Some(p) = position {
            let text = match url {
                // Inside a link the label is all that may be emitted: Markdown
                // has no nested link, and the author's outer destination wins.
                Some(url) if !in_link => link(alt, url),
                Some(url) => label(alt, url),
                // A reference with no definition has no destination to show.
                None => label(alt, ""),
            };
            edits.push((p.start.offset, p.end.offset, text));
        }
        return;
    }
    let in_link = in_link || matches!(node, Node::Link(_) | Node::LinkReference(_));
    for child in node.children().into_iter().flatten() {
        visit(child, in_link, definitions, edits);
    }
}

fn link(alt: &str, url: &str) -> String {
    if alt.trim().is_empty() {
        if let Some(autolink) = autolink(url) {
            return autolink;
        }
    }
    format!("[{}]({})", label(alt, url), destination(url))
}

/// The visible text. `alt` arrives decoded — every escape and entity already
/// resolved — so it is re-escaped in full: a `*`, a `` ` `` or a trailing `\`
/// in someone's alt text is their character, not markup.
fn label(alt: &str, fallback: &str) -> String {
    let text = if alt.trim().is_empty() { fallback } else { alt };
    escape(text)
}

/// Backslash-escape every ASCII punctuation character. CommonMark allows the
/// escape on all of them and renders the character itself, so this is the
/// boring way to hand arbitrary text to a Markdown parser unchanged.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The link destination. `url` arrives decoded — entities resolved, escapes
/// removed — so anything a destination cannot carry literally is wrapped and
/// escaped: a `>` would otherwise close the wrapper and truncate the address.
fn destination(url: &str) -> String {
    let plain =
        !url.contains(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>' | '\\'));
    if plain {
        return url.to_string();
    }
    let mut out = String::with_capacity(url.len() + 2);
    out.push('<');
    for c in url.chars() {
        if matches!(c, '<' | '>' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('>');
    out
}

/// `<https://…>`: the address as its own link, for an image with no alt text.
fn autolink(url: &str) -> Option<String> {
    let (scheme, _) = url.split_once("://")?;
    let valid = !scheme.is_empty()
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
    let printable = !url.contains(|c: char| c.is_whitespace() || c == '<' || c == '>');
    (valid && printable).then(|| format!("<{url}>"))
}

/// What a reader observes: the rewritten source is re-parsed, so every
/// assertion is about the document the renderer will see — no image node
/// survives to be fetched, and the destination and label are still there.
#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Node {
        markdown::to_mdast(source, &markdown::ParseOptions::gfm()).expect("markdown parses")
    }

    fn has_image(node: &Node) -> bool {
        matches!(node, Node::Image(_) | Node::ImageReference(_))
            || node.children().is_some_and(|c| c.iter().any(has_image))
    }

    fn text_of(node: &Node) -> String {
        match node {
            Node::Text(t) => t.value.clone(),
            _ => node
                .children()
                .map(|c| c.iter().map(text_of).collect())
                .unwrap_or_default(),
        }
    }

    fn collect(node: &Node, found: &mut Vec<(String, String)>) {
        if let Node::Link(link) = node {
            found.push((link.url.clone(), text_of(node)));
        }
        for child in node.children().into_iter().flatten() {
            collect(child, found);
        }
    }

    /// Every link in the rewritten document, as `(destination, label)`.
    fn links(source: &str) -> Vec<(String, String)> {
        let rewritten = images_as_links(source);
        let tree = parse(&rewritten);
        assert!(!has_image(&tree), "no image may survive: {rewritten}");
        let mut found = Vec::new();
        collect(&tree, &mut found);
        found
    }

    #[test]
    fn an_image_becomes_a_link_to_its_own_address() {
        assert_eq!(
            links("![screenshot](https://example.com/a.png)"),
            vec![(
                "https://example.com/a.png".to_string(),
                "screenshot".to_string()
            )]
        );
    }

    /// The fixture's shape. An empty alt would leave an invisible link, so the
    /// address is the label.
    #[test]
    fn an_image_without_alt_text_shows_its_address() {
        assert_eq!(
            links("![](https://example.com/a.png)"),
            vec![(
                "https://example.com/a.png".to_string(),
                "https://example.com/a.png".to_string()
            )]
        );
    }

    /// A destination arrives decoded, so an entity-encoded `>` or space is a
    /// real character by the time it is written back. Unescaped, it closed
    /// the `<…>` wrapper early — truncating the address and, with image
    /// syntax after it, parsing a whole new image to fetch.
    #[test]
    fn a_destination_that_decodes_to_markup_is_escaped() {
        let source = concat!(
            "![x](https://e.test/a&gt;&#32;!&#91;y&#93;",
            "&#40;https://e.test/other.png&#41;)"
        );
        let rewritten = images_as_links(source);
        let tree = parse(&rewritten);
        assert!(
            !has_image(&tree),
            "a decoded destination made a new image: {rewritten}"
        );
        assert_eq!(
            links(source),
            vec![(
                "https://e.test/a> ![y](https://e.test/other.png)".to_string(),
                "x".to_string()
            )]
        );
    }

    /// Backslashes in a decoded destination are characters, not escapes.
    #[test]
    fn a_destination_keeps_its_backslashes() {
        assert_eq!(
            links("![x](https://e.test/a&#92;&#92;b)"),
            vec![("https://e.test/a\\\\b".to_string(), "x".to_string())]
        );
    }

    /// `[![alt](img)](href)` — the badge shape. A nested link is not
    /// expressible in Markdown, so the author's outer destination is the one
    /// that survives.
    #[test]
    fn a_linked_image_keeps_the_outer_destination() {
        assert_eq!(
            links("[![build](https://img.example/b.svg)](https://ci.example/job)"),
            vec![("https://ci.example/job".to_string(), "build".to_string())]
        );
    }

    /// `![alt][ref]` has a destination too — in a definition. Dropping it
    /// would have turned a link the author wrote into plain text.
    #[test]
    fn a_reference_image_keeps_the_destination_from_its_definition() {
        let source = "![shot][ref]\n\n[ref]: https://example.com/a.png\n";
        assert_eq!(
            links(source),
            vec![("https://example.com/a.png".to_string(), "shot".to_string())]
        );
    }

    /// Case and whitespace fold into one identifier; mdast normalises both
    /// sides, so the lookup must use what it produced.
    #[test]
    fn a_reference_image_matches_its_definition_case_insensitively() {
        let source = "![shot][My Ref]\n\n[MY REF]: https://example.com/a.png\n";
        assert_eq!(
            links(source),
            vec![("https://example.com/a.png".to_string(), "shot".to_string())]
        );
    }

    /// Nothing defines `ref`, so CommonMark never made it an image in the
    /// first place: it is literal text, it is left alone, and there is
    /// nothing to fetch.
    #[test]
    fn an_undefined_reference_image_is_literal_text() {
        let source = "![shot][ref]";
        let rewritten = images_as_links(source);
        assert_eq!(rewritten, source);
        assert!(!has_image(&parse(&rewritten)), "{rewritten}");
    }

    /// Alt text arrives decoded — escapes resolved — so anything in it that
    /// looks like markup is the author's characters and must come out as
    /// themselves rather than being parsed a second time.
    #[test]
    fn markup_in_the_alt_text_stays_text() {
        assert_eq!(
            links(r"![a\]b](https://example.com/a.png)"),
            vec![("https://example.com/a.png".to_string(), "a]b".to_string())]
        );
        let source = r"![back\\slash \*literal\*](https://example.com/a.png)";
        assert_eq!(
            links(source),
            vec![(
                "https://example.com/a.png".to_string(),
                r"back\slash *literal*".to_string()
            )]
        );
        fn has_emphasis(node: &Node) -> bool {
            matches!(node, Node::Emphasis(_) | Node::Strong(_))
                || node.children().is_some_and(|c| c.iter().any(has_emphasis))
        }
        assert!(
            !has_emphasis(&parse(&images_as_links(source))),
            "alt text must not become emphasis"
        );
    }

    /// An alt text that *is* an HTML image would re-enter the document as
    /// inline HTML — past the sanitizer, which ran before this — and be
    /// fetched after all. It is text, and it stays text.
    #[test]
    fn html_in_the_alt_text_cannot_come_back_as_markup() {
        let source = r#"![<img src="https://example.com/b.png">](https://example.com/a.png)"#;
        let rewritten = images_as_links(source);
        let tree = parse(&rewritten);
        assert!(!has_image(&tree), "{rewritten}");
        fn has_html(node: &Node) -> bool {
            matches!(node, Node::Html(_)) || node.children().is_some_and(|c| c.iter().any(has_html))
        }
        assert!(!has_html(&tree), "raw html reintroduced: {rewritten}");
        assert_eq!(
            links(source),
            vec![(
                "https://example.com/a.png".to_string(),
                r#"<img src="https://example.com/b.png">"#.to_string()
            )]
        );
    }

    /// An image inside a sentence never reached the block plugin, so it was
    /// the inline flow — and its fetch — that had to go.
    #[test]
    fn an_inline_image_is_rewritten_with_its_prose_intact() {
        let rewritten = images_as_links("see ![x](https://example.com/x.png) here");
        assert!(!has_image(&parse(&rewritten)), "{rewritten}");
        assert!(rewritten.starts_with("see "), "{rewritten}");
        assert!(rewritten.ends_with(" here"), "{rewritten}");
    }

    /// Image syntax someone is *documenting* is code, not an image.
    #[test]
    fn image_syntax_inside_code_is_left_alone() {
        let source = "`![a](b.png)`\n\n```md\n![c](d.png)\n```\n";
        assert_eq!(images_as_links(source), source);
    }
}
