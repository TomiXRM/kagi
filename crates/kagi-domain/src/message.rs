//! Commit-message presentation helpers (pure).

/// Join hard-wrapped lines within a paragraph so the message soft-wraps to the
/// panel width. Blank lines stay paragraph breaks; lines that look
/// preformatted (indented, bullets, quotes, code fences) are kept verbatim.
pub fn reflow_message(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    let mut prev_joinable = false;
    for line in msg.split('\n') {
        let verbatim = line.is_empty()
            || line.starts_with([' ', '\t', '-', '*', '>', '#', '`'])
            || line.split_once(':').is_some_and(|(k, v)| {
                // trailer line ("Co-Authored-By: …", "Signed-off-by: …");
                // hyphenated single-word key — "fix: …" prose still joins
                !k.contains(' ') && k.contains('-') && !v.is_empty()
            });
        if prev_joinable && !verbatim {
            out.push(' ');
        } else if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
        prev_joinable = !verbatim;
    }
    out
}

/// Split a commit message into its subject line and the rest of the body,
/// dropping the blank line git puts between them.
///
/// The commit panel authors these as two separate inputs; drafts and generated
/// messages are still stored as one string, so every crossing of that boundary
/// goes through this pair.
pub fn split_title_body(msg: &str) -> (String, String) {
    match msg.split_once('\n') {
        Some((title, rest)) => (title.to_string(), rest.trim_start_matches('\n').to_string()),
        None => (msg.to_string(), String::new()),
    }
}

/// Inverse of [`split_title_body`]: the git convention of subject, blank line,
/// body. An empty body yields the subject alone (no trailing blank line).
pub fn join_title_body(title: &str, body: &str) -> String {
    let (title, body) = (title.trim(), body.trim());
    if body.is_empty() {
        title.to_string()
    } else {
        format!("{title}\n\n{body}")
    }
}

/// Whether a line is a commit-message comment (git's `core.commentChar`, `#`).
pub fn is_comment_line(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

/// Drop the comment lines from a `commit.template` file.
///
/// git strips these when the editor exits; kagi has no editor step, so the
/// template is stripped on load and what the user sees is what gets committed.
pub fn strip_template_comments(text: &str) -> String {
    let kept: Vec<&str> = text.lines().filter(|l| !is_comment_line(l)).collect();
    kept.join("\n").trim_end().to_string()
}

/// Make GitHub-flavoured markdown safe for kagi's native renderer.
///
/// gpui-component's inline layout asserts that no text run contains a raw
/// `\n`; mdast keeps line endings inside inline nodes, and a code span or
/// emphasis that wraps across lines reaches the layouter with the newline
/// still inside — a hard panic. So: CRLF → LF, and any newline inside an
/// inline code span (backticks, outside fenced blocks) becomes a space, which
/// is what CommonMark renders anyway.
pub fn sanitize_markdown_for_view(src: &str) -> String {
    let src = src.replace("\r\n", "\n").replace('\r', "\n");
    // Raw HTML (bot footers: `<!-- -->` comments, `<a><picture>…`, `<details>`)
    // is handed to the layouter as one multi-line text run — the actual crash
    // on a codesmith footer. Drop comments entirely and reduce tags to their
    // text content (a `<details><summary>x</summary>` degrades to "x …").
    let src = strip_html(&src);
    let mut out = String::with_capacity(src.len());
    // The fenced block we are inside, tracked by its own marker and run
    // length — the same [`Fence`] `strip_html` uses. A boolean toggled on
    // every ``` line calls the inner ```rust of a ````-quoted block a *close*,
    // and then rewrites the newlines of the program it quotes (#751).
    let mut fence: Option<Fence> = None;
    // Open inline code span carried across lines (its `\n` becomes a space).
    let mut in_span = false;
    for (i, line) in src.split('\n').enumerate() {
        if i > 0 {
            out.push(if in_span && fence.is_none() {
                ' '
            } else {
                '\n'
            });
        }
        if let Some(open) = fence {
            if open.closed_by(line) {
                fence = None;
            }
            out.push_str(line);
            continue;
        }
        if let Some(open) = Fence::opened_by(line) {
            fence = Some(open);
            in_span = false;
            out.push_str(line);
            continue;
        }
        // Track backtick spans within the line (a run of N backticks opens /
        // closes a span; unmatched runs stay open into the next line).
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            out.push(c);
            if c == '`' {
                while chars.peek() == Some(&'`') {
                    out.push(chars.next().unwrap());
                }
                in_span = !in_span;
            }
        }
    }
    // issue #356: PR bodies / review comments are remote-origin text. CR was
    // already folded to '\n' above; neutralize the remaining terminal control
    // bytes (ANSI escapes, DEL, C1) before the text reaches a display run.
    // CJK/emoji are preserved.
    crate::text_safety::sanitize_control_bytes(&out)
}

