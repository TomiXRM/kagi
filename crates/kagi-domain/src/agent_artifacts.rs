//! Agent-artifact classification — pure, no I/O (issue #338).
//!
//! Classifies a repo-relative path as an AI-agent artifact so the changed-files
//! views can group and de-emphasise the noisy config (folded under "Agent
//! artifacts (N)") while *emphasising* the small set of convention bodies
//! (`AGENTS.md`, `CLAUDE.md`, …) that actually steer agent behaviour.
//!
//! This is a **separate axis** from [`crate::generated`]: a file can be both
//! generated and an agent artifact. Callers give the agent-artifact grouping
//! precedence for display but must not merge the two mechanisms.
//!
//! Patterns are **hardcoded for v1** (agents keep multiplying; a config surface
//! can come later). Paths are matched with forward slashes, case-sensitive.

/// What kind of agent artifact a path is, if any (issue #338).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentArtifactKind {
    /// A *convention body* — the human-authored rules that change how an agent
    /// behaves (`AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `.cursorrules`,
    /// `.github/copilot-instructions.md`). Emphasised with a badge, never folded.
    ConventionBody,
    /// Agent *config / artifacts* — bulk, often-generated settings and rules
    /// (`.claude/**`, `.cursor/rules/**`, `.specify/**`, `.mcp.json`, …).
    /// Folded under a collapsed "Agent artifacts (N)" group.
    ArtifactConfig,
    /// Not an agent artifact.
    None,
}

/// The final path component (basename) of `path`, split on `/`.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Convention bodies: exact basename anywhere, plus the one fixed-path case.
///
/// Basename match (not path-anchored) so a `CLAUDE.md` in a subdir still reads
/// as a convention body — these files carry meaning wherever they live.
fn is_convention_body(path: &str) -> bool {
    matches!(
        basename(path),
        "AGENTS.md" | "CLAUDE.md" | "GEMINI.md" | ".cursorrules"
    ) || path == ".github/copilot-instructions.md"
}

/// Directory prefixes whose entire subtree is agent artifact config.
const ARTIFACT_DIR_PREFIXES: &[&str] = &[
    ".claude/", // includes .claude/settings.json AND .claude/worktrees/**
    ".cursor/rules/",
    ".specify/",
    ".agents/",
    ".github/agents/",
];

/// Exact-path artifact config files (not a whole subtree).
const ARTIFACT_EXACT: &[&str] = &[".mcp.json", ".worktreeinclude"];

/// Classify `path` (repo-relative, `/`-separated) as an agent artifact.
///
/// Convention bodies are checked first so `.github/copilot-instructions.md`
/// wins over the `.github/agents/**` prefix would-be match on siblings, and a
/// bare `AGENTS.md` is never mistaken for config.
pub fn classify_agent_artifact(path: &str) -> AgentArtifactKind {
    if is_convention_body(path) {
        return AgentArtifactKind::ConventionBody;
    }
    if ARTIFACT_EXACT.contains(&path) || ARTIFACT_DIR_PREFIXES.iter().any(|p| path.starts_with(p)) {
        return AgentArtifactKind::ArtifactConfig;
    }
    AgentArtifactKind::None
}

/// How agent-artifact files are grouped for the changed-files list (issue #338).
///
/// `artifact` holds indices of [`AgentArtifactKind::ArtifactConfig`] files
/// (folded); `normal` holds everything else (convention bodies + non-artifacts,
/// shown inline — convention bodies get a badge at render time). `collapsed`
/// is the fold default — **true** (folding default-on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentArtifactGroup {
    /// Indices of ArtifactConfig files (shown under the collapsed group).
    pub artifact: Vec<usize>,
    /// Indices of the remaining files (convention bodies + non-artifacts).
    pub normal: Vec<usize>,
    /// Default fold state of the "Agent artifacts (N)" section — `true`.
    pub collapsed: bool,
}

/// Partition file indices into normal + artifact groups from per-file kinds.
/// The artifact section defaults to collapsed.
pub fn group_agent_artifacts(kinds: &[AgentArtifactKind]) -> AgentArtifactGroup {
    let mut artifact = Vec::new();
    let mut normal = Vec::new();
    for (i, k) in kinds.iter().enumerate() {
        if *k == AgentArtifactKind::ArtifactConfig {
            artifact.push(i);
        } else {
            normal.push(i);
        }
    }
    AgentArtifactGroup {
        artifact,
        normal,
        collapsed: true,
    }
}

