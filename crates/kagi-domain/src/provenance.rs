//! Agent provenance classification — issue #337.
//!
//! Pure, UI-independent text logic that answers "was this commit written by an
//! AI agent, and if so which one?". It depends only on `std` and the sibling
//! [`crate::trailers`] parser (#336) — **no** `git2`, `gpui`, or I/O.
//!
//! Three detection routes are tried, strongest first:
//!  1. **trailer** — a `Co-authored-by:` with a *bot* email, or a known
//!     agent trailer key (`Amp-Thread-ID:`). May carry a source URL.
//!  2. **author / committer** — a bot login/email (`copilot-swe-agent[bot]`,
//!     `app/github-copilot`, `copilot@github.com`, `noreply@anthropic.com`).
//!  3. **branch prefix** — `copilot/`, `cu-`, `worktree-`.
//!
//! Design invariants (PM-locked, see ADR-0152 / issue #337 §5):
//!  - A **plain human** `Co-authored-by:` is NOT flagged — only bot-email forms
//!    (`copilot@github.com`, `…[bot]…` noreply, known agent emails) are. This is
//!    the single most important discrimination and is asserted below.
//!  - An **unclassifiable** commit returns `None` (show nothing). We prefer a
//!    missing badge over a false positive.
//!  - Built-in defaults are complemented by a **settings-extensible** pattern
//!    list ([`AgentPattern`]) so new agents can be recognized without a release.

use crate::commit::Signature;
use crate::trailers::{is_url, split_name_email, Trailer};

// ──────────────────────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────────────────────

/// Which agent produced a commit. Known agents get a stable variant so the UI
/// can pick an icon/label; anything else is [`AgentKind::Named`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentKind {
    Copilot,
    Amp,
    ClaudeCode,
    Cursor,
    ContainerUse,
    /// An agent recognized by a user-supplied pattern or a `[bot]` login we
    /// don't have a dedicated variant for. Carries a display label.
    Named(String),
}

impl AgentKind {
    /// Short display label for the badge (non-judgmental — just the name).
    pub fn label(&self) -> &str {
        match self {
            AgentKind::Copilot => "Copilot",
            AgentKind::Amp => "Amp",
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::Cursor => "Cursor",
            AgentKind::ContainerUse => "container-use",
            AgentKind::Named(s) => s,
        }
    }
}

/// The provenance verdict for one commit. Only produced when an agent route
/// matched — absence (`Option::None`) means "show nothing".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// Which agent produced the commit.
    pub agent: AgentKind,
    /// Link back to the originating conversation/thread, when a trailer carries
    /// one (e.g. `Amp-Thread-ID: https://…`). Reuses #336's URL detection.
    pub source_url: Option<String>,
    /// A `Reviewed-by:` trailer is present — a human vouched for the change.
    /// Renders as a neutral "reviewed" qualifier, never as judgment.
    pub reviewed: bool,
}

/// A user-extensible detection pattern (from settings). `needle` is matched
/// case-insensitively as a substring of the author/committer name & email and
/// the branch name; on a hit the commit is attributed to `Named(label)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPattern {
    pub label: String,
    pub needle: String,
}

impl AgentPattern {
    /// Parse a comma-separated settings string of `label:needle` entries (a
    /// bare `needle` with no colon uses the needle as its own label). Blank or
    /// needle-less entries are skipped. Pure text — safe in `kagi-domain`.
    pub fn parse_list(raw: &str) -> Vec<AgentPattern> {
        raw.split(',')
            .filter_map(|e| {
                let e = e.trim();
                if e.is_empty() {
                    return None;
                }
                let (label, needle) = match e.split_once(':') {
                    Some((l, n)) => (l.trim(), n.trim()),
                    None => (e, e),
                };
                if needle.is_empty() {
                    return None;
                }
                Some(AgentPattern {
                    label: label.to_string(),
                    needle: needle.to_string(),
                })
            })
            .collect()
    }
}

// ──────────────────────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────────────────────

