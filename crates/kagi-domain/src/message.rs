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
    let mut in_fence = false;
    // Open inline code span carried across lines (its `\n` becomes a space).
    let mut in_span = false;
    for (i, line) in src.split('\n').enumerate() {
        if i > 0 {
            out.push(if in_span && !in_fence { ' ' } else { '\n' });
        }
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            in_span = false;
            out.push_str(line);
            continue;
        }
        if in_fence {
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
    out
}

/// Remove `<!-- … -->` comments and HTML tags, keeping text content. Code
/// spans / fences are left alone (a `<T>` in code is not a tag).
fn strip_html(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_fence = false;
    for (i, line) in src.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
        }
        if in_fence || t.starts_with("```") || t.starts_with("~~~") {
            out.push_str(line);
            continue;
        }
        out.push_str(&strip_html_line(line));
    }
    // Comments may span lines: second pass over the joined text.
    let mut result = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(start) = rest.find("<!--") {
        result.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => {
                rest = "";
            }
        }
    }
    result.push_str(rest);
    result
}

fn strip_html_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_code = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '`' {
            in_code = !in_code;
            out.push(c);
            continue;
        }
        if !in_code && c == '<' && !line.starts_with("<!--") {
            // A tag: `<` followed by a letter, `/` or `!` — otherwise it is
            // a literal (e.g. "a < b").
            let is_tag = matches!(chars.peek(), Some(n) if n.is_ascii_alphabetic() || *n == '/' || *n == '!');
            if is_tag {
                let mut depth_closed = false;
                for n in chars.by_ref() {
                    if n == '>' {
                        depth_closed = true;
                        break;
                    }
                }
                if depth_closed {
                    // Block-ish tags become a space so words don't glue.
                    out.push(' ');
                    continue;
                }
                return out;
            }
        }
        out.push(c);
    }
    out
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