#[cfg(test)]
mod tests {
    use super::AgentArtifactKind::*;
    use super::*;

    // ── Convention bodies (emphasised, never folded) ────────────────
    #[test]
    fn convention_bodies() {
        assert_eq!(classify_agent_artifact("AGENTS.md"), ConventionBody);
        assert_eq!(classify_agent_artifact("CLAUDE.md"), ConventionBody);
        assert_eq!(classify_agent_artifact("GEMINI.md"), ConventionBody);
        assert_eq!(classify_agent_artifact(".cursorrules"), ConventionBody);
        assert_eq!(
            classify_agent_artifact(".github/copilot-instructions.md"),
            ConventionBody
        );
        // Basename match: a nested convention body still counts.
        assert_eq!(classify_agent_artifact("sub/dir/CLAUDE.md"), ConventionBody);
    }

    // ── Artifact config (folded) ────────────────────────────────────
    #[test]
    fn artifact_config() {
        assert_eq!(
            classify_agent_artifact(".claude/settings.json"),
            ArtifactConfig
        );
        assert_eq!(
            classify_agent_artifact(".claude/agents/foo.md"),
            ArtifactConfig
        );
        // .claude/worktrees/** is clearly generated → foldable.
        assert_eq!(
            classify_agent_artifact(".claude/worktrees/x/settings.json"),
            ArtifactConfig
        );
        assert_eq!(
            classify_agent_artifact(".cursor/rules/style.mdc"),
            ArtifactConfig
        );
        assert_eq!(classify_agent_artifact(".specify/plan.md"), ArtifactConfig);
        assert_eq!(classify_agent_artifact(".agents/x.md"), ArtifactConfig);
        assert_eq!(
            classify_agent_artifact(".github/agents/reviewer.md"),
            ArtifactConfig
        );
        assert_eq!(classify_agent_artifact(".mcp.json"), ArtifactConfig);
        assert_eq!(classify_agent_artifact(".worktreeinclude"), ArtifactConfig);
    }

    // ── NEGATIVE: normal files are not misclassified ────────────────
    #[test]
    fn normal_files_are_none() {
        assert_eq!(classify_agent_artifact("src/x.rs"), None);
        assert_eq!(classify_agent_artifact("README.md"), None);
        assert_eq!(classify_agent_artifact("Cargo.toml"), None);
        // A file merely under .github/ that is NOT copilot-instructions and NOT
        // under .github/agents/ must stay None (mutation guard for §6).
        assert_eq!(classify_agent_artifact(".github/workflows/ci.yml"), None);
        assert_eq!(classify_agent_artifact(".github/CODEOWNERS"), None);
        // A file merely named like a convention body's neighbour.
        assert_eq!(classify_agent_artifact("AGENTS.rs"), None);
        assert_eq!(classify_agent_artifact("docs/CLAUDE.txt"), None);
    }

    /// MUTATION GUARD (§6): dropping the `.claude/**` prefix flips this.
    #[test]
    fn claude_subtree_is_artifact() {
        assert_eq!(
            classify_agent_artifact(".claude/settings.local.json"),
            ArtifactConfig
        );
        assert_ne!(classify_agent_artifact(".claude/settings.json"), None);
    }

    /// MUTATION GUARD (§6): convention bodies must NOT be folded — they are
    /// classified ConventionBody, not ArtifactConfig, so grouping keeps them
    /// inline.
    #[test]
    fn convention_body_is_not_artifact_config() {
        assert_ne!(classify_agent_artifact("CLAUDE.md"), ArtifactConfig);
        assert_ne!(classify_agent_artifact("AGENTS.md"), ArtifactConfig);
    }

    // ── grouping helper (folding default-on) ────────────────────────
    #[test]
    fn group_defaults_collapsed_and_splits() {
        let kinds = [None, ArtifactConfig, ConventionBody, ArtifactConfig];
        let g = group_agent_artifacts(&kinds);
        assert_eq!(g.artifact, vec![1, 3]);
        // Convention body (index 2) stays in `normal` — shown with a badge.
        assert_eq!(g.normal, vec![0, 2]);
        assert!(g.collapsed, "artifact section must default collapsed");
    }

    #[test]
    fn group_all_normal() {
        let g = group_agent_artifacts(&[None, ConventionBody]);
        assert_eq!(g.artifact, Vec::<usize>::new());
        assert_eq!(g.normal, vec![0, 1]);
    }
}
