//! Structured commit-message template — T-COMMIT-009 (lane W14-TEMPLATE).
//!
//! Pure, UI-independent assembly + parsing for the Commit Panel's *template*
//! mode. The panel collects six fields — `type` / `scope` / `summary` / `body`
//! / `test` / `risk` — and this module turns them into a single commit message
//! (and, for the reverse direction, makes a best-effort guess at the structured
//! fields from a plain message so the plain ⇄ template toggle does not lose
//! work).
//!
//! Format (Conventional-Commits-flavoured, but never strictly enforced — empty
//! fields are simply omitted):
//!
//! ```text
//! type(scope): summary
//!
//! <body>
//!
//! Test: <test>
//! Risk: <risk>
//! ```
//!
//! Assembly rules (all whitespace-trimmed first):
//! - The subject line is built from `type`, `scope`, `summary`:
//!   - `type` + `scope` + `summary` → `type(scope): summary`
//!   - `type` + `summary` (no scope) → `type: summary`
//!   - `scope` but no `type` → `(scope): summary` is *not* produced; without a
//!     `type` the scope is dropped and the line is just `summary` (a bare
//!     `(scope):` prefix is not valid Conventional Commits).
//!   - only `summary` → `summary`
//!   - no `summary` but a `type` → `type:` (trailing colon kept so the intent
//!     survives a round-trip; rare, but better than silently dropping `type`).
//! - `body`, `Test:` and `Risk:` trailers are each separated from the previous
//!   block by one blank line, and omitted entirely when empty.
//!
//! Parsing (`parse_message`) is best-effort and deliberately forgiving: it only
//! tries to recover `type(scope): summary` from the first line and treats the
//! remainder as the body. It does **not** attempt to re-extract `Test:` / `Risk:`
//! trailers — round-tripping those exactly is out of MVP scope (ADR-0042 keeps
//! the expanded plain text as the source of truth), and a wrong guess there
//! would be more surprising than leaving them in the body.

/// The six structured fields of a template-mode commit message.
///
/// All fields are owned `String`s; callers fill them from the panel's
/// `InputState`s. Empty / whitespace-only fields are omitted by [`assemble`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateFields {
    /// Commit type, e.g. `feat`, `fix`, `docs` (free text or a picked value).
    pub r#type: String,
    /// Optional scope, e.g. `commit-panel`.
    pub scope: String,
    /// Short subject summary (the part after `type(scope): `).
    pub summary: String,
    /// Free-form body paragraph(s).
    pub body: String,
    /// `Test:` trailer — how the change was verified.
    pub test: String,
    /// `Risk:` trailer — known risks / blast radius.
    pub risk: String,
}

/// Built-in commit type choices offered by the panel's `type` picker.
///
/// The field is *not* restricted to these — the panel also allows free text —
/// but these are the quick-pick options (Conventional Commits' common set).
pub const TYPE_CHOICES: &[&str] = &[
    "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert",
];

impl TemplateFields {
    /// Build the structured fields from the six raw input strings.
    ///
    /// No trimming or validation is done here — that happens in [`assemble`] —
    /// so this is just an ergonomic constructor for callers reading six
    /// `InputState`s.
    pub fn new(
        r#type: impl Into<String>,
        scope: impl Into<String>,
        summary: impl Into<String>,
        body: impl Into<String>,
        test: impl Into<String>,
        risk: impl Into<String>,
    ) -> Self {
        TemplateFields {
            r#type: r#type.into(),
            scope: scope.into(),
            summary: summary.into(),
            body: body.into(),
            test: test.into(),
            risk: risk.into(),
        }
    }
}

/// Assemble the six [`TemplateFields`] into a single commit message.
///
/// See the module docs for the exact rules. The result has no trailing newline
/// and never contains a block for an empty field. Returns an empty string when
/// every field is empty.
pub fn assemble(fields: &TemplateFields) -> String {
    let ty = fields.r#type.trim();
    let scope = fields.scope.trim();
    let summary = fields.summary.trim();
    let body = fields.body.trim();
    let test = fields.test.trim();
    let risk = fields.risk.trim();

    // ── Subject line ────────────────────────────────────────────
    let subject = build_subject(ty, scope, summary);

    // ── Blocks, joined by a single blank line, empties skipped ──
    let mut blocks: Vec<String> = Vec::new();
    if !subject.is_empty() {
        blocks.push(subject);
    }
    if !body.is_empty() {
        blocks.push(body.to_string());
    }

    // Test: / Risk: trailers share one block (no blank line between them, but a
    // blank line before the group — they are conventionally grouped).
    let mut trailers: Vec<String> = Vec::new();
    if !test.is_empty() {
        trailers.push(format!("Test: {}", test));
    }
    if !risk.is_empty() {
        trailers.push(format!("Risk: {}", risk));
    }
    if !trailers.is_empty() {
        blocks.push(trailers.join("\n"));
    }

    blocks.join("\n\n")
}

