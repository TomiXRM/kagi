//! Behavioral hot-spot analysis — pure Rust, no deps (ADR-0119).
//!
//! A *hot-spot* is a file that concentrates both **change** and **complexity**:
//! the small fraction of a codebase where most maintenance happens and where
//! defects are therefore most likely (Tornhill, *Your Code as a Crime Scene*;
//! Nagappan & Ball 2005). The risk score is deliberately simple and
//! interpretable —
//!
//! ```text
//! risk(file) = normalize(churn) × normalize(complexity)
//! ```
//!
//! - **churn** = number of commits in the selected window that touched the file.
//! - **complexity** = current line count (a cheap, language-independent proxy).
//!
//! There is **no bug-fix time-decay term** (the retired Google 2011 score): it
//! mis-flags healthy high-churn / refactor files and, in an AI-assisted
//! workflow, rapid auto-fix churn pollutes it further. Output is framed as
//! *attention*, never a verdict.
//!
//! This module is the pure core of the Ecosystem view: the [`RawEcosystem`]
//! input is produced by the `kagi-git` layer (`git log --numstat` + a LOC
//! scan); everything here is window-free unit-testable.

use crate::activity::Granularity;
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// One file's change within a single commit (one `--numstat` row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub insertions: u64,
    pub deletions: u64,
}

/// Extensions excluded from hot-spot analysis: binary / non-source artifacts
/// (PDFs, raster & vector images, CAD / 3D models) where "churn × line count"
/// is meaningless. Lowercased, without the leading dot. KiCad project files
/// (`*.kicad_pcb`, `*.kicad_sch`, …) are matched separately by extension prefix.
const EXCLUDED_EXTENSIONS: &[&str] = &[
    "pdf", // documents
    // raster / vector image data
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "ico", "icns", "tif", "tiff", "svg", "heic", "heif",
    "avif", "psd", "ai", "eps", // CAD / 3D models
    "step", "stp", "stl", "iges", "igs", "3mf", // archives / binary bundles
    "zip", // fonts
    "ttf", "otf", "ttc", "woff", "woff2", "eot",
];

/// File **base names** excluded regardless of extension — generated caches and
/// the like that have no useful extension (e.g. KiCad's `fp-info-cache`).
const EXCLUDED_FILENAMES: &[&str] = &["fp-info-cache"];

/// Lowercased file extension (text after the last `.`), or `None` when the path
/// has no extension.
pub fn ext_lower(path: &str) -> Option<String> {
    match path.rsplit_once('.') {
        Some((_, e)) if !e.is_empty() => Some(e.to_ascii_lowercase()),
        _ => None,
    }
}

/// True when `path` is an excluded binary / non-source artifact, judged by its
/// extension (case-insensitive) against the built-in defaults. KiCad files are
/// matched by the `kicad` extension prefix (`kicad_pcb`, `kicad_sch`, …). The
/// `kagi-git` mining layer additionally honours a user-configured list.
pub fn is_excluded(path: &str) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path);
    if EXCLUDED_FILENAMES.contains(&base) {
        return true;
    }
    match ext_lower(path) {
        Some(ext) => ext.starts_with("kicad") || EXCLUDED_EXTENSIONS.contains(&ext.as_str()),
        None => false,
    }
}

/// One commit's changed-file set, tagged with its author time (epoch secs) and
/// author identity (email — the stable key, like [`crate::activity`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitChanges {
    pub time: i64,
    pub author: String,
    pub files: Vec<FileChange>,
}

/// The raw mined history the `kagi-git` layer hands to [`analyze`]: every
/// commit's changed files, plus the current working-tree line count per path
/// (the complexity proxy). Pure data — no git2, no I/O.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawEcosystem {
    pub commits: Vec<CommitChanges>,
    pub loc: BTreeMap<String, u32>,
}