/// The ``` / ~~~ run a fenced block opens with.
///
/// A boolean "inside a fence" flag cannot tell a ```` ```` ```` block that
/// *quotes* ``` from a second fence: the run's character and length are what
/// decides where the block ends, so everything between stays literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fence {
    marker: u8,
    len: usize,
}

impl Fence {
    /// The fence this line opens, if it opens one.
    pub fn opened_by(line: &str) -> Option<Self> {
        let run = line.trim_start().as_bytes();
        let marker = *run.first()?;
        if marker != b'`' && marker != b'~' {
            return None;
        }
        let len = run.iter().take_while(|b| **b == marker).count();
        (len >= 3).then_some(Self { marker, len })
    }

    /// Whether this line closes the block: the same marker, a run at least
    /// as long, and nothing else on the line.
    pub fn closed_by(&self, line: &str) -> bool {
        let line = line.trim();
        line.len() >= self.len && line.bytes().all(|b| b == self.marker)
    }
}

/// Remove `<!-- … -->` comments and HTML tags, keeping text content.
///
/// Fenced blocks and inline code spans are verbatim: a `<T>` — or a literal
/// `<!-- … -->` someone is *documenting* — is code, not markup (#751). An
/// HTML comment opened and never closed keeps swallowing to the end of the
/// document, which is what CommonMark (and therefore GitHub) shows.
fn strip_html(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut fence: Option<Fence> = None;
    // An HTML comment left open by an earlier line.
    let mut in_comment = false;
    for (i, line) in src.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if let Some(open) = fence {
            if open.closed_by(line) {
                fence = None;
            }
            out.push_str(line);
            continue;
        }
        if in_comment {
            let Some(end) = line.find("-->") else {
                continue;
            };
            in_comment = false;
            strip_html_line(&line[end + 3..], &mut out, &mut in_comment);
            continue;
        }
        if let Some(open) = Fence::opened_by(line) {
            fence = Some(open);
            out.push_str(line);
            continue;
        }
        strip_html_line(line, &mut out, &mut in_comment);
    }
    out
}

fn strip_html_line(line: &str, out: &mut String, in_comment: &mut bool) {
    let bytes = line.as_bytes();
    // The backtick run that opened the code span we are inside, if any. A
    // span closes on a run of the same length, so ``a`b`` is one span.
    let mut span: Option<usize> = None;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
            out.push_str(&line[i..i + run]);
            span = match span {
                Some(open) if open == run => None,
                open @ Some(_) => open,
                None => Some(run),
            };
            i += run;
            continue;
        }
        if bytes[i] != b'<' || span.is_some() {
            let next = line[i..]
                .find(['`', '<'])
                .map_or(line.len(), |offset| i + offset)
                .max(i + 1);
            out.push_str(&line[i..next]);
            i = next;
            continue;
        }
        if line[i..].starts_with("<!--") {
            match line[i..].find("-->") {
                Some(end) => i += end + 3,
                None => {
                    *in_comment = true;
                    return;
                }
            }
            continue;
        }
        // A tag: `<` followed by a letter, `/` or `!` — otherwise it is a
        // literal (e.g. "a < b").
        let is_tag = matches!(bytes.get(i + 1), Some(n) if n.is_ascii_alphabetic() || *n == b'/' || *n == b'!');
        if !is_tag {
            out.push('<');
            i += 1;
            continue;
        }
        let Some(end) = line[i..].find('>') else {
            return;
        };
        // Block-ish tags become a space so words don't glue.
        out.push(' ');
        i += end + 1;
    }
}

