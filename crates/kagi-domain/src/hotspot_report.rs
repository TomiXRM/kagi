//! "Copy diagnostic" serialization for the Ecosystem view (ADR-0119).
//!
//! Turns a ranked [`Ecosystem`] into LLM-ready text — paste it straight into a
//! chat to prime a model with *where the maintenance risk is* before asking for
//! a refactor / review plan (the Aider repo-map idea, kept in-app). Pure string
//! building; `kagi-domain` has no serde, so JSON is hand-rolled with escaping.

use crate::hotspot::{CouplingPair, Ecosystem, FileOwnership};

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

/// Serialize the change-coupling pairs (Coupling mode) in `format`. `window` is
/// the granularity label; `total` is the full pair count before truncation.
pub fn render_couplings(
    pairs: &[CouplingPair],
    window: &str,
    total: usize,
    format: ReportFormat,
) -> String {
    match format {
        ReportFormat::Markdown => couplings_markdown(pairs, window, total),
        ReportFormat::Json => couplings_json(pairs, window, total),
    }
}

/// Serialize per-file ownership (Ownership mode) in `format`. `window` is the
/// granularity label; `total` is the full file count before truncation.
pub fn render_ownership(
    owns: &[FileOwnership],
    window: &str,
    total: usize,
    format: ReportFormat,
) -> String {
    match format {
        ReportFormat::Markdown => ownership_markdown(owns, window, total),
        ReportFormat::Json => ownership_json(owns, window, total),
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

// ── Coupling (change-coupling pairs) ────────────────────────────────────

fn couplings_markdown(pairs: &[CouplingPair], window: &str, total: usize) -> String {
    let mut out = String::new();
    out.push_str("# Change coupling (files that change together)\n\n");
    out.push_str(&format!(
        "Window: {} · pairs shown: {} of {}\n\n",
        window,
        pairs.len(),
        total,
    ));
    out.push_str("`together` = co-change count · `degree` = Jaccard overlap (1.0 = only ever change together)\n\n");
    out.push_str("| # | file A | file B | together | degree |\n");
    out.push_str("|---|--------|--------|---------:|-------:|\n");
    for (i, p) in pairs.iter().enumerate() {
        out.push_str(&format!(
            "| {} | `{}` | `{}` | {} | {:.2} |\n",
            i + 1,
            p.a,
            p.b,
            p.together,
            p.degree,
        ));
    }
    out
}

fn couplings_json(pairs: &[CouplingPair], window: &str, total: usize) -> String {
    let mut out = String::from("{\n");
    out.push_str(&format!("  \"window\": \"{}\",\n", esc(window)));
    out.push_str(&format!("  \"total_pairs\": {},\n", total));
    out.push_str("  \"couplings\": [\n");
    for (i, p) in pairs.iter().enumerate() {
        let comma = if i + 1 < pairs.len() { "," } else { "" };
        out.push_str(&format!(
            "    {{ \"a\": \"{}\", \"b\": \"{}\", \"together\": {}, \"degree\": {:.4} }}{}\n",
            esc(&p.a),
            esc(&p.b),
            p.together,
            p.degree,
            comma,
        ));
    }
    out.push_str("  ]\n}\n");
    out
}

// ── Ownership (bus-factor) ──────────────────────────────────────────────

fn ownership_markdown(owns: &[FileOwnership], window: &str, total: usize) -> String {
    let mut out = String::new();
    out.push_str("# Ownership (bus-factor)\n\n");
    out.push_str(&format!(
        "Window: {} · files shown: {} of {}\n\n",
        window,
        owns.len(),
        total,
    ));
    out.push_str("`share` = primary author's fraction of commits · `authors` = distinct authors (1 = single-owner risk)\n\n");
    out.push_str("| # | file | primary author | share | authors | commits |\n");
    out.push_str("|---|------|----------------|------:|--------:|--------:|\n");
    for (i, o) in owns.iter().enumerate() {
        out.push_str(&format!(
            "| {} | `{}` | {} | {:.0}% | {} | {} |\n",
            i + 1,
            o.path,
            o.primary_author,
            o.primary_share * 100.0,
            o.authors,
            o.commits,
        ));
    }
    out
}

fn ownership_json(owns: &[FileOwnership], window: &str, total: usize) -> String {
    let mut out = String::from("{\n");
    out.push_str(&format!("  \"window\": \"{}\",\n", esc(window)));
    out.push_str(&format!("  \"total_files\": {},\n", total));
    out.push_str("  \"ownership\": [\n");
    for (i, o) in owns.iter().enumerate() {
        let comma = if i + 1 < owns.len() { "," } else { "" };
        out.push_str(&format!(
            "    {{ \"path\": \"{}\", \"primary_author\": \"{}\", \"primary_share\": {:.4}, \"authors\": {}, \"commits\": {} }}{}\n",
            esc(&o.path),
            esc(&o.primary_author),
            o.primary_share,
            o.authors,
            o.commits,
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
                author: "a@x".into(),
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
                author: "a@x".into(),
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

    #[test]
    fn couplings_markdown_lists_pairs() {
        let pairs = vec![CouplingPair {
            a: "src/a.rs".into(),
            b: "src/b.rs".into(),
            together: 7,
            degree: 0.5,
        }];
        let md = render_couplings(&pairs, "all time", 1, ReportFormat::Markdown);
        assert!(md.contains("# Change coupling"));
        assert!(md.contains("| `src/a.rs` | `src/b.rs` | 7 | 0.50 |"));
    }

    #[test]
    fn couplings_json_is_well_formed() {
        let pairs = vec![CouplingPair {
            a: "src/a.rs".into(),
            b: "src/b.rs".into(),
            together: 7,
            degree: 0.5,
        }];
        let js = render_couplings(&pairs, "all time", 3, ReportFormat::Json);
        assert!(js.contains("\"total_pairs\": 3"));
        assert!(js.contains("\"a\": \"src/a.rs\""));
        assert!(js.contains("\"together\": 7"));
        // Single row → no trailing comma before the closing bracket.
        assert!(!js.contains("},\n  ]"));
        assert!(js.trim_end().ends_with('}'));
    }

    #[test]
    fn ownership_markdown_and_json() {
        let owns = vec![FileOwnership {
            path: "src/a.rs".into(),
            primary_author: "alice@x".into(),
            primary_share: 0.75,
            authors: 2,
            commits: 8,
        }];
        let md = render_ownership(&owns, "all time", 1, ReportFormat::Markdown);
        assert!(md.contains("# Ownership"));
        assert!(md.contains("| `src/a.rs` | alice@x | 75% | 2 | 8 |"));
        let js = render_ownership(&owns, "all time", 1, ReportFormat::Json);
        assert!(js.contains("\"primary_author\": \"alice@x\""));
        assert!(js.contains("\"authors\": 2"));
    }
}
