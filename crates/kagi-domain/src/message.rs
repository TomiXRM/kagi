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

/// Drop the comment lines from a `commit.template` file.
///
/// git strips these when the editor exits; kagi has no editor step, so the
/// template is stripped on load and what the user sees is what gets committed.
pub fn strip_template_comments(text: &str) -> String {
    let kept: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect();
    kept.join("\n").trim_end().to_string()
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