#[cfg(test)]
mod markdown_sanitize_tests {
    use super::sanitize_markdown_for_view;

    #[test]
    fn html_comments_and_tags_are_stripped_but_code_kept() {
        let s = "text\n<!-- codesmith:footer -->\n---\n<a href=\"x\"><picture><source media=\"y\"></picture></a>\n<sup>Need help? Tag <code>@codesmith</code></sup>\n`<T>` stays\n";
        let out = sanitize_markdown_for_view(s);
        assert!(!out.contains("<!--"), "{out}");
        assert!(!out.contains("<a "), "{out}");
        assert!(out.contains("Need help? Tag  @codesmith"), "{out}");
        assert!(out.contains("`<T>` stays"), "{out}");
    }

    /// A comment is hidden; a comment someone is *quoting* inside code is
    /// content. The whole-document second pass could not tell them apart and
    /// deleted both (#751).
    #[test]
    fn a_comment_is_hidden_but_a_quoted_one_inside_code_is_kept() {
        let out = sanitize_markdown_for_view(
            "before <!-- hidden --> after\n\n`literal <!-- kept -->`\n\n```rust\n// <!-- fenced -->\n```\n",
        );
        assert!(!out.contains("hidden"), "{out}");
        assert!(out.contains("`literal <!-- kept -->`"), "{out}");
        assert!(out.contains("// <!-- fenced -->"), "{out}");
    }

    /// A four-backtick fence quotes ``` — the block ends at a run as long as
    /// the one that opened it, not at the first fence-looking line.
    #[test]
    fn a_longer_fence_keeps_the_shorter_one_inside_it_literal() {
        let s = "````md\n```rust\n// <!-- kept -->\n```\n````\n";
        assert_eq!(sanitize_markdown_for_view(s), s);
    }

    /// The newline rewrite has to read the same fence the HTML strip does.
    /// With a boolean toggled on every fence-looking line, the inner
    /// ```` ```rust ```` closed the block, and the multi-line code span in the
    /// program it quotes had its newline replaced by a space (#751).
    #[test]
    fn a_quoted_fence_keeps_the_newlines_of_the_program_inside_it() {
        let s = "````md\n```rust\nlet s = `one\ntwo`;\n```\n````\nprose `a\nb` c\n";
        assert_eq!(
            sanitize_markdown_for_view(s),
            "````md\n```rust\nlet s = `one\ntwo`;\n```\n````\nprose `a b` c\n",
            "only the span in the prose below the block is folded"
        );
        let tildes = "~~~~\n~~~\n`one\ntwo`\n~~~\n~~~~\n";
        assert_eq!(sanitize_markdown_for_view(tildes), tildes);
    }

    #[test]
    fn crlf_becomes_lf() {
        assert_eq!(sanitize_markdown_for_view("a\r\nb"), "a\nb");
    }

    #[test]
    fn newline_inside_inline_code_becomes_space() {
        assert_eq!(sanitize_markdown_for_view("x `a\nb` y"), "x `a b` y");
    }

    #[test]
    fn fenced_blocks_are_left_alone() {
        let s = "```\nlet a;\nlet b;\n```\n";
        assert_eq!(sanitize_markdown_for_view(s), s);
    }

