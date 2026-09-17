use super::*;
#[cfg(test)]
mod comment_tag_tests {
    use super::*;

    /// Codex's real shape: a shields.io image inside nested `<sub>`.
    #[test]
    fn codex_shields_badge_becomes_a_tag_and_leaves_the_prose() {
        let body = "**<sub><sub>![P1 Badge](https://img.shields.io/badge/P1-orange?style=flat)</sub></sub>  Preserve KEEP plates**\n\nWhen an operator…";
        let (tag, rest) = extract_comment_tag(body);
        let tag = tag.expect("tag");
        assert_eq!(tag.label, "P1");
        assert_eq!(tag.severity, TagSeverity::High);
        assert!(!rest.contains("shields.io"), "image removed: {rest}");
        assert!(!rest.contains("<sub>"), "wrapper removed: {rest}");
        assert!(rest.starts_with("Preserve KEEP plates"), "{rest}");
    }

    #[test]
    fn p2_is_medium_and_p3_is_low() {
        let mk =
            |p: &str| format!("![{p} Badge](https://img.shields.io/badge/{p}-yellow?style=flat) x");
        assert_eq!(
            extract_comment_tag(&mk("P2")).0.unwrap().severity,
            TagSeverity::Medium
        );
        assert_eq!(
            extract_comment_tag(&mk("P3")).0.unwrap().severity,
            TagSeverity::Low
        );
    }

    /// With a label `severity_for` does not know, the badge **colour** decides
    /// — the only arm that exercises the colour heuristic at all, since a
    /// known label matches first and returns.
    #[test]
    fn unknown_label_falls_back_to_the_badge_colour() {
        let sev = |url_label: &str, colour: &str| {
            let body = format!("![X Badge](https://img.shields.io/badge/{url_label}-{colour})");
            extract_comment_tag(&body).0.unwrap().severity
        };
        assert_eq!(sev("X", "red"), TagSeverity::High);
        assert_eq!(sev("X", "orange"), TagSeverity::High);
        assert_eq!(sev("X", "critical"), TagSeverity::High);
        assert_eq!(sev("X", "important"), TagSeverity::High);
        assert_eq!(sev("X", "yellow"), TagSeverity::Medium);
        assert_eq!(sev("X", "yellowgreen"), TagSeverity::Medium);
        assert_eq!(sev("X", "green"), TagSeverity::Low);
        assert_eq!(sev("X", "blue"), TagSeverity::Low);
        // The label wins over a contradicting colour.
        assert_eq!(sev("NIT", "red"), TagSeverity::Low);
        assert_eq!(sev("P1", "green"), TagSeverity::High);
        assert_eq!(sev("SHOULD", "green"), TagSeverity::Medium);
        // Colour matching is case-insensitive (`to_ascii_lowercase`).
        assert_eq!(sev("X", "RED"), TagSeverity::High);
    }

    /// Copilot's shape.
    #[test]
    fn copilot_bracket_prefix_becomes_a_tag() {
        let (tag, rest) = extract_comment_tag("[MUST] `f()` changed shape");
        assert_eq!(tag.unwrap().label, "MUST");
        assert_eq!(rest, "`f()` changed shape");
    }

    /// A markdown link or ordinary prose must not be mistaken for a tag.
    #[test]
    fn links_and_prose_are_left_alone() {
        for body in [
            "[see docs](https://x) then fix",
            "plain prose",
            "[a very long bracketed phrase] no",
        ] {
            let (tag, rest) = extract_comment_tag(body);
            assert!(tag.is_none(), "{body} → {tag:?}");
            assert_eq!(rest, body);
        }
    }

    #[test]
    fn has_suggestion_detects_only_real_suggestion_fences() {
        let c = |body: &str| ReviewComment {
            author: "copilot".into(),
            path: "src/lib.rs".into(),
            line: 10,
            start_line: None,
            body: body.into(),
            diff_hunk: String::new(),
            created_at: String::new(),
            in_reply_to: None,
        };
        assert!(c("nit\n\n```suggestion\nlet x = 1;\n```\n").has_suggestion());
        // Indented inside a list item still counts.
        assert!(c("- see below\n  ```suggestion\n  ok\n  ```").has_suggestion());
        // Language-tagged suggestion fences (```suggestion rust) count too.
        assert!(c("```suggestion rust\nok\n```").has_suggestion());
        // Prose and plain code fences do not gate the apply affordance.
        assert!(!c("looks good to me").has_suggestion());
        assert!(!c("```rust\nlet x = 1;\n```").has_suggestion());
        assert!(!c("the word suggestion appears mid-line ```suggestion").has_suggestion());
    }
}
