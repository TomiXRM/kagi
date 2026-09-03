//! Pure, dependency-free CODEOWNERS parser + matcher (ADR-0153).
//!
//! GitHub's API does **not** return which CODEOWNERS entries own a PR's files,
//! so kagi computes it locally: parse the repo's `CODEOWNERS`, match a PR's
//! changed paths, and show "@org/team review required" before the reviewer even
//! sees a `BLOCKED` merge state.
//!
//! # Why hand-rolled, not the `ignore` crate
//!
//! `kagi-domain` is dependency-free by invariant, and CODEOWNERS patterns are a
//! *small* subset of gitignore: no `[charclass]`, and only the constructs
//! GitHub documents — a leading `/` anchor, a trailing `/` for directories,
//! `*` (within a path segment) and `**` (across segments). A ~40-line matcher
//! covers it; pulling in a glob crate to save that is the tail wagging the dog.
//!
//! # Semantics
//!
//! * Blank lines and `#` comments are ignored.
//! * A line is `PATTERN owner1 owner2 …`; owners are `@user`, `@org/team`, or
//!   an email. A pattern with **no** owners (or a leading `!`) clears ownership.
//! * **Last matching rule wins** (gitignore / CODEOWNERS precedence).
//!
//! ponytail: `build/*`-style single-level globs are matched *recursively* for
//! ownership (a matched child dir owns its contents). GitHub treats `/*` as
//! one level; the difference only widens who is asked to review, never narrows
//! it — the safe direction. Tighten by not appending the implicit `**` if a
//! real CODEOWNERS ever depends on the distinction.

/// One parsed CODEOWNERS line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// The pattern as written (without a leading `!`).
    pub pattern: String,
    /// Owners on this line: `@user`, `@org/team`, or email. Empty ⇒ the rule
    /// removes ownership for matching files.
    pub owners: Vec<String>,
    /// A `!`-prefixed negation: matching files have no required owner.
    pub negated: bool,
}

/// Parse CODEOWNERS text into rules in file order (precedence is last-wins).
pub fn parse(text: &str) -> Vec<Rule> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<Rule> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut fields = line.split_whitespace();
    let raw = fields.next()?;
    let (negated, pattern) = match raw.strip_prefix('!') {
        Some(rest) => (true, rest.to_string()),
        None => (false, raw.to_string()),
    };
    if pattern.is_empty() {
        return None;
    }
    let owners: Vec<String> = fields.map(str::to_string).collect();
    Some(Rule {
        pattern,
        owners,
        negated,
    })
}

/// Owners required for `path` (repo-relative, `/`-separated, no leading slash).
///
/// Returns the owners of the **last** matching rule, or an empty vec if the
/// last match is a negation / owner-less rule, or nothing matches.
pub fn owners_for<'a>(rules: &'a [Rule], path: &str) -> &'a [String] {
    let path = path.trim_start_matches('/');
    rules
        .iter()
        .rev()
        .find(|r| pattern_matches(&r.pattern, path))
        .filter(|r| !r.negated)
        .map(|r| r.owners.as_slice())
        .unwrap_or(&[])
}

/// The distinct owners required across a set of changed `paths`, in first-seen
/// order. This is the "who must review this PR" list.
pub fn required_owners(rules: &[Rule], paths: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in paths {
        for o in owners_for(rules, p) {
            if !out.iter().any(|x| x == o) {
                out.push(o.clone());
            }
        }
    }
    out
}

/// Does one CODEOWNERS `pattern` match a repo-relative `path`?
fn pattern_matches(pattern: &str, path: &str) -> bool {
    let dir_only = pattern.ends_with('/');
    let core = pattern.trim_end_matches('/');
    // A slash anywhere but a lone trailing one anchors the pattern to the repo
    // root (gitignore rule). `docs/` and `*.js` float; `/docs`, `a/b` anchor.
    let anchored = core.contains('/');
    let core = core.trim_start_matches('/');
    if core.is_empty() {
        return false;
    }
    let path_segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    if !anchored {
        // Single-segment floating pattern: match any path component (for a
        // directory pattern, an *ancestor* component so files under it match).
        let last = path_segs.len();
        let range = if dir_only {
            0..last.saturating_sub(1)
        } else {
            0..last
        };
        return path_segs
            .get(range)
            .map(|segs| segs.iter().any(|c| seg_match(core, c)))
            .unwrap_or(false);
    }

    // Anchored: match the pattern segments as a prefix of the path (append an
    // implicit `**` so a matched entry owns its contents too).
    let mut pat_segs: Vec<&str> = core.split('/').collect();
    pat_segs.push("**");
    glob(&pat_segs, &path_segs)
}