    #[test]
    fn control_bytes_escaped_cjk_preserved() {
        // issue #356: a PR body / review comment carrying ANSI escapes is
        // neutralized while legitimate CJK/emoji pass through.
        let s = "見出し 🎉\n\nbody \x1b[31mred\x07 中文";
        let out = sanitize_markdown_for_view(s);
        assert!(!out.contains('\x1b'), "ESC escaped: {out}");
        assert!(!out.contains('\x07'), "BEL escaped: {out}");
        assert!(out.contains("\\x1B"), "visible escape: {out}");
        assert!(out.contains("見出し"), "CJK preserved: {out}");
        assert!(out.contains("🎉"), "emoji preserved: {out}");
        assert!(out.contains("中文"), "CJK preserved: {out}");
        // LF (paragraph break) must survive.
        assert!(out.contains('\n'), "newlines preserved: {out}");
    }
}

#[cfg(test)]
mod title_body_tests {
    use super::*;

    #[test]
    fn splits_on_the_blank_line_after_the_subject() {
        let (t, b) = split_title_body("subject\n\nbody line one\nbody line two");
        assert_eq!(t, "subject");
        assert_eq!(b, "body line one\nbody line two");
    }

    #[test]
    fn subject_only_message_has_an_empty_body() {
        assert_eq!(
            split_title_body("just a subject"),
            ("just a subject".into(), String::new())
        );
    }

    #[test]
    fn round_trips_through_join() {
        let msg = "subject\n\nbody";
        let (t, b) = split_title_body(msg);
        assert_eq!(join_title_body(&t, &b), msg);
    }

    #[test]
    fn join_without_a_body_leaves_no_trailing_blank_line() {
        assert_eq!(join_title_body("subject", "   "), "subject");
    }

    /// A body that starts immediately (no blank separator) still belongs to the
    /// body — git would treat it that way too.
    #[test]
    fn handles_a_missing_blank_separator() {
        let (t, b) = split_title_body("subject\nbody");
        assert_eq!((t.as_str(), b.as_str()), ("subject", "body"));
    }

    #[test]
    fn strips_comment_lines_from_a_template() {
        let tpl = "\n# Please enter a message\nSummary:\n#  more help\nWhy:\n";
        assert_eq!(strip_template_comments(tpl), "\nSummary:\nWhy:");
    }

    /// The common shape: a template that is *entirely* a cheat-sheet of
    /// comments. It must survive in the body input (so the author can read it)
    /// and strip to nothing at commit time — stripping it on load instead made
    /// the template look like it had failed to load at all.
    #[test]
    fn a_comment_only_template_strips_to_nothing() {
        let tpl = "\n# ==== Emojis ====\n# ✨ :sparkles: Add new feature\n#\n# Subject\n";
        assert_eq!(strip_template_comments(tpl), "");
    }

    #[test]
    fn keeps_markdown_headings_that_are_not_leading_comments() {
        // A '#' inside a line is not a comment marker.
        assert_eq!(
            strip_template_comments("fix: #123 crash"),
            "fix: #123 crash"
        );
    }
}

#[cfg(test)]
mod reflow_tests {
    use super::reflow_message;

    #[test]
    fn joins_hard_wrapped_paragraph() {
        assert_eq!(
            reflow_message("subject\n\nfirst line\nsecond line"),
            "subject\n\nfirst line second line"
        );
    }

    #[test]
    fn keeps_bullets_blanks_and_trailers() {
        let msg = "s\n\n- item one\n- item two\n\nCo-Authored-By: X <x@y>";
        assert_eq!(reflow_message(msg), msg);
    }

    #[test]
    fn prose_with_colon_still_joins() {
        assert_eq!(
            reflow_message("fix: the thing\nbroke because reasons"),
            "fix: the thing broke because reasons"
        );
    }
}