/// Classify the provenance of a commit from its parsed `trailers` (see
/// [`crate::trailers::parse_trailers`]), `author`/`committer` signatures, the
/// `branch_name` it sits on (if known), and any user-supplied `extra` patterns.
///
/// Returns `None` when nothing matches — the caller shows no badge.
pub fn classify_provenance(
    trailers: &[Trailer],
    author: &Signature,
    committer: &Signature,
    branch_name: Option<&str>,
    extra: &[AgentPattern],
) -> Option<Provenance> {
    // `Reviewed-by:` is a qualifier on any detected agent, independent of route.
    let reviewed = trailers
        .iter()
        .any(|t| t.key.eq_ignore_ascii_case("reviewed-by"));

    let build = |agent, source_url| {
        Some(Provenance {
            agent,
            source_url,
            reviewed,
        })
    };

    // Route 1 — trailers (strongest; may carry a source URL).
    if let Some((agent, url)) = classify_trailers(trailers) {
        return build(agent, url);
    }
    // Route 2 — author / committer identity.
    if let Some(agent) = agent_for_identity(&author.name, &author.email)
        .or_else(|| agent_for_identity(&committer.name, &committer.email))
    {
        return build(agent, None);
    }
    // Route 3 — branch prefix.
    if let Some(agent) = branch_name.and_then(agent_for_branch) {
        return build(agent, None);
    }
    // Extensible user patterns (author/committer/branch substring match).
    if let Some(agent) = match_extra(extra, author, committer, branch_name) {
        return build(agent, None);
    }
    None
}

// ──────────────────────────────────────────────────────────────
// Route helpers
// ──────────────────────────────────────────────────────────────

/// Route 1: recognize an agent from the trailer block.
fn classify_trailers(trailers: &[Trailer]) -> Option<(AgentKind, Option<String>)> {
    for t in trailers {
        // Known agent trailer keys (e.g. Amp-Thread-ID) — may hold a URL.
        if let Some(agent) = agent_for_trailer_key(&t.key) {
            let url = is_url(&t.value).then(|| t.value.trim().to_string());
            return Some((agent, url));
        }
        // Co-authored-by, but ONLY when the co-author is a bot. A plain human
        // co-author must not be flagged (PM-locked discrimination).
        if t.key.eq_ignore_ascii_case("co-authored-by") {
            let (name, email) = split_name_email(&t.value);
            if is_bot_identity(&name, &email) {
                let agent = agent_for_identity(&name, &email)
                    .unwrap_or_else(|| AgentKind::Named(strip_bot(&name)));
                return Some((agent, None));
            }
        }
    }
    None
}

/// Map a known agent-specific trailer key to its agent.
fn agent_for_trailer_key(key: &str) -> Option<AgentKind> {
    match key.to_ascii_lowercase().as_str() {
        "amp-thread-id" => Some(AgentKind::Amp),
        _ => None,
    }
}

/// True when a `name <email>` pair is a bot/agent identity rather than a human.
/// This is the crux of the "don't misclassify human co-authors" invariant.
fn is_bot_identity(name: &str, email: &str) -> bool {
    if agent_for_identity(name, email).is_some() {
        return true;
    }
    // GitHub bot accounts render as `<login>[bot]`, with noreply email
    // `<id>+<login>[bot]@users.noreply.github.com`.
    name.to_ascii_lowercase().contains("[bot]") || email.to_ascii_lowercase().contains("[bot]")
}

/// Route 2 (and the co-author sub-case): recognize a known agent from a
/// `name`/`email` identity. Returns `None` for plain humans.
fn agent_for_identity(name: &str, email: &str) -> Option<AgentKind> {
    let e = email.trim().to_ascii_lowercase();
    match e.as_str() {
        "copilot@github.com" => return Some(AgentKind::Copilot),
        "noreply@anthropic.com" => return Some(AgentKind::ClaudeCode),
        _ => {}
    }
    let n = name.to_ascii_lowercase();
    if n.contains("copilot") || e.contains("copilot") {
        return Some(AgentKind::Copilot);
    }
    if n.contains("[bot]") {
        return Some(AgentKind::Named(strip_bot(name)));
    }
    None
}