/// Which evaluation axis the view is showing. Hotspots is the MVP; the others
/// have stub panels (ADR-0119) until their data/paint land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcosystemMode {
    Hotspots,
    Coupling,
    Ownership,
}

impl EcosystemMode {
    /// All variants in toggle order.
    pub const ALL: [EcosystemMode; 3] = [
        EcosystemMode::Hotspots,
        EcosystemMode::Coupling,
        EcosystemMode::Ownership,
    ];

    /// Toggle button label.
    pub fn label(self) -> &'static str {
        match self {
            EcosystemMode::Hotspots => "Hotspots",
            EcosystemMode::Coupling => "Coupling",
            EcosystemMode::Ownership => "Ownership",
        }
    }
}

/// A ranked file in the Hotspots view.
#[derive(Debug, Clone, PartialEq)]
pub struct FileMetric {
    pub path: String,
    /// Commits in-window that touched this file (the churn axis).
    pub commits: u32,
    pub insertions: u64,
    pub deletions: u64,
    /// Current line count (the complexity proxy).
    pub loc: u32,
    /// `normalize(churn) × normalize(complexity)`, in `[0, 1]`.
    pub risk: f64,
}

/// A change-coupling partner of some focus file: how often they co-change and
/// the conditional probability `P(partner changed | focus changed)`.
#[derive(Debug, Clone, PartialEq)]
pub struct CouplingEdge {
    pub partner: String,
    pub together: u32,
    /// `together / (commits touching the focus file)`, in `[0, 1]`.
    pub ratio: f64,
}

/// The analysed ecosystem for one window: files ranked by risk, descending.
#[derive(Debug, Clone, PartialEq)]
pub struct Ecosystem {
    pub files: Vec<FileMetric>,
    pub granularity: Granularity,
    /// Window `[window_start, now]` (epoch secs) the analysis covers.
    pub window_start: i64,
    pub now: i64,
}

/// Fixed window length in seconds; `None` for `All` (data-driven, like
/// [`crate::activity`]).
fn window_secs(g: Granularity) -> Option<i64> {
    Some(match g {
        Granularity::Day => 86_400,
        Granularity::Week => 7 * 86_400,
        Granularity::Month => 30 * 86_400,
        Granularity::Year => 365 * 86_400,
        Granularity::All => return None,
    })
}

/// Left edge of the window: `now − window`, or the earliest commit for `All`.
fn window_start(raw: &RawEcosystem, now: i64, g: Granularity) -> i64 {
    match window_secs(g) {
        Some(w) => now - w,
        None => raw
            .commits
            .iter()
            .map(|c| c.time)
            .filter(|&t| t <= now)
            .min()
            .unwrap_or(now),
    }
}

/// Rank files by `churn × complexity` over the granularity window ending at
/// `now`. Files with zero in-window churn are omitted (nothing to attend to).
pub fn analyze(raw: &RawEcosystem, now: i64, g: Granularity) -> Ecosystem {
    let start = window_start(raw, now, g);

    // Accumulate (commits, insertions, deletions) per path within the window.
    let mut by_path: BTreeMap<&str, (u32, u64, u64)> = BTreeMap::new();
    for c in &raw.commits {
        if c.time < start || c.time > now {
            continue;
        }
        for f in &c.files {
            if is_excluded(&f.path) {
                continue;
            }
            let e = by_path.entry(f.path.as_str()).or_default();
            e.0 += 1;
            e.1 += f.insertions;
            e.2 += f.deletions;
        }
    }

    let mut files: Vec<FileMetric> = by_path
        .into_iter()
        .map(|(path, (commits, ins, del))| FileMetric {
            loc: raw.loc.get(path).copied().unwrap_or(0),
            path: path.to_string(),
            commits,
            insertions: ins,
            deletions: del,
            risk: 0.0,
        })
        .collect();

    // Normalize each axis by its in-window max, then multiply.
    let max_churn = files.iter().map(|f| f.commits).max().unwrap_or(0).max(1) as f64;
    let max_loc = files.iter().map(|f| f.loc).max().unwrap_or(0).max(1) as f64;
    for f in &mut files {
        f.risk = (f.commits as f64 / max_churn) * (f.loc as f64 / max_loc);
    }

    files.sort_by(|a, b| {
        b.risk
            .partial_cmp(&a.risk)
            .unwrap_or(Ordering::Equal)
            .then(b.commits.cmp(&a.commits))
            .then(a.path.cmp(&b.path))
    });

    Ecosystem {
        files,
        granularity: g,
        window_start: start,
        now,
    }
}

