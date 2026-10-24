//! Git's conflict-path block in MERGE_MSG; pure parsing only.

/// Git's automatic comment characters, in preference order. A recorded conflict
/// heading identifies the choice already made by Git; otherwise select the first
/// candidate not used at the start of a message line (Git's auto rule).
pub fn auto_comment_char(message: &str) -> char {
    const CANDIDATES: &str = "#;@!$%^&|:";
    for candidate in CANDIDATES.chars() {
        if message.lines().any(|line| {
            line.strip_prefix(candidate)
                .is_some_and(|rest| rest.trim_end_matches('\r') == " Conflicts:")
        }) {
            return candidate;
        }
    }
    CANDIDATES
        .chars()
        .find(|candidate| !message.lines().any(|line| line.starts_with(*candidate)))
        .unwrap_or('#')
}

/// Parse the block using the effective repository comment character. Keep path
/// bytes after the tab verbatim apart from a CRLF terminator; stop at prose or
/// non-comment text and deduplicate without changing order.
pub fn conflicted_paths(merge_msg: &str, comment_char: char) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    let mut in_block = false;
    for line in merge_msg.lines() {
        let line = line.trim_end_matches('\r');
        let comment = line.strip_prefix(comment_char);
        if !in_block {
            if comment == Some(" Conflicts:") {
                in_block = true;
            }
            continue;
        }
        let Some(path) = comment.and_then(|rest| rest.strip_prefix('\t')) else {
            break;
        };
        if path.is_empty() {
            break;
        }
        if !paths.iter().any(|known| known == path) {
            paths.push(path.to_string());
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::{auto_comment_char, conflicted_paths};

    #[test]
    fn preserves_path_edge_whitespace() {
        let msg = "# Conflicts:\r\n#\t leading and trailing \r\n";
        assert_eq!(conflicted_paths(msg, '#'), vec![" leading and trailing "]);
    }

    #[test]
    fn parses_git_block() {
        let msg = "Merge branch 'topic'\n\n# Conflicts:\n#\tsrc/a.rs\n#\tdocs/b.md\n";
        assert_eq!(conflicted_paths(msg, '#'), vec!["src/a.rs", "docs/b.md"]);
    }

    #[test]
    fn parses_semicolon_and_auto_comment_char() {
        let msg = "# message line\n; Conflicts:\n;\tfile.txt\n";
        assert_eq!(conflicted_paths(msg, ';'), vec!["file.txt"]);
        assert_eq!(auto_comment_char(msg), ';');
        assert_eq!(
            conflicted_paths(msg, auto_comment_char(msg)),
            vec!["file.txt"]
        );
        assert_eq!(auto_comment_char("ordinary message\n"), '#');
        assert_eq!(auto_comment_char("# message\n; message\n"), '@');
        assert_eq!(auto_comment_char("# Conflicts:\n#\tfile.txt\n"), '#');
    }

    #[test]
    fn stops_at_prose_hint_or_text() {
        let msg = "# Conflicts:\n#\tone\n# It looks like you may be committing a merge.\n#\ttwo\n";
        assert_eq!(conflicted_paths(msg, '#'), vec!["one"]);
        let msg = "# Conflicts:\n#\tone\nbody\n#\ttwo\n";
        assert_eq!(conflicted_paths(msg, '#'), vec!["one"]);
    }

    #[test]
    fn missing_block_yields_nothing() {
        assert!(conflicted_paths("Merge branch 'topic'\n", '#').is_empty());
        assert!(conflicted_paths("", '#').is_empty());
    }

    #[test]
    fn dedups_and_handles_crlf() {
        let msg = "# Conflicts:\r\n#\ta b.txt\r\n#\ta b.txt\r\n#    not a path\r\n";
        assert_eq!(conflicted_paths(msg, '#'), vec!["a b.txt"]);
    }
}