/// Route 3: recognize an agent from a branch-name prefix.
fn agent_for_branch(branch: &str) -> Option<AgentKind> {
    let b = branch.trim();
    if b.starts_with("copilot/") {
        Some(AgentKind::Copilot)
    } else if b.starts_with("cu-") || b.starts_with("worktree-") {
        Some(AgentKind::ContainerUse)
    } else {
        None
    }
}

/// Extensible route: substring-match user patterns against the commit identity.
fn match_extra(
    extra: &[AgentPattern],
    author: &Signature,
    committer: &Signature,
    branch: Option<&str>,
) -> Option<AgentKind> {
    let fields = [
        author.name.to_ascii_lowercase(),
        author.email.to_ascii_lowercase(),
        committer.name.to_ascii_lowercase(),
        committer.email.to_ascii_lowercase(),
        branch.unwrap_or("").to_ascii_lowercase(),
    ];
    extra.iter().find_map(|p| {
        let needle = p.needle.to_ascii_lowercase();
        fields
            .iter()
            .any(|f| f.contains(&needle))
            .then(|| AgentKind::Named(p.label.clone()))
    })
}

/// Strip a trailing `[bot]` marker to get a cleaner display label.
fn strip_bot(name: &str) -> String {
    let n = name.trim();
    n.strip_suffix("[bot]").unwrap_or(n).trim().to_string()
}

