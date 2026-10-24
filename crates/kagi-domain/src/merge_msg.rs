//! The `# Conflicts:` block Git appends to `MERGE_MSG` when a merge, cherry-pick,
//! revert or sequencer-backed rebase stops on conflicts — the repository's own
//! record of which paths were conflicted when the operation began. Pure parser;
//! the git layer intersects the result with the live index (ADR-0227 sidebar).

/// Paths listed under the `# Conflicts:` heading of a `MERGE_MSG` body.
///
/// Git writes the block as:
///
/// ```text
/// # Conflicts:
/// #	src/a.rs
/// #	docs/b.md
/// ```
///
/// Every `#` line after the heading that starts with a tab is one path (Git
/// writes `#\t<path>`; its prose hints use `# `), until a non-comment line or a
/// comment without the tab ends the block. Paths are returned in file order
/// without duplicates; quoted paths (Git quotes unusual bytes with
/// `core.quotePath`) are returned verbatim, so callers that need exact bytes
/// must compare against the quoted form too.
pub fn conflicted_paths(merge_msg: &str) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    let mut in_block = false;
    for line in merge_msg.lines() {
        let trimmed = line.trim_end_matches('\r');
        if !in_block {
            if trimmed.trim() == "# Conflicts:" {
                in_block = true;
            }
            continue;
        }
        let Some(path) = trimmed
            .strip_prefix('#')
            .and_then(|rest| rest.strip_prefix('\t'))
        else {
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
    use super::conflicted_paths;

    #[test]
    fn preserves_path_edge_whitespace() {
        let msg = "# Conflicts:\r\n#\t leading and trailing \r\n";
        assert_eq!(conflicted_paths(msg), vec![" leading and trailing "]);
    }

    #[test]
    fn parses_git_block() {
        let msg = "Merge branch 'topic'\n\n# Conflicts:\n#\tsrc/a.rs\n#\tdocs/b.md\n";
        assert_eq!(conflicted_paths(msg), vec!["src/a.rs", "docs/b.md"]);
    }

    #[test]
    fn stops_at_prose_hint_or_text() {
        let msg = "# Conflicts:\n#\tone\n# It looks like you may be committing a merge.\n#\ttwo\n";
        assert_eq!(conflicted_paths(msg), vec!["one"]);
        let msg = "# Conflicts:\n#\tone\nbody\n#\ttwo\n";
        assert_eq!(conflicted_paths(msg), vec!["one"]);
    }

    #[test]
    fn missing_block_yields_nothing() {
        assert!(conflicted_paths("Merge branch 'topic'\n").is_empty());
        assert!(conflicted_paths("").is_empty());
    }

    #[test]
    fn dedups_and_handles_crlf() {
        let msg = "# Conflicts:\r\n#\ta b.txt\r\n#\ta b.txt\r\n#    not a path\r\n";
        assert_eq!(conflicted_paths(msg), vec!["a b.txt"]);
    }
}
