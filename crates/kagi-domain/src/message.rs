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
