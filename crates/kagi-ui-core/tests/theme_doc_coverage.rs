//! `docs/themes.md` is the public custom-theme spec (#922, ADR-0219). These
//! tests keep it honest against the real `Theme`:
//!
//! * the token table lists exactly the keys `Theme` serializes to (with
//!   `syntax.*` nested), so a new field fails here until it is documented;
//! * both JSON examples load through the real custom-theme loader and produce
//!   the built-in values the doc claims they use.
//!
//! Its own test binary, so `KAGI_LOG_DIR` (process-global) is touched by a
//! single test only.

use std::collections::BTreeSet;

use kagi_ui_core::theme::{self, THEMES};
use serde_json::{Map, Value};

const DOC: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/themes.md"));

/// Every token the docs table must list: each top-level `Theme` key, with
/// `syntax` expanded into `syntax.<key>` (the one nested palette the file
/// format spells out key by key). Every other value — including an object
/// such as `term_selection` — is one token row.
fn token_paths(theme: &Map<String, Value>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (key, value) in theme {
        match (key.as_str(), value) {
            ("syntax", Value::Object(syntax)) => {
                out.extend(syntax.keys().map(|k| format!("syntax.{k}")));
            }
            ("syntax", other) => panic!("syntax must serialize to an object, got {other}"),
            _ => {
                out.insert(key.clone());
            }
        }
    }
    out
}

fn serialized(theme: &theme::Theme) -> Map<String, Value> {
    match serde_json::to_value(theme).expect("Theme serializes") {
        Value::Object(map) => map,
        other => panic!("Theme must serialize to an object, got {other}"),
    }
}

fn theme_key_paths() -> BTreeSet<String> {
    token_paths(&serialized(&THEMES[0]))
}

/// Token names from the first column of the table between the
/// `theme-tokens` markers.
fn documented_tokens() -> Vec<String> {
    let start = DOC
        .find("<!-- theme-tokens:start -->")
        .expect("token table start marker");
    let end = DOC
        .find("<!-- theme-tokens:end -->")
        .expect("token table end marker");
    DOC[start..end]
        .lines()
        .filter_map(|line| {
            let cell = line.strip_prefix('|')?.split('|').next()?.trim();
            let token = cell.strip_prefix('`')?.strip_suffix('`')?;
            Some(token.to_string())
        })
        .collect()
}

/// The ```json block following `<!-- example:<name> -->`, verbatim.
fn example_text(name: &str) -> &'static str {
    let marker = format!("<!-- example:{name} -->");
    let at = DOC
        .find(&marker)
        .unwrap_or_else(|| panic!("{marker} missing"));
    let rest = &DOC[at + marker.len()..];
    let open = rest.find("```json\n").expect("json fence after marker") + "```json\n".len();
    let close = open + rest[open..].find("```").expect("closing fence");
    &rest[open..close]
}

fn example(name: &str) -> Map<String, Value> {
    match serde_json::from_str(example_text(name)) {
        Ok(Value::Object(map)) => map,
        other => panic!("example {name} is not a JSON object: {other:?}"),
    }
}

fn keys(map: &Map<String, Value>) -> BTreeSet<String> {
    map.keys().cloned().collect()
}

#[test]
fn token_table_matches_theme_fields_exactly() {
    let documented = documented_tokens();
    let mut seen = BTreeSet::new();
    let duplicates: Vec<_> = documented
        .iter()
        .filter(|t| !seen.insert(t.as_str()))
        .collect();
    assert!(
        duplicates.is_empty(),
        "duplicate token rows: {duplicates:?}"
    );

    let fields = theme_key_paths();
    let documented: BTreeSet<String> = documented.into_iter().collect();
    let undocumented: Vec<_> = fields.difference(&documented).collect();
    let stale: Vec<_> = documented.difference(&fields).collect();
    assert!(
        undocumented.is_empty() && stale.is_empty(),
        "docs/themes.md token table is out of sync with Theme.\n\
         missing rows: {undocumented:?}\nrows for no field: {stale:?}"
    );
}

#[test]
fn complete_example_spells_out_every_field() {
    let complete = example("complete");
    let builtin = serialized(&THEMES[0]);
    assert!(!complete.contains_key("extends"));
    assert_eq!(keys(&complete), keys(&builtin), "top-level keys");

    let syntax = complete["syntax"].as_object().expect("syntax object");
    let builtin_syntax = builtin["syntax"].as_object().expect("syntax object");
    assert_eq!(keys(syntax), keys(builtin_syntax), "syntax keys");

    let selection = complete["term_selection"]
        .as_object()
        .expect("term_selection object");
    assert_eq!(
        keys(selection),
        BTreeSet::from(["alpha".to_string(), "color".to_string()])
    );
}

#[test]
fn examples_load_with_the_documented_builtin_values() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::env::set_var("KAGI_LOG_DIR", dir.path());
    let themes_dir = theme::themes_dir().expect("themes dir");
    assert_eq!(
        themes_dir,
        dir.path().join("themes"),
        "folder the doc names"
    );
    std::fs::create_dir_all(&themes_dir).expect("create themes dir");

    let extends = example("extends");
    let complete = example("complete");
    // The doc's bytes, not a re-encoding: what a user copies is what loads.
    for name in ["extends", "complete"] {
        std::fs::write(themes_dir.join(format!("{name}.json")), example_text(name))
            .expect("write example");
    }

    let errors = theme::reload_custom_themes();
    let reasons: Vec<_> = errors
        .iter()
        .map(|e| format!("{}: {}", e.path.display(), e.reason))
        .collect();
    assert!(reasons.is_empty(), "examples rejected: {reasons:?}");

    let builtin = |slug: &str| {
        let t = THEMES
            .iter()
            .find(|t| t.slug == slug)
            .unwrap_or_else(|| panic!("built-in {slug}"));
        serialized(t)
    };
    let loaded = |json: &Map<String, Value>| {
        let slug = json["slug"].as_str().expect("slug");
        let handle = theme::theme_by_slug(slug).unwrap_or_else(|| panic!("{slug} not loaded"));
        let t: &theme::Theme = &handle;
        serialized(t)
    };

    // Complete example: the default theme, field for field, under its own
    // slug and name.
    let mut expected = builtin(&*THEMES[0].slug);
    expected.insert("slug".into(), complete["slug"].clone());
    expected.insert("name".into(), complete["name"].clone());
    assert_eq!(loaded(&complete), expected, "complete example != THEMES[0]");

    // Extends example: its parent, with the overridden colours taken from
    // the built-in the doc names (Tokyo Night).
    let parent = extends["extends"].as_str().expect("extends slug");
    let donor = builtin("tokyo-night");
    let mut expected = builtin(parent);
    expected.insert("slug".into(), extends["slug"].clone());
    expected.insert("name".into(), extends["name"].clone());
    let overridden: Vec<&String> = extends
        .keys()
        .filter(|k| !matches!(k.as_str(), "slug" | "name" | "extends"))
        .collect();
    assert!(!overridden.is_empty());
    for key in overridden {
        expected.insert(key.clone(), donor[key.as_str()].clone());
    }
    assert_eq!(loaded(&extends), expected, "extends example");

    std::env::remove_var("KAGI_LOG_DIR");
}