/// Recursive segment glob with `**` = zero-or-more path segments.
fn glob(pat: &[&str], path: &[&str]) -> bool {
    match pat.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|i| glob(rest, &path[i..])),
        Some((seg, rest)) => !path.is_empty() && seg_match(seg, path[0]) && glob(rest, &path[1..]),
    }
}

/// Match one path segment against one pattern segment supporting `*` (any run,
/// no `/`) and `?` (one char). Classic two-pointer wildcard matcher.
fn seg_match(pat: &str, seg: &str) -> bool {
    let (p, s): (Vec<char>, Vec<char>) = (pat.chars().collect(), seg.chars().collect());
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# top-level owner
*            @org/core
*.js         @js-team
/docs/       @docs-team
apps/api/    @org/backend @alice
!secret.txt
config.yml   @ops
";

    #[test]
    fn parses_teams_users_and_negation() {
        let rules = parse(SAMPLE);
        assert_eq!(rules.len(), 6, "comment + blank lines dropped");
        assert_eq!(rules[0].pattern, "*");
        assert_eq!(rules[0].owners, vec!["@org/core".to_string()]);
        // team + user on one line.
        assert_eq!(
            rules[3].owners,
            vec!["@org/backend".to_string(), "@alice".to_string()]
        );
        assert!(rules[4].negated);
        assert!(rules[4].owners.is_empty());
    }

    #[test]
    fn star_owns_everything_as_the_fallback() {
        let rules = parse(SAMPLE);
        // README.md matches only `*` → core team.
        assert_eq!(owners_for(&rules, "README.md"), &["@org/core".to_string()]);
    }

    #[test]
    fn last_match_wins_over_the_star_fallback() {
        let rules = parse(SAMPLE);
        // main.js matches `*` then `*.js`; last wins.
        assert_eq!(owners_for(&rules, "src/main.js"), &["@js-team".to_string()]);
    }

    #[test]
    fn directory_pattern_owns_the_subtree() {
        let rules = parse(SAMPLE);
        assert_eq!(
            owners_for(&rules, "docs/guide/intro.md"),
            &["@docs-team".to_string()]
        );
        // Anchored two-segment dir pattern with two owners.
        assert_eq!(
            owners_for(&rules, "apps/api/routes/user.rs"),
            &["@org/backend".to_string(), "@alice".to_string()]
        );
    }

    #[test]
    fn negation_clears_ownership_when_it_is_the_last_match() {
        let rules = parse(SAMPLE);
        // secret.txt matches `*` then `!secret.txt`; negation wins → no owner.
        assert!(owners_for(&rules, "secret.txt").is_empty());
    }

    #[test]
    fn anchored_pattern_does_not_match_same_name_deeper() {
        // `/docs/` is root-anchored: a nested `docs/` is not the same dir.
        let rules = parse("/docs/  @docs-team\n");
        assert_eq!(owners_for(&rules, "docs/x.md"), &["@docs-team".to_string()]);
        assert!(owners_for(&rules, "pkg/docs/x.md").is_empty());
    }

    #[test]
    fn floating_dir_pattern_matches_at_any_depth() {
        // No leading slash → floats; matches a `build/` dir anywhere.
        let rules = parse("build/  @ci\n");
        assert_eq!(owners_for(&rules, "a/b/build/out.o"), &["@ci".to_string()]);
        assert_eq!(owners_for(&rules, "build/out.o"), &["@ci".to_string()]);
    }

    #[test]
    fn required_owners_dedups_across_paths() {
        let rules = parse(SAMPLE);
        let owners = required_owners(
            &rules,
            &[
                "src/a.js".into(),   // @js-team
                "src/b.js".into(),   // @js-team (dup)
                "docs/x.md".into(),  // @docs-team
                "secret.txt".into(), // negated → none
            ],
        );
        assert_eq!(
            owners,
            vec!["@js-team".to_string(), "@docs-team".to_string()]
        );
    }

    #[test]
    fn seg_match_wildcards() {
        assert!(seg_match("*", "anything"));
        assert!(seg_match("*.rs", "main.rs"));
        assert!(!seg_match("*.rs", "main.js"));
        assert!(seg_match("v?", "v1"));
        assert!(!seg_match("v?", "v10"));
        assert!(seg_match("a*c", "abbbc"));
    }
}