/// Top-`n` change-coupling partners of `path` over the same window: files that
/// most often change in the same commit as `path`, ranked by co-change count.
pub fn coupling_for(
    raw: &RawEcosystem,
    path: &str,
    now: i64,
    g: Granularity,
    n: usize,
) -> Vec<CouplingEdge> {
    if is_excluded(path) {
        return Vec::new();
    }
    let start = window_start(raw, now, g);
    let mut own = 0u32;
    let mut partners: BTreeMap<&str, u32> = BTreeMap::new();
    for c in &raw.commits {
        if c.time < start || c.time > now {
            continue;
        }
        if !c.files.iter().any(|f| f.path == path) {
            continue;
        }
        own += 1;
        for f in &c.files {
            if f.path != path && !is_excluded(&f.path) {
                *partners.entry(f.path.as_str()).or_default() += 1;
            }
        }
    }

    let denom = own.max(1) as f64;
    let mut edges: Vec<CouplingEdge> = partners
        .into_iter()
        .map(|(partner, together)| CouplingEdge {
            partner: partner.to_string(),
            together,
            ratio: together as f64 / denom,
        })
        .collect();
    edges.sort_by(|a, b| b.together.cmp(&a.together).then(a.partner.cmp(&b.partner)));
    edges.truncate(n);
    edges
}

/// A repository-wide change-coupling pair: two files that tend to change in the
/// same commit (Gall et al. 1998 logical coupling).
#[derive(Debug, Clone, PartialEq)]
pub struct CouplingPair {
    pub a: String,
    pub b: String,
    pub together: u32,
    /// Jaccard overlap `together / (changes_a + changes_b − together)`, in
    /// `(0, 1]` — 1.0 means the two files *only ever* change together.
    pub degree: f64,
}

/// Commits touching more files than this are skipped for pair enumeration:
/// sweeping refactors / vendored drops would dominate the O(k²) pairing without
/// signalling real coupling (they still count toward per-file change totals).
const COUPLING_MAX_FILES_PER_COMMIT: usize = 40;

/// Top-`n` change-coupling pairs over the granularity window, ranked by
/// co-change count then Jaccard degree. Excluded artifacts are ignored.
pub fn top_couplings(raw: &RawEcosystem, now: i64, g: Granularity, n: usize) -> Vec<CouplingPair> {
    let start = window_start(raw, now, g);
    let mut changes: BTreeMap<&str, u32> = BTreeMap::new();
    let mut together: BTreeMap<(&str, &str), u32> = BTreeMap::new();

    for c in &raw.commits {
        if c.time < start || c.time > now {
            continue;
        }
        let files: Vec<&str> = c
            .files
            .iter()
            .map(|f| f.path.as_str())
            .filter(|p| !is_excluded(p))
            .collect();
        for f in &files {
            *changes.entry(f).or_default() += 1;
        }
        if files.len() > COUPLING_MAX_FILES_PER_COMMIT {
            continue; // counted toward totals, but not paired
        }
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                let (a, b) = if files[i] <= files[j] {
                    (files[i], files[j])
                } else {
                    (files[j], files[i])
                };
                *together.entry((a, b)).or_default() += 1;
            }
        }
    }

    let mut pairs: Vec<CouplingPair> = together
        .into_iter()
        .map(|((a, b), t)| {
            let ca = changes.get(a).copied().unwrap_or(0);
            let cb = changes.get(b).copied().unwrap_or(0);
            let denom = (ca + cb).saturating_sub(t).max(1) as f64;
            CouplingPair {
                a: a.to_string(),
                b: b.to_string(),
                together: t,
                degree: t as f64 / denom,
            }
        })
        .collect();
    pairs.sort_by(|x, y| {
        y.together
            .cmp(&x.together)
            .then(y.degree.partial_cmp(&x.degree).unwrap_or(Ordering::Equal))
            .then(x.a.cmp(&y.a))
            .then(x.b.cmp(&y.b))
    });
    pairs.truncate(n);
    pairs
}