/// Build the subject line from `type` / `scope` / `summary` (all pre-trimmed).
fn build_subject(ty: &str, scope: &str, summary: &str) -> String {
    match (ty.is_empty(), scope.is_empty(), summary.is_empty()) {
        // Nothing → empty subject.
        (true, _, true) => String::new(),
        // No type: scope cannot stand alone in CC; drop it, keep summary.
        (true, _, false) => summary.to_string(),
        // Type present, no summary → "type:" (preserve the intent).
        (false, true, true) => format!("{}:", ty),
        (false, false, true) => format!("{}({}):", ty, scope),
        // Type + summary, optional scope.
        (false, true, false) => format!("{}: {}", ty, summary),
        (false, false, false) => format!("{}({}): {}", ty, scope, summary),
    }
}

/// Best-effort parse of a plain commit `message` back into [`TemplateFields`].
///
/// The first line is matched against `type(scope): summary` (scope optional).
/// On a match, `type` / `scope` / `summary` are filled and everything after the
/// first blank line becomes the `body`. If the first line does not look like a
/// Conventional-Commits subject, the **entire** message goes into `summary`
/// (lossless: the toggle back to plain re-assembles it unchanged) and the other
/// fields stay empty.
///
/// `Test:` / `Risk:` are intentionally left inside the body — see module docs.
pub fn parse_message(message: &str) -> TemplateFields {
    let msg = message.trim_end_matches(['\n', '\r']);
    if msg.trim().is_empty() {
        return TemplateFields::default();
    }

    // Split off the first line; the body is whatever follows the first blank
    // line (so a wrapped subject is not mistaken for a body).
    let (first_line, rest) = match msg.split_once('\n') {
        Some((f, r)) => (f, r),
        None => (msg, ""),
    };

    // Body = everything after the leading blank line(s) following the subject.
    let body = rest.trim_start_matches(['\n', '\r']).to_string();

    if let Some((ty, scope, summary)) = parse_subject(first_line.trim()) {
        TemplateFields {
            r#type: ty,
            scope,
            summary,
            body,
            test: String::new(),
            risk: String::new(),
        }
    } else {
        // Not a recognisable subject → keep the whole thing in summary so the
        // round-trip back to plain is lossless.
        TemplateFields {
            summary: msg.to_string(),
            ..TemplateFields::default()
        }
    }
}

/// Parse a single subject line of the form `type(scope): summary`.
///
/// Returns `Some((type, scope, summary))` when the line starts with a token
/// followed by `: ` (scope in parentheses optional). The `type` token must be a
/// single word (no whitespace) for the line to be treated as structured;
/// otherwise `None` (the caller then dumps the whole message into `summary`).
fn parse_subject(line: &str) -> Option<(String, String, String)> {
    let colon = line.find(':')?;
    let (head, after) = line.split_at(colon);
    let summary = after[1..].trim().to_string(); // skip the ':'

    // Split the head into type + optional (scope).
    let (ty, scope) = if let Some(open) = head.find('(') {
        if head.ends_with(')') {
            let ty = head[..open].trim();
            let scope = head[open + 1..head.len() - 1].trim();
            (ty.to_string(), scope.to_string())
        } else {
            // Unbalanced paren → treat the whole head as the type.
            (head.trim().to_string(), String::new())
        }
    } else {
        (head.trim().to_string(), String::new())
    };

    // A structured type is a single non-empty word (Conventional Commits style).
    // If the head contains spaces (e.g. "Merge branch") it is prose, not a type.
    if ty.is_empty() || ty.chars().any(|c| c.is_whitespace()) {
        return None;
    }

    Some((ty, scope, summary))
}