/// Byte ranges of explicit HTTP(S) URLs in plain commit text, not Markdown.
/// Japanese prose/punctuation and unmatched closing brackets terminate a URL;
/// balanced ASCII brackets inside paths remain part of it.
pub fn message_url_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut offset = 0;
    while offset < text.len() {
        let remaining = &text[offset..];
        let Some(relative) = remaining
            .find("http://")
            .into_iter()
            .chain(remaining.find("https://"))
            .min()
        else {
            break;
        };
        let start = offset + relative;
        let scheme_len = if text[start..].starts_with("https://") {
            8
        } else {
            7
        };
        let mut end = start + scheme_len;
        let mut brackets = [0usize; 3];
        for (index, ch) in text[end..].char_indices() {
            if ch.is_whitespace()
                || ch.is_control()
                || matches!(ch, '<' | '>' | '"' | '\'' | '`' | '\\')
                || !ch.is_ascii()
            {
                break;
            }
            if let Some(slot) = ['(', '[', '{'].iter().position(|&c| c == ch) {
                brackets[slot] += 1;
            }
            if let Some(slot) = [')', ']', '}'].iter().position(|&c| c == ch) {
                if brackets[slot] == 0 {
                    break;
                }
                brackets[slot] -= 1;
            }
            end = start + scheme_len + index + ch.len_utf8();
        }
        while end > start + scheme_len
            && matches!(
                text.as_bytes()[end - 1],
                b'.' | b',' | b';' | b':' | b'!' | b'?'
            )
        {
            end -= 1;
        }
        let authority = text[start + scheme_len..end]
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default();
        let boundary = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && !matches!(c, '_' | ':' | '/' | '.'));
        if boundary && !authority.is_empty() {
            ranges.push(start..end);
        }
        offset = end.max(start + scheme_len);
    }
    ranges
}

/// Inspector's literal HTML, with only detected HTTP(S) URLs made into anchors.
/// Attribute escaping is separate from text escaping; authored HTML stays text.
pub fn message_with_links_to_html(text: &str, links: &[std::ops::Range<usize>]) -> String {
    let mut out = String::with_capacity(text.len() + 32);
    let mut start = 0;
    for range in links {
        escape_message_text(&text[start..range.start], &mut out, false);
        let url = &text[range.clone()];
        out.push_str("<a href=\"");
        escape_message_text(url, &mut out, true);
        out.push_str("\">");
        escape_message_text(url, &mut out, false);
        out.push_str("</a>");
        start = range.end;
    }
    escape_message_text(&text[start..], &mut out, false);
    message_html_paragraphs(&out, |line, out| out.push_str(line))
}

fn escape_message_text(text: &str, out: &mut String, attribute: bool) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            '\'' if attribute => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
}

fn message_html_paragraphs(text: &str, mut write_line: impl FnMut(&str, &mut String)) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(text.len() + 16);
    for para in text
        .split("\n\n")
        .map(|p| p.trim_matches('\n'))
        .filter(|p| !p.trim().is_empty())
    {
        for line in para.split('\n') {
            out.push_str("<p>");
            write_line(line, &mut out);
            out.push_str("</p>");
        }
    }
    out
}

/// Render a commit message as HTML that displays **literally**.
///
/// The only selectable text element available is `TextView`, and it parses
/// either Markdown or HTML — there is no plain-text mode. Markdown is wrong
/// for a commit message: `* fix foo` becomes a bullet list, `#123` becomes a
/// heading, `_x_` goes italic and the underscores vanish. HTML with every
/// special character escaped is the only faithful option.
///
/// Every line becomes its own paragraph, so the author's line breaks survive:
/// the pinned `TextView` drops a `<br>` inside a paragraph, which ran a
/// commit body's bullet lines into one (#946, first seen in the Operation Log
/// detail, #908). Blank lines only separate lines; a run of them adds nothing.
pub fn message_to_html(msg: &str) -> String {
    message_html_paragraphs(msg, |line, out| escape_message_text(line, out, false))
}

#[cfg(test)]
mod message_to_html_tests {
    use super::message_to_html;

