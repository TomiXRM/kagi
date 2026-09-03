# ADR-0152: Agent provenance classification (graph + PR badges)

- Status: **Accepted**
- Date: 2026-09-03
- Related: #337 (this feature), #336 (trailer parser, reused), #354 (colour-blind
  badge rules), #346 (local ruleset preflight — coupling deferred)

## Context

In the agent era a commit's `author` no longer answers "who wrote this". Agents
commit under the user's name, under a bot name, or leave the signal only in a
trailer. GitHub has productised the distinction (`require_extra_approval_for_
unattributed_changes`, default on), so "an AI made this change" is a first-class
history axis. Kagi's commit graph is its core, so surfacing an AI axis there is a
natural fit. #337 asks for a badge on graph rows and on the PR list.

The hard risk is **false positives**: `Co-Authored-By:` is also how humans record
pair-programming, and over-flagging reads as judgmental. The PM locked the design
(#337 §5): distinguish agent from human co-authors by **bot-email pattern**; a
plain human co-author is never flagged; an unclassifiable commit shows **nothing**
(prefer no badge to a wrong one); a `Reviewed-by:` trailer adds a neutral
qualifier, never judgment; the badge must be legible without relying on hue.

## Decision

**Classification is pure and lives in `kagi-domain`.** New module
`crates/kagi-domain/src/provenance.rs` exposes
`classify_provenance(trailers, author, committer, branch_name, extra) ->
Option<Provenance>` where `Provenance { agent: AgentKind, source_url:
Option<String>, reviewed: bool }`. It depends only on `std` and the #336 trailer
parser (`parse_trailers`, `is_url`, `split_name_email` — the last promoted to
`pub` for reuse). No `git2`, no `gpui`, no I/O — the domain-purity invariant holds
and all logic is unit-tested here.

**Three detection routes, strongest first** (so a trailer's source URL wins over
a weaker branch guess):

1. **trailer** — a `Co-authored-by:` whose email is a *bot* form, or a known
   agent trailer key (`Amp-Thread-ID:`, which also yields `source_url`).
2. **author / committer** — a bot login/email (`copilot@github.com`,
   `noreply@anthropic.com`, `copilot-swe-agent[bot]`, any `…[bot]` identity).
3. **branch prefix** — `copilot/`, `cu-`, `worktree-`.

**Bot-vs-human discrimination is the crux.** `is_bot_identity` flags a co-author
only when the email is a known agent address or the name/email carries a `[bot]`
marker. A plain `alice@example.com` or a human `…@users.noreply.github.com`
co-author returns `None`. This is asserted, and the assert is mutation-checked
(forcing `is_bot_identity` true makes the human tests fail).

**Detection is built-in defaults + a settings-extensible list** (agents keep
appearing; a hardcoded list rots). `Settings::agent_patterns()` reads a flat
`agent_patterns` string (`label:needle`, comma-separated, per the settings rules)
and `AgentPattern::parse_list` (pure) parses it; the UI layers it on top of the
built-ins. Parsing is pure so it lives in the domain; only the disk read is in
`settings.rs`.

**UI is display-only, GUI-gated.** `CommitRow` gains an `Option<Provenance>`
computed once in `build_commit_rows` (not per frame; that is also the single
place that loads the extensible patterns). The graph badge and the PR-list
"agent-created" badge both render a 🤖 glyph plus the agent name, so they are
legible without hue (aligns with #354). The PR badge classifies from author
login + head branch only (no trailers/committer available) and uses built-in
patterns only, to avoid per-frame `Settings::load()` I/O. Conversation-URL
linkification already happens in the commit detail via the #336 trailer renderer,
so the badge itself needs no click handler.

## Consequences

- Provenance is testable without a repo or a window; the acceptance asserts
  (§6: all three routes; human co-author not misclassified; unclassifiable →
  nothing) live in `provenance.rs`.
- New agents are added by a settings string, no release. Adding a *dedicated*
  `AgentKind` variant (icon/label) is still a code change.
- **Deferred:** `require_extra_approval_for_unattributed_changes` / #346 local
  preflight coupling — out of scope per §5.
- PR-list extensibility uses built-ins only; wire `agent_patterns` through if a
  PR-specific agent ever needs it (marked with a `ponytail:` note in `pr_mode`).
- The graph branch-route only fires when a branch/HEAD ref badge sits on the
  commit (refs exist at tips), so mid-history agent commits rely on the trailer
  and author routes — the stronger signals anyway.
