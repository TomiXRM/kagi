//! Git's conflict-path block in MERGE_MSG; pure byte parsing only.

/// Resolve the configured character using the same recorded-message evidence
/// for Saved paths and the merge commit composer.
pub fn effective_comment_char(configured: Option<&str>, message: &[u8]) -> u8 {
    match configured {
        Some("auto") => auto_comment_char(message),
        Some(value) => value.as_bytes().first().copied().unwrap_or(b'#'),
        None => b'#',
    }
}

/// Git comments start in column zero; indented prose remains user content.
pub fn non_comment_lines(message: &str, comment_char: u8) -> impl Iterator<Item = &str> {
    message
        .lines()
        .filter(move |line| line.as_bytes().first() != Some(&comment_char))
}

/// Strip generated comments without interpreting other punctuation as comments.
pub fn strip_comments(message: &str, comment_char: u8) -> String {
    let mut stripped = String::with_capacity(message.len());
    for line in non_comment_lines(message, comment_char) {
        stripped.push_str(line);
        stripped.push('\n');
    }
    stripped.truncate(stripped.trim_end().len());
    stripped
}

/// Git's automatic comment characters, in preference order. A recorded conflict
/// heading identifies the choice already made by Git; otherwise select the first
/// candidate not used at the start of a message line (Git's auto rule).
pub fn auto_comment_char(message: &[u8]) -> u8 {
    const CANDIDATES: &[u8] = b"#;@!$%^&|:";
    for &candidate in CANDIDATES {
        if message.split(|&byte| byte == b'\n').any(|line| {
            line.strip_prefix(&[candidate])
                .is_some_and(|rest| strip_cr(rest) == b" Conflicts:")
        }) {
            return candidate;
        }
    }
    CANDIDATES
        .iter()
        .copied()
        .find(|candidate| {
            !message
                .split(|&byte| byte == b'\n')
                .any(|line| line.first() == Some(candidate))
        })
        .unwrap_or(b'#')
}

fn strip_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// Parse the block using the effective repository comment character. Keep path
/// bytes after the tab verbatim apart from a CRLF terminator; stop at prose or
/// non-comment text and deduplicate without changing order.
pub fn conflicted_paths(merge_msg: &[u8], comment_char: u8) -> Vec<Vec<u8>> {
    let mut paths: Vec<Vec<u8>> = Vec::new();
    let mut in_block = false;
    for line in merge_msg.split(|&byte| byte == b'\n') {
        let line = strip_cr(line);
        let comment = line.strip_prefix(&[comment_char]);
        if !in_block {
            if comment == Some(b" Conflicts:".as_slice()) {
                in_block = true;
            }
            continue;
        }
        let Some(path) = comment.and_then(|rest| rest.strip_prefix(b"\t")) else {
            break;
        };
        if path.is_empty() {
            break;
        }
        if !paths.iter().any(|known| known == path) {
            paths.push(path.to_vec());
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::{auto_comment_char, conflicted_paths, effective_comment_char, strip_comments};

    #[test]
    fn preserves_path_edge_whitespace() {
        let msg = b"# Conflicts:\r\n#\t leading and trailing \r\n";
        assert_eq!(
            conflicted_paths(msg, b'#'),
            vec![b" leading and trailing ".to_vec()]
        );
    }

    #[test]
    fn preserves_non_utf8_path_bytes() {
        let msg = b"# Conflicts:\n#\tfile-\xff.txt\n#\tvalid.txt\n";
        assert_eq!(
            conflicted_paths(msg, b'#'),
            vec![b"file-\xff.txt".to_vec(), b"valid.txt".to_vec()]
        );
    }

    #[test]
    fn parses_git_block() {
        let msg = b"Merge branch 'topic'\n\n# Conflicts:\n#\tsrc/a.rs\n#\tdocs/b.md\n";
        assert_eq!(
            conflicted_paths(msg, b'#'),
            vec![b"src/a.rs".to_vec(), b"docs/b.md".to_vec()]
        );
    }

    #[test]
    fn parses_semicolon_and_auto_comment_char() {
        let msg = b"# message line\n; Conflicts:\n;\tfile.txt\n";
        assert_eq!(conflicted_paths(msg, b';'), vec![b"file.txt".to_vec()]);
        assert_eq!(auto_comment_char(msg), b';');
        assert_eq!(
            conflicted_paths(msg, auto_comment_char(msg)),
            vec![b"file.txt".to_vec()]
        );
        assert_eq!(auto_comment_char(b"ordinary message\n"), b'#');
        assert_eq!(auto_comment_char(b"# message\n; message\n"), b'@');
        assert_eq!(auto_comment_char(b"# Conflicts:\n#\tfile.txt\n"), b'#');
    }

    #[test]
    fn strips_configured_merge_comments_preserving_user_body() {
        let message = "Merge topic\n\n# User heading\nBody; punctuation\n  ; indented prose\n\n; Conflicts:\n;\tfile.txt\n";
        let expected = "Merge topic\n\n# User heading\nBody; punctuation\n  ; indented prose";
        for configured in [Some(";"), Some("auto")] {
            let comment = effective_comment_char(configured, message.as_bytes());
            assert_eq!(comment, b';');
            assert_eq!(strip_comments(message, comment), expected);
        }
    }

    #[test]
    fn strips_default_and_comment_only_merge_templates() {
        assert_eq!(effective_comment_char(None, b""), b'#');
        assert_eq!(
            strip_comments("Merge topic\n\n# Conflicts:\n#\tfile\n", b'#'),
            "Merge topic"
        );
        assert_eq!(strip_comments("; Conflicts:\n;\tfile\n", b';'), "");
        assert_eq!(
            strip_comments(
                "Merge topic\n\nUser body\n",
                effective_comment_char(Some("auto"), b"Merge topic\n\nUser body\n")
            ),
            "Merge topic\n\nUser body"
        );
    }

    #[test]
    fn stops_at_prose_hint_or_text() {
        let msg = b"# Conflicts:\n#\tone\n# It looks like you may be committing a merge.\n#\ttwo\n";
        assert_eq!(conflicted_paths(msg, b'#'), vec![b"one".to_vec()]);
        let msg = b"# Conflicts:\n#\tone\nbody\n#\ttwo\n";
        assert_eq!(conflicted_paths(msg, b'#'), vec![b"one".to_vec()]);
    }

    #[test]
    fn missing_block_yields_nothing() {
        assert!(conflicted_paths(b"Merge branch 'topic'\n", b'#').is_empty());
        assert!(conflicted_paths(b"", b'#').is_empty());
    }

    #[test]
    fn dedups_and_handles_crlf() {
        let msg = b"# Conflicts:\r\n#\ta b.txt\r\n#\ta b.txt\r\n#    not a path\r\n";
        assert_eq!(conflicted_paths(msg, b'#'), vec![b"a b.txt".to_vec()]);
    }
}