// ──────────────────────────────────────────────────────────────
// Tests — mirror issue #337 §6 acceptance criteria.
// ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::trailers::parse_trailers;

    fn sig(name: &str, email: &str) -> Signature {
        Signature {
            name: name.into(),
            email: email.into(),
            time: 0,
        }
    }

    fn human() -> Signature {
        sig("Alice Example", "alice@example.com")
    }

    // ── §6: all three routes detect ──────────────────────────────────────

    #[test]
    fn route_trailer_coauthor_bot_detected() {
        let t = parse_trailers("Subject\n\nCo-authored-by: Copilot <copilot@github.com>\n");
        let p = classify_provenance(&t, &human(), &human(), None, &[]).unwrap();
        assert_eq!(p.agent, AgentKind::Copilot);
    }

    #[test]
    fn route_trailer_amp_thread_id_with_url() {
        let t = parse_trailers("Subject\n\nAmp-Thread-ID: https://ampcode.com/threads/T-abc\n");
        let p = classify_provenance(&t, &human(), &human(), None, &[]).unwrap();
        assert_eq!(p.agent, AgentKind::Amp);
        assert_eq!(
            p.source_url.as_deref(),
            Some("https://ampcode.com/threads/T-abc")
        );
    }

    #[test]
    fn route_author_committer_detected() {
        let bot = sig(
            "copilot-swe-agent[bot]",
            "198982749+copilot@users.noreply.github.com",
        );
        let p = classify_provenance(&[], &bot, &human(), None, &[]).unwrap();
        assert_eq!(p.agent, AgentKind::Copilot);
    }

    #[test]
    fn route_branch_prefix_detected() {
        let copilot = classify_provenance(&[], &human(), &human(), Some("copilot/fix-1"), &[]);
        assert_eq!(copilot.unwrap().agent, AgentKind::Copilot);
        let cu = classify_provenance(&[], &human(), &human(), Some("cu-abc123"), &[]);
        assert_eq!(cu.unwrap().agent, AgentKind::ContainerUse);
        let wt = classify_provenance(&[], &human(), &human(), Some("worktree-xyz"), &[]);
        assert_eq!(wt.unwrap().agent, AgentKind::ContainerUse);
    }

    // ── §6: human Co-authored-by NOT misclassified (the crux) ────────────

    #[test]
    fn human_coauthor_not_misclassified() {
        // A perfectly ordinary human co-author. Must yield None — flagging it
        // would be a false positive. Mutation target: weaken `is_bot_identity`
        // to accept any co-author and this fails.
        let t = parse_trailers("Subject\n\nCo-authored-by: Bob Human <bob@example.com>\n");
        assert!(classify_provenance(&t, &human(), &human(), None, &[]).is_none());
    }

    #[test]
    fn human_noreply_coauthor_not_misclassified() {
        // A human using a personal GitHub noreply address — NOT a `[bot]` form.
        let t = parse_trailers(
            "Subject\n\nCo-authored-by: Carol <12345+carol@users.noreply.github.com>\n",
        );
        assert!(classify_provenance(&t, &human(), &human(), None, &[]).is_none());
    }

    // ── §6: unclassifiable → nothing shown ───────────────────────────────

    #[test]
    fn unclassifiable_shows_nothing() {
        let t = parse_trailers("Just a normal commit\n\nSigned-off-by: Dan <dan@corp.com>\n");
        assert!(classify_provenance(&t, &human(), &human(), Some("feature/login"), &[]).is_none());
    }

    // ── reviewed qualifier (non-judgmental) ──────────────────────────────

    #[test]
    fn reviewed_by_adds_qualifier() {
        let t = parse_trailers(
            "Subject\n\nCo-authored-by: Copilot <copilot@github.com>\nReviewed-by: Eve <eve@x.com>\n",
        );
        let p = classify_provenance(&t, &human(), &human(), None, &[]).unwrap();
        assert_eq!(p.agent, AgentKind::Copilot);
        assert!(p.reviewed);
    }

    #[test]
    fn reviewed_false_without_trailer() {
        let t = parse_trailers("Subject\n\nCo-authored-by: Copilot <copilot@github.com>\n");
        assert!(
            !classify_provenance(&t, &human(), &human(), None, &[])
                .unwrap()
                .reviewed
        );
    }

    // ── claude-code co-author (real form used by this repo) ──────────────

    #[test]
    fn claude_code_coauthor_detected() {
        let t = parse_trailers("Subject\n\nCo-Authored-By: Claude <noreply@anthropic.com>\n");
        let p = classify_provenance(&t, &human(), &human(), None, &[]).unwrap();
        assert_eq!(p.agent, AgentKind::ClaudeCode);
    }

    // ── extensible settings patterns ─────────────────────────────────────

    #[test]
    fn extra_pattern_matches_and_labels() {
        let extra = AgentPattern::parse_list("Devin:devin-ai, cursor");
        let dev = sig("devin-ai-integration", "bot@devin.ai");
        let p = classify_provenance(&[], &dev, &human(), None, &extra).unwrap();
        assert_eq!(p.agent, AgentKind::Named("Devin".into()));

        // Bare needle doubles as label; matches on branch too.
        let p2 = classify_provenance(&[], &human(), &human(), Some("cursor/wip"), &extra).unwrap();
        assert_eq!(p2.agent, AgentKind::Named("cursor".into()));
    }

    #[test]
    fn parse_list_skips_blanks_and_empty_needles() {
        let got = AgentPattern::parse_list(" , Foo:, :bar, Baz:baz , qux ");
        assert_eq!(
            got,
            vec![
                AgentPattern {
                    label: "".into(),
                    needle: "bar".into()
                },
                AgentPattern {
                    label: "Baz".into(),
                    needle: "baz".into()
                },
                AgentPattern {
                    label: "qux".into(),
                    needle: "qux".into()
                },
            ]
        );
    }

    // ── route precedence: trailer beats branch ───────────────────────────

    #[test]
    fn trailer_route_wins_over_branch() {
        let t = parse_trailers("Subject\n\nAmp-Thread-ID: https://ampcode.com/threads/T-1\n");
        // Branch says copilot, trailer says Amp — trailer (with URL) wins.
        let p = classify_provenance(&t, &human(), &human(), Some("copilot/x"), &[]).unwrap();
        assert_eq!(p.agent, AgentKind::Amp);
        assert!(p.source_url.is_some());
    }
}
