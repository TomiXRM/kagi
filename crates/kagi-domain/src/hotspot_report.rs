//! "Copy diagnostic" serialization for the Ecosystem view (ADR-0119).
//!
//! Turns a ranked [`Ecosystem`] into LLM-ready text — paste it straight into a
//! chat to prime a model with *where the maintenance risk is* before asking for
//! a refactor / review plan (the Aider repo-map idea, kept in-app). Pure string
//! building; `kagi-domain` has no serde, so JSON is hand-rolled with escaping.

use crate::hotspot::Ecosystem;

/// Diagnostic output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Markdown,
    Json,
}

/// Serialize the top-`n` hot-spots of `eco` in `format`.
pub fn render(eco: &Ecosystem, n: usize, format: ReportFormat) -> String {
    match format {
        ReportFormat::Markdown => markdown(eco, n),
        ReportFormat::Json => json(eco, n),
    }
}

fn markdown(eco: &Ecosystem, n: usize) -> String {
    let mut out = String::new();
    out.push_str("# Code hot-spots (churn × complexity)\n\n");
    out.push_str(&format!(
        "Window: {} · files shown: {} of {}\n\n",
        eco.granularity.window_label(),
        eco.files.len().min(n),
        eco.files.len(),
    ));
    out.push_str("| # | file | commits | LOC | risk |\n");
    out.push_str("|---|------|--------:|----:|-----:|\n");
    for (i, f) in eco.files.iter().take(n).enumerate() {
        out.push_str(&format!(
            "| {} | `{}` | {} | {} | {:.2} |\n",
            i + 1,
            f.path,
            f.commits,
            f.loc,
            f.risk,
        ));
    }
    out
}

fn json(eco: &Ecosystem, n: usize) -> String {
    let mut out = String::from("{\n");
    out.push_str(&format!(
        "  \"window\": \"{}\",\n",
        esc(eco.granularity.window_label())
    ));
    out.push_str(&format!("  \"total_files\": {},\n", eco.files.len()));
    out.push_str("  \"hotspots\": [\n");
    let shown = eco.files.len().min(n);
    for (i, f) in eco.files.iter().take(n).enumerate() {
        let comma = if i + 1 < shown { "," } else { "" };
        out.push_str(&format!(
            "    {{ \"path\": \"{}\", \"commits\": {}, \"loc\": {}, \"insertions\": {}, \"deletions\": {}, \"risk\": {:.4} }}{}\n",
            esc(&f.path),
            f.commits,
            f.loc,
            f.insertions,
            f.deletions,
            f.risk,
            comma,
        ));
    }
    out.push_str("  ]\n}\n");
    out
}

/// Minimal JSON string escaping (the chars that would break a `"…"` literal).
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

// ────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::Granularity;
    use crate::hotspot::{analyze, CommitChanges, FileChange, RawEcosystem};
    use std::collections::BTreeMap;

    const NOW: i64 = 1_700_000_000;

    fn sample() -> Ecosystem {
        let mut loc = BTreeMap::new();
        loc.insert("src/a.rs".to_string(), 100);
        loc.insert("src/b.rs".to_string(), 20);
        let commits = vec![
            CommitChanges {
                time: NOW - 100,
                files: vec![
                    FileChange {
                        path: "src/a.rs".into(),
                        insertions: 5,
                        deletions: 2,
                    },
                    FileChange {
                        path: "src/b.rs".into(),
                        insertions: 1,
                        deletions: 0,
                    },
                ],
            },
            CommitChanges {
                time: NOW - 200,
                files: vec![FileChange {
                    path: "src/a.rs".into(),
                    insertions: 3,
                    deletions: 1,
                }],
            },
        ];
        analyze(&RawEcosystem { commits, loc }, NOW, Granularity::All)
    }

    #[test]
    fn markdown_has_header_and_top_file() {
        let md = render(&sample(), 10, ReportFormat::Markdown);
        assert!(md.contains("# Code hot-spots"));
        assert!(md.contains("| 1 | `src/a.rs` |"));
        assert!(md.contains("all time"));
    }

    #[test]
    fn json_is_well_formed_and_respects_limit() {
        let js = render(&sample(), 1, ReportFormat::Json);
        assert!(js.contains("\"total_files\": 2"));
        assert!(js.contains("\"path\": \"src/a.rs\""));
        // Only one hotspot row → no trailing comma before the closing bracket.
        assert!(!js.contains("},\n  ]"));
        assert!(js.trim_end().ends_with('}'));
    }

    #[test]
    fn escapes_quotes_in_paths() {
        assert_eq!(esc(r#"a"b\c"#), r#"a\"b\\c"#);
    }
}