    /// The whole reason this exists: a commit message is not Markdown. Each of
    /// these renders as something else entirely if handed to a Markdown parser
    /// — a bullet list, a heading, italics with the underscores eaten.
    #[test]
    fn markdown_syntax_survives_as_literal_text() {
        for src in [
            "* fix the thing",
            "#123 follow-up",
            "snake_case_name stays whole",
            "a * b * c",
            "1. not a list",
            "` backtick",
        ] {
            let html = message_to_html(src);
            let inner = html
                .trim_start_matches("<p>")
                .trim_end_matches("</p>")
                .to_string();
            assert_eq!(inner, src, "{src:?} was altered");
        }
    }

    #[test]
    fn html_special_characters_are_escaped() {
        assert_eq!(
            message_to_html("fix <script> & \"quotes\" > here"),
            "<p>fix &lt;script&gt; &amp; \"quotes\" &gt; here</p>"
        );
    }

    /// A `<` that is not escaped would swallow everything up to the next `>`;
    /// a bare `&` can eat the following word as an entity.
    #[test]
    fn an_email_in_a_trailer_is_not_swallowed() {
        let html = message_to_html("Co-authored-by: A <a@b.example>");
        assert!(html.contains("&lt;a@b.example&gt;"), "{html}");
    }

    /// Each line is a block of its own: a `<br>` inside a paragraph is dropped
    /// by the pinned `TextView`, which joined bullet lines into one (#946).
    #[test]
    fn every_line_is_its_own_paragraph() {
        assert_eq!(
            message_to_html("subject\n\n- one\n- two"),
            "<p>subject</p><p>- one</p><p>- two</p>"
        );
        assert!(!message_to_html("a\nb").contains("<br>"));
    }

    #[test]
    fn runs_of_blank_lines_do_not_produce_empty_paragraphs() {
        assert_eq!(message_to_html("a\n\n\n\n\nb"), "<p>a</p><p>b</p>");
        assert_eq!(message_to_html("   \n\n  \n"), "");
        assert_eq!(message_to_html(""), "");
    }

    #[test]
    fn crlf_is_normalised() {
        assert_eq!(
            message_to_html("a\r\n\r\nb\r\nc"),
            "<p>a</p><p>b</p><p>c</p>"
        );
    }
}

#[cfg(test)]
mod message_link_tests {
    use super::{message_url_ranges, message_with_links_to_html};

    fn urls(text: &str) -> Vec<&str> {
        message_url_ranges(text)
            .into_iter()
            .map(|r| &text[r])
            .collect()
    }

    #[test]
    fn japanese_prose_brackets_and_sentence_punctuation() {
        assert_eq!(
            urls("参照（https://example.com/a）。https://example.com/b日本語"),
            ["https://example.com/a", "https://example.com/b"]
        );
        assert_eq!(
            urls("(https://example.com/a_(b)), http://example.com/c!"),
            ["https://example.com/a_(b)", "http://example.com/c"]
        );
        assert_eq!(
            urls("[https://example.com/a?q=x&n=2#part]."),
            ["https://example.com/a?q=x&n=2#part"]
        );
    }

    #[test]
    fn arbitrary_schemes_embedded_prefixes_and_empty_hosts_are_not_links() {
        assert!(urls("javascript:alert(1) data:text/html,x file:///tmp/a ftp://host xhttps://host javascript:https://host https:///a https://").is_empty());
    }

    #[test]
    fn authored_markup_is_literal_and_attributes_cannot_escape() {
        let text = "<script>alert(1)</script> [label](https://example.com/a?q=x&n=2) <img src=x> https://example.com/\" onclick=alert(1)";
        let html = message_with_links_to_html(text, &message_url_ranges(text));
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(html.contains("&lt;img src=x&gt;"));
        assert!(html.contains("[label](<a href=\"https://example.com/a?q=x&amp;n=2\">https://example.com/a?q=x&amp;n=2</a>)"));
        assert!(!html.contains("<script>"));
        assert!(!html.contains("<img "));
        assert!(html.contains(
            "<a href=\"https://example.com/\">https://example.com/</a>\" onclick=alert(1)"
        ));
    }
}