/// Per-file authorship for the Ownership mode: who touches a file most, their
/// share, and how many distinct authors have touched it (the bus-factor
/// signal — `authors == 1` is a single-owner risk).
#[derive(Debug, Clone, PartialEq)]
pub struct FileOwnership {
    pub path: String,
    pub primary_author: String,
    /// Primary author's share of in-window commits to this file, in `(0, 1]`.
    pub primary_share: f64,
    pub authors: u32,
    /// Total in-window commits touching this file.
    pub commits: u32,
}

/// Per-file ownership over the granularity window, ranked single-owner /
/// highest-share first (the files most exposed to a bus-factor of one).
/// Excluded artifacts are ignored.
pub fn ownership(raw: &RawEcosystem, now: i64, g: Granularity, n: usize) -> Vec<FileOwnership> {
    let start = window_start(raw, now, g);
    // path → (author → commits)
    let mut by_path: BTreeMap<&str, BTreeMap<&str, u32>> = BTreeMap::new();
    for c in &raw.commits {
        if c.time < start || c.time > now {
            continue;
        }
        for f in &c.files {
            if is_excluded(&f.path) {
                continue;
            }
            *by_path
                .entry(f.path.as_str())
                .or_default()
                .entry(c.author.as_str())
                .or_default() += 1;
        }
    }

    let mut owners: Vec<FileOwnership> = by_path
        .into_iter()
        .map(|(path, authors)| {
            let total: u32 = authors.values().sum();
            let (primary, top) = authors
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
                .map(|(a, c)| (a.to_string(), *c))
                .unwrap_or_default();
            FileOwnership {
                path: path.to_string(),
                primary_author: primary,
                primary_share: top as f64 / total.max(1) as f64,
                authors: authors.len() as u32,
                commits: total,
            }
        })
        .collect();
    // Most-changed files first (the ones that matter), so the real authorship
    // mix is visible up top instead of a wall of single-author files; ties go
    // to fewer authors (the bus-factor risk) then highest share, then path.
    owners.sort_by(|a, b| {
        b.commits
            .cmp(&a.commits)
            .then(a.authors.cmp(&b.authors))
            .then(
                b.primary_share
                    .partial_cmp(&a.primary_share)
                    .unwrap_or(Ordering::Equal),
            )
            .then(a.path.cmp(&b.path))
    });
    owners.truncate(n);
    owners
}

