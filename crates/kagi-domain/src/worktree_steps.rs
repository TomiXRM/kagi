//! Typed post-create / pre-remove worktree steps (issue #341, ADR-0161).
//!
//! Pure model only: the three step types, their trust classification, their
//! human-readable enumeration for the plan, and the control-byte escape used
//! when a committed config's contents are shown in the trust prompt.
//!
//! **No I/O, no toml, no hashing here** — parsing `.kagi/worktree.toml`, the
//! SHA-256 keying, the trust store, and the executor all live in `kagi-git`
//! (this crate must stay dependency-free per the layering invariant). The
//! critical precedent is gwq v0.1.0, whose committed `setup_commands` were an
//! arbitrary-code-execution vector: only the `Command` type carries trust.

/// One typed step. `Copy` and `Symlink` have closed side effects and never need
/// trust; `Command` shells out and is the trust-gated type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeStep {
    /// Copy a file from `from` (relative to the main worktree) to `to`
    /// (relative to the new worktree). Never overwrites an existing dest.
    Copy { from: String, to: String },
    /// Create a symlink at `to` (in the new worktree) pointing at the absolute
    /// path of `from` (in the main worktree). Never overwrites; `.git` excluded.
    Symlink { from: String, to: String },
    /// Run a subprocess (argv-split, never a shell). **Requires trust.**
    Command { run: String },
}

impl WorktreeStep {
    /// The config `type` keyword — used when enumerating steps in the plan.
    pub fn kind(&self) -> &'static str {
        match self {
            WorktreeStep::Copy { .. } => "copy",
            WorktreeStep::Symlink { .. } => "symlink",
            WorktreeStep::Command { .. } => "command",
        }
    }

    /// Only `Command` requires trust (it is the ACE vector). `Copy`/`Symlink`
    /// have closed side effects and always run without a trust prompt.
    pub fn needs_trust(&self) -> bool {
        matches!(self, WorktreeStep::Command { .. })
    }

    /// One-line, per-type enumeration for the plan — the whole point of typed
    /// steps is that the plan can say exactly what each step does rather than
    /// "runs 3 shell commands". Any control bytes in user/config-supplied text
    /// are escaped so a hostile committed config cannot spoof the display.
    pub fn describe(&self) -> String {
        match self {
            WorktreeStep::Copy { from, to } => {
                format!(
                    "copy: {} → {}",
                    escape_control_bytes(from),
                    escape_control_bytes(to)
                )
            }
            WorktreeStep::Symlink { from, to } => format!(
                "symlink: {} → {}",
                escape_control_bytes(to),
                escape_control_bytes(from)
            ),
            WorktreeStep::Command { run } => {
                format!("command (needs trust): {}", escape_control_bytes(run))
            }
        }
    }
}

/// The two step phases parsed from `.kagi/worktree.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeSteps {
    pub post_create: Vec<WorktreeStep>,
    pub pre_remove: Vec<WorktreeStep>,
}

impl WorktreeSteps {
    pub fn is_empty(&self) -> bool {
        self.post_create.is_empty() && self.pre_remove.is_empty()
    }

    /// True when `post_create` contains at least one trust-gated command.
    pub fn post_create_needs_trust(&self) -> bool {
        self.post_create.iter().any(WorktreeStep::needs_trust)
    }

    /// True when `pre_remove` contains at least one trust-gated command.
    pub fn pre_remove_needs_trust(&self) -> bool {
        self.pre_remove.iter().any(WorktreeStep::needs_trust)
    }
}

/// Escape ASCII control bytes (C0 range `0x00–0x1F` and `DEL` `0x7F`) as
/// `\xHH`, leaving all printable and non-ASCII text intact. Applied to any
/// config-supplied string before it is shown to the user, so a committed
/// `.kagi/worktree.toml` cannot use escape sequences / newlines to spoof the
/// trust prompt (issue #341 §5; same threat as #356's `sanitize_control_bytes`,
/// inlined here since that helper is not yet in the tree).
pub fn escape_control_bytes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let c = ch as u32;
        if c < 0x20 || c == 0x7f {
            out.push_str(&format!("\\x{:02x}", c));
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_command_needs_trust() {
        assert!(!WorktreeStep::Copy {
            from: "a".into(),
            to: "b".into()
        }
        .needs_trust());
        assert!(!WorktreeStep::Symlink {
            from: "a".into(),
            to: "b".into()
        }
        .needs_trust());
        assert!(WorktreeStep::Command {
            run: "npm ci".into()
        }
        .needs_trust());
    }

    #[test]
    fn describe_lists_each_step_by_type() {
        assert_eq!(
            WorktreeStep::Copy {
                from: ".env.example".into(),
                to: ".env".into()
            }
            .describe(),
            "copy: .env.example → .env"
        );
        assert_eq!(
            WorktreeStep::Symlink {
                from: ".claude".into(),
                to: ".claude".into()
            }
            .describe(),
            "symlink: .claude → .claude"
        );
        assert_eq!(
            WorktreeStep::Command {
                run: "npm ci".into()
            }
            .describe(),
            "command (needs trust): npm ci"
        );
    }

    #[test]
    fn escape_neutralizes_control_bytes() {
        // A committed config that tries to inject a fake "trusted" line via a
        // carriage return / escape sequence is neutralized.
        assert_eq!(
            escape_control_bytes("npm ci\r\x1b[2Krm -rf ~"),
            "npm ci\\x0d\\x1b[2Krm -rf ~"
        );
        assert_eq!(escape_control_bytes("plain text"), "plain text");
        // Non-ASCII passes through untouched.
        assert_eq!(escape_control_bytes("café 日本語"), "café 日本語");
        // DEL is escaped.
        assert_eq!(escape_control_bytes("a\x7fb"), "a\\x7fb");
    }

    #[test]
    fn describe_escapes_control_bytes() {
        let d = WorktreeStep::Command {
            run: "echo\x00danger".into(),
        }
        .describe();
        assert_eq!(d, "command (needs trust): echo\\x00danger");
    }

    #[test]
    fn needs_trust_aggregation() {
        let steps = WorktreeSteps {
            post_create: vec![
                WorktreeStep::Copy {
                    from: "a".into(),
                    to: "b".into(),
                },
                WorktreeStep::Command { run: "x".into() },
            ],
            pre_remove: vec![WorktreeStep::Symlink {
                from: "a".into(),
                to: "b".into(),
            }],
        };
        assert!(steps.post_create_needs_trust());
        assert!(!steps.pre_remove_needs_trust());
    }
}
