//! Unit tests for commit-message trailer parsing — W18-COAUTHOR-COPY.
//!
//! Exercises `kagi::git::parse_coauthors`, the pure function that extracts
//! `Co-authored-by:` trailers from a raw commit message.  No filesystem or
//! git access is required — these are pure string tests.

use kagi::git::{CoAuthor, parse_coauthors};

fn ca(name: &str, email: &str) -> CoAuthor {
    CoAuthor {
        name: name.to_string(),
        email: email.to_string(),
    }
}

#[test]
fn no_trailers_returns_empty() {
    let msg = "fix: something\n\njust a body with no trailers";
    assert!(parse_coauthors(msg).is_empty());
}

#[test]
fn single_coauthor() {
    let msg = "feat: thing\n\nbody\n\nCo-authored-by: Alice Example <alice@example.com>";
    assert_eq!(
        parse_coauthors(msg),
        vec![ca("Alice Example", "alice@example.com")]
    );
}

#[test]
fn multiple_coauthors_preserve_order() {
    let msg = "subject\n\n\
        Co-authored-by: Alice <alice@example.com>\n\
        Co-authored-by: Bob <bob@example.com>";
    assert_eq!(
        parse_coauthors(msg),
        vec![
            ca("Alice", "alice@example.com"),
            ca("Bob", "bob@example.com"),
        ]
    );
}

#[test]
fn key_is_case_insensitive() {
    let msg = "subject\n\n\
        co-authored-by: Lower <lower@example.com>\n\
        CO-AUTHORED-BY: Upper <upper@example.com>\n\
        Co-Authored-By: Mixed <mixed@example.com>";
    assert_eq!(
        parse_coauthors(msg),
        vec![
            ca("Lower", "lower@example.com"),
            ca("Upper", "upper@example.com"),
            ca("Mixed", "mixed@example.com"),
        ]
    );
}

#[test]
fn leading_whitespace_tolerated() {
    let msg = "subject\n\n   Co-authored-by: Indented <indent@example.com>";
    assert_eq!(
        parse_coauthors(msg),
        vec![ca("Indented", "indent@example.com")]
    );
}

#[test]
fn name_only_no_email() {
    let msg = "subject\n\nCo-authored-by: NoEmail Person";
    assert_eq!(parse_coauthors(msg), vec![ca("NoEmail Person", "")]);
}

#[test]
fn email_only_no_name() {
    let msg = "subject\n\nCo-authored-by: <onlyemail@example.com>";
    assert_eq!(parse_coauthors(msg), vec![ca("", "onlyemail@example.com")]);
}

#[test]
fn empty_value_is_skipped() {
    let msg = "subject\n\nCo-authored-by:   \nCo-authored-by: Real <real@example.com>";
    assert_eq!(parse_coauthors(msg), vec![ca("Real", "real@example.com")]);
}

#[test]
fn duplicates_are_deduplicated() {
    let msg = "subject\n\n\
        Co-authored-by: Alice <alice@example.com>\n\
        Co-authored-by: Alice <ALICE@example.com>\n\
        Co-authored-by: Alice <alice@example.com>";
    // The first two collapse (email case-insensitive), the third is an exact dup.
    assert_eq!(
        parse_coauthors(msg),
        vec![ca("Alice", "alice@example.com")]
    );
}

#[test]
fn mid_prose_mention_is_not_a_trailer() {
    // The key must be the first non-whitespace token on the line.
    let msg = "subject\n\nThis was Co-authored-by: someone, informally.";
    assert!(parse_coauthors(msg).is_empty());
}

#[test]
fn cjk_name_preserved() {
    let msg = "subject\n\nCo-authored-by: 田中太郎 <tanaka@example.com>";
    assert_eq!(
        parse_coauthors(msg),
        vec![ca("田中太郎", "tanaka@example.com")]
    );
}

#[test]
fn trailers_interspersed_with_other_lines() {
    let msg = "subject\n\n\
        Some body text.\n\
        Co-authored-by: Alice <alice@example.com>\n\
        Signed-off-by: Maintainer <maint@example.com>\n\
        Co-authored-by: Bob <bob@example.com>";
    assert_eq!(
        parse_coauthors(msg),
        vec![
            ca("Alice", "alice@example.com"),
            ca("Bob", "bob@example.com"),
        ]
    );
}