// ────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000;

    fn fc(path: &str) -> FileChange {
        FileChange {
            path: path.into(),
            insertions: 1,
            deletions: 0,
        }
    }

    fn commit(time: i64, paths: &[&str]) -> CommitChanges {
        commit_by(time, "a@x", paths)
    }

    fn commit_by(time: i64, author: &str, paths: &[&str]) -> CommitChanges {
        CommitChanges {
            time,
            author: author.into(),
            files: paths.iter().map(|p| fc(p)).collect(),
        }
    }

    fn raw(commits: Vec<CommitChanges>, loc: &[(&str, u32)]) -> RawEcosystem {
        RawEcosystem {
            commits,
            loc: loc.iter().map(|(p, n)| (p.to_string(), *n)).collect(),
        }
    }

    #[test]
    fn ranks_by_churn_times_complexity() {
        // hot: high churn (3) + high loc (1000). big: high loc but churn 1.
        // busy: high churn (3) but tiny loc (10).
        let r = raw(
            vec![
                commit(NOW - 100, &["hot", "busy"]),
                commit(NOW - 200, &["hot", "busy"]),
                commit(NOW - 300, &["hot", "busy"]),
                commit(NOW - 400, &["big"]),
            ],
            &[("hot", 1000), ("busy", 10), ("big", 1000)],
        );
        let eco = analyze(&r, NOW, Granularity::All);
        assert_eq!(eco.files[0].path, "hot"); // wins on both axes
        assert_eq!(eco.files[0].commits, 3);
        assert!(eco.files[0].risk > eco.files[1].risk);
        // "big" (loc 1000, churn 1) and "busy" (loc 10, churn 3) both rank
        // below "hot"; neither dominates the other trivially but both < hot.
        assert!(eco.files.iter().all(|f| f.risk <= eco.files[0].risk));
    }

    #[test]
    fn top_risk_is_normalized_to_one() {
        let r = raw(
            vec![commit(NOW - 100, &["a"]), commit(NOW - 200, &["a"])],
            &[("a", 50)],
        );
        let eco = analyze(&r, NOW, Granularity::All);
        // Single file → it is the max on both axes → risk == 1.0.
        assert_eq!(eco.files.len(), 1);
        assert!((eco.files[0].risk - 1.0).abs() < 1e-9);
    }

    #[test]
    fn window_excludes_older_commits() {
        let r = raw(
            vec![
                commit(NOW - 3_600, &["a"]),   // 1h ago — in Day
                commit(NOW - 200_000, &["a"]), // >24h ago — out of Day
            ],
            &[("a", 10)],
        );
        assert_eq!(analyze(&r, NOW, Granularity::Day).files[0].commits, 1);
        assert_eq!(analyze(&r, NOW, Granularity::Month).files[0].commits, 2);
    }

    #[test]
    fn empty_history_yields_no_files() {
        let eco = analyze(&RawEcosystem::default(), NOW, Granularity::All);
        assert!(eco.files.is_empty());
    }

    #[test]
    fn missing_loc_counts_as_zero_complexity() {
        let r = raw(vec![commit(NOW - 100, &["gone"])], &[]);
        let eco = analyze(&r, NOW, Granularity::All);
        assert_eq!(eco.files[0].loc, 0);
        assert_eq!(eco.files[0].risk, 0.0); // zero complexity → zero risk
    }

    #[test]
    fn coupling_ranks_co_changed_partners() {
        let r = raw(
            vec![
                commit(NOW - 100, &["a", "b"]),
                commit(NOW - 200, &["a", "b"]),
                commit(NOW - 300, &["a", "c"]),
                commit(NOW - 400, &["a"]),
            ],
            &[],
        );
        let edges = coupling_for(&r, "a", NOW, Granularity::All, 10);
        assert_eq!(edges[0].partner, "b");
        assert_eq!(edges[0].together, 2);
        // a changed in 4 commits → P(b | a) = 2/4 = 0.5.
        assert!((edges[0].ratio - 0.5).abs() < 1e-9);
        assert_eq!(edges[1].partner, "c");
        assert_eq!(edges[1].together, 1);
    }

    #[test]
    fn top_couplings_ranks_co_changed_pairs() {
        let r = raw(
            vec![
                commit(NOW - 100, &["a", "b"]),
                commit(NOW - 200, &["a", "b"]),
                commit(NOW - 300, &["a", "c"]),
                commit(NOW - 400, &["out.pdf", "a"]), // pdf excluded → no pair
            ],
            &[],
        );
        let pairs = top_couplings(&r, NOW, Granularity::All, 10);
        assert_eq!((pairs[0].a.as_str(), pairs[0].b.as_str()), ("a", "b"));
        assert_eq!(pairs[0].together, 2);
        // (a,c) once; (a,pdf) excluded.
        assert!(pairs.iter().all(|p| p.a != "out.pdf" && p.b != "out.pdf"));
        assert_eq!(pairs.iter().find(|p| p.b == "c").unwrap().together, 1);
    }

    #[test]
    fn ownership_ranks_most_changed_first_with_real_author_mix() {
        let r = raw(
            vec![
                // shared.rs: most-changed (3) and multi-author (alice×2, bob).
                commit_by(NOW - 100, "alice", &["shared.rs"]),
                commit_by(NOW - 150, "bob", &["shared.rs"]),
                commit_by(NOW - 200, "alice", &["shared.rs"]),
                // solo.rs: single-owner but less active (2).
                commit_by(NOW - 300, "alice", &["solo.rs"]),
                commit_by(NOW - 400, "alice", &["solo.rs"]),
            ],
            &[],
        );
        let owners = ownership(&r, NOW, Granularity::All, 10);
        // Most-changed file leads, showing its real 2-author / 67% split — not a
        // wall of single-owner files.
        assert_eq!(owners[0].path, "shared.rs");
        assert_eq!(owners[0].authors, 2);
        assert_eq!(owners[0].primary_author, "alice");
        assert!((owners[0].primary_share - 2.0 / 3.0).abs() < 1e-9);
        // The single-owner file is still reported (bus-factor flagged in the UI).
        let solo = owners.iter().find(|o| o.path == "solo.rs").unwrap();
        assert_eq!(solo.authors, 1);
        assert!((solo.primary_share - 1.0).abs() < 1e-9);
    }

    #[test]
    fn is_excluded_matches_binaries_cad_and_kicad() {
        for p in [
            "doc/spec.pdf",
            "img/logo.PNG",
            "a/b.jpeg",
            "icons/x.svg",
            "board.kicad_pcb",
            "sheet.kicad_sch",
            "proj.kicad_pro",
            "model.step",
            "part.STP",
            "mesh.stl",
            "icon.icns",
            "bundle.zip",
            "assets/Inter.ttf",
            "fonts/x.woff2",
            "hw/proj/fp-info-cache",
        ] {
            assert!(is_excluded(p), "{p} should be excluded");
        }
        for p in [
            "src/main.rs",
            "README.md",
            "Makefile",
            ".gitignore",
            "a.toml",
        ] {
            assert!(!is_excluded(p), "{p} should NOT be excluded");
        }
    }

    #[test]
    fn analyze_drops_excluded_files() {
        let r = raw(
            vec![
                commit(
                    NOW - 100,
                    &["src/a.rs", "doc/manual.pdf", "board.kicad_pcb"],
                ),
                commit(NOW - 200, &["src/a.rs", "doc/manual.pdf"]),
            ],
            &[
                ("src/a.rs", 50),
                ("doc/manual.pdf", 9000),
                ("board.kicad_pcb", 9000),
            ],
        );
        let eco = analyze(&r, NOW, Granularity::All);
        // Only the .rs file survives — the PDF / KiCad artifacts are gone even
        // though their "LOC" is huge.
        assert_eq!(eco.files.len(), 1);
        assert_eq!(eco.files[0].path, "src/a.rs");
    }

    #[test]
    fn coupling_ignores_excluded_partners() {
        let r = raw(
            vec![commit(NOW - 100, &["src/a.rs", "out.pdf", "src/b.rs"])],
            &[],
        );
        let edges = coupling_for(&r, "src/a.rs", NOW, Granularity::All, 10);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].partner, "src/b.rs");
    }

    #[test]
    fn coupling_truncates_to_top_n() {
        let r = raw(vec![commit(NOW - 100, &["a", "b", "c", "d"])], &[]);
        assert_eq!(coupling_for(&r, "a", NOW, Granularity::All, 2).len(), 2);
    }
}
