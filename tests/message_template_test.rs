//! Integration tests for the structured commit-message template assembly /
//! parsing pure functions — T-COMMIT-009 (lane W14-TEMPLATE).
//!
//! These exercise `kagi_git::message_template` end to end: assembling the six
//! fields into one message (empty fields omitted), parsing a plain message back
//! into best-effort structured fields, and the plain ⇄ template round-trip the
//! Commit Panel relies on to not lose user input on a mode toggle.

use kagi_git::message_template::{assemble, parse_message, TemplateFields, TYPE_CHOICES};

// ── assemble: full + partial ────────────────────────────────────

#[test]
fn assemble_full_message() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = TemplateFields::new(
        "feat",
        "commit-panel",
        "add template mode",
        "Adds a structured editor with six fields.\nSecond paragraph.",
        "cargo test green",
        "low",
    );
    let out = assemble(&f);
    assert_eq!(
        out,
        "feat(commit-panel): add template mode\n\n\
         Adds a structured editor with six fields.\nSecond paragraph.\n\n\
         Test: cargo test green\nRisk: low"
    );
}

#[test]
fn assemble_subject_only() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = TemplateFields::new("fix", "", "correct off-by-one", "", "", "");
    assert_eq!(assemble(&f), "fix: correct off-by-one");
}

#[test]
fn assemble_summary_only_no_type() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = TemplateFields::new("", "", "just a plain summary", "", "", "");
    assert_eq!(assemble(&f), "just a plain summary");
}

#[test]
fn assemble_drops_scope_without_type() {
    if !crate::test_support::run_isolated() {
        return;
    }
    // A bare "(scope):" prefix is not valid CC — scope is dropped without a type.
    let f = TemplateFields::new("", "ui", "tweak", "", "", "");
    assert_eq!(assemble(&f), "tweak");
}

#[test]
fn assemble_omits_empty_blocks() {
    if !crate::test_support::run_isolated() {
        return;
    }
    // No body, but Test/Risk present → body block must be skipped, not blank.
    let f = TemplateFields::new("docs", "", "update readme", "", "manual", "none");
    assert_eq!(
        assemble(&f),
        "docs: update readme\n\nTest: manual\nRisk: none"
    );
}

#[test]
fn assemble_body_only_no_test_risk() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = TemplateFields::new("feat", "", "x", "Body text here.", "", "");
    assert_eq!(assemble(&f), "feat: x\n\nBody text here.");
}

#[test]
fn assemble_only_test_trailer() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = TemplateFields::new("", "", "", "", "ran it", "");
    assert_eq!(assemble(&f), "Test: ran it");
}

#[test]
fn assemble_empty_fields_yield_empty() {
    if !crate::test_support::run_isolated() {
        return;
    }
    assert_eq!(assemble(&TemplateFields::default()), "");
}

#[test]
fn assemble_trims_whitespace() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = TemplateFields::new("  feat  ", "  api ", "  do thing ", " body ", " t ", " r ");
    assert_eq!(
        assemble(&f),
        "feat(api): do thing\n\nbody\n\nTest: t\nRisk: r"
    );
}

#[test]
fn assemble_type_without_summary_keeps_colon() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = TemplateFields::new("chore", "", "", "", "", "");
    assert_eq!(assemble(&f), "chore:");
    let f2 = TemplateFields::new("chore", "deps", "", "", "", "");
    assert_eq!(assemble(&f2), "chore(deps):");
}

// ── parse: structured subjects ──────────────────────────────────

#[test]
fn parse_type_scope_summary() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = parse_message("feat(commit-panel): add template mode");
    assert_eq!(f.r#type, "feat");
    assert_eq!(f.scope, "commit-panel");
    assert_eq!(f.summary, "add template mode");
    assert_eq!(f.body, "");
}

#[test]
fn parse_type_summary_no_scope() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = parse_message("fix: bug");
    assert_eq!(f.r#type, "fix");
    assert_eq!(f.scope, "");
    assert_eq!(f.summary, "bug");
}

#[test]
fn parse_subject_with_body() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = parse_message("feat(x): do it\n\nThe body.\nMore body.");
    assert_eq!(f.r#type, "feat");
    assert_eq!(f.scope, "x");
    assert_eq!(f.summary, "do it");
    assert_eq!(f.body, "The body.\nMore body.");
}

#[test]
fn parse_non_conventional_goes_to_summary() {
    if !crate::test_support::run_isolated() {
        return;
    }
    // No "type:" prefix → whole message in summary (lossless for round-trip).
    let f = parse_message("Merge branch 'main' into feature");
    assert_eq!(f.r#type, "");
    assert_eq!(f.scope, "");
    assert_eq!(f.summary, "Merge branch 'main' into feature");
    assert_eq!(f.body, "");
}

#[test]
fn parse_prose_with_colon_is_not_a_type() {
    if !crate::test_support::run_isolated() {
        return;
    }
    // "Note" alone before ':' is a single word → treated as a type here, which
    // is acceptable; but multi-word heads must NOT be parsed as a type.
    let f = parse_message("See also: the other thing");
    assert_eq!(f.r#type, "");
    assert_eq!(f.summary, "See also: the other thing");
}

#[test]
fn parse_empty_message_is_default() {
    if !crate::test_support::run_isolated() {
        return;
    }
    assert_eq!(parse_message(""), TemplateFields::default());
    assert_eq!(parse_message("   \n  "), TemplateFields::default());
}

#[test]
fn parse_multiline_no_blank_keeps_only_first_as_subject() {
    if !crate::test_support::run_isolated() {
        return;
    }
    // Second line directly after subject (no blank line) still becomes body.
    let f = parse_message("fix: a\nb");
    assert_eq!(f.r#type, "fix");
    assert_eq!(f.summary, "a");
    assert_eq!(f.body, "b");
}

// ── round-trip: template → plain → template ─────────────────────

#[test]
fn round_trip_full_message() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = TemplateFields::new("feat", "core", "ship it", "Body paragraph.", "", "");
    let plain = assemble(&f);
    let back = parse_message(&plain);
    assert_eq!(back.r#type, "feat");
    assert_eq!(back.scope, "core");
    assert_eq!(back.summary, "ship it");
    assert_eq!(back.body, "Body paragraph.");
}

#[test]
fn round_trip_non_conventional_is_lossless() {
    if !crate::test_support::run_isolated() {
        return;
    }
    // plain → template → plain must reproduce arbitrary text exactly.
    let original = "Random message\nwith a second line.";
    let fields = parse_message(original);
    // Non-CC text lands entirely in summary (whole message, all lines).
    assert_eq!(fields.summary, original);
    assert_eq!(assemble(&fields), original);
    // For a true lossless plain→template→plain check, a message that does not
    // start with a CC subject keeps the whole thing in summary:
    let single = "totally freeform";
    assert_eq!(assemble(&parse_message(single)), single);
}

// ── type choices sanity ─────────────────────────────────────────

#[test]
fn type_choices_present() {
    if !crate::test_support::run_isolated() {
        return;
    }
    assert!(TYPE_CHOICES.contains(&"feat"));
    assert!(TYPE_CHOICES.contains(&"fix"));
    assert!(!TYPE_CHOICES.is_empty());
}

#[path = "support/isolated.rs"]
mod test_support;
