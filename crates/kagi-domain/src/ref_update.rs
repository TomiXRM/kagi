//! `git update-ref --stdin` lines as data (#344, ADR-0211 決定 4).
//!
//! `git replay` (and `git history --dry-run`) print the refs they *would*
//! move as `update <ref> <new> <old>` lines. Parsed here they become the
//! plan's preview; serialised back they are the exact bytes the execute step
//! feeds to `git update-ref --stdin`, so what the user confirmed is what git
//! applies — with `<old>` doubling as a compare-and-swap git enforces itself.

/// One `update` line: move `reference` from `old` to `new`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefUpdate {
    /// Full ref name, e.g. `refs/heads/feature`.
    pub reference: String,
    /// New object id (40 hex).
    pub new: String,
    /// Expected current object id (40 hex); `update-ref` refuses if it differs.
    pub old: String,
}

/// Why a line could not be read as a ref update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefUpdateParseError {
    /// Not `update <ref> <new> <old>`.
    Malformed { line: String },
    /// A known but unsupported verb (`create`, `delete`, `verify`, …).
    UnsupportedVerb { verb: String, line: String },
    /// An object id that is not 40 hex digits.
    BadOid { oid: String, line: String },
}

impl std::fmt::Display for RefUpdateParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed { line } => write!(f, "unrecognised ref update line: {line:?}"),
            Self::UnsupportedVerb { verb, line } => {
                write!(f, "unsupported ref update verb {verb:?}: {line:?}")
            }
            Self::BadOid { oid, line } => write!(f, "bad object id {oid:?} in {line:?}"),
        }
    }
}

fn is_oid(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Parse every non-empty line of `git replay --ref-action=print` /
/// `git history --dry-run` output. Empty output is `Ok(vec![])` (nothing to
/// move), which callers treat as a no-op, never as success.
pub fn parse_update_ref_lines(output: &str) -> Result<Vec<RefUpdate>, RefUpdateParseError> {
    let mut updates = Vec::new();
    for raw in output.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let verb = parts.next().unwrap_or_default();
        if verb != "update" {
            return Err(RefUpdateParseError::UnsupportedVerb {
                verb: verb.to_string(),
                line: line.to_string(),
            });
        }
        let (Some(reference), Some(new), Some(old), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(RefUpdateParseError::Malformed {
                line: line.to_string(),
            });
        };
        for oid in [new, old] {
            if !is_oid(oid) {
                return Err(RefUpdateParseError::BadOid {
                    oid: oid.to_string(),
                    line: line.to_string(),
                });
            }
        }
        updates.push(RefUpdate {
            reference: reference.to_string(),
            new: new.to_string(),
            old: old.to_string(),
        });
    }
    Ok(updates)
}

/// The `git update-ref --stdin` transaction that applies `updates` all or
/// nothing: `start`, one `update` per ref with its expected old value, then
/// `commit`.
pub fn update_ref_transaction(updates: &[RefUpdate]) -> String {
    let mut out = String::from("start\n");
    for u in updates {
        out.push_str(&format!("update {} {} {}\n", u.reference, u.new, u.old));
    }
    out.push_str("commit\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const C: &str = "cccccccccccccccccccccccccccccccccccccccc";

    #[test]
    fn parses_one_and_many_refs() {
        let one = parse_update_ref_lines(&format!("update refs/heads/feat {A} {B}\n")).unwrap();
        assert_eq!(
            one,
            vec![RefUpdate {
                reference: "refs/heads/feat".into(),
                new: A.into(),
                old: B.into()
            }]
        );
        // `--update-refs=branches` / `--contained`: descendant branches too.
        let many = parse_update_ref_lines(&format!(
            "update refs/heads/feat {A} {B}\nupdate refs/heads/feat-child {C} {A}\n"
        ))
        .unwrap();
        assert_eq!(many.len(), 2);
        assert_eq!(many[1].reference, "refs/heads/feat-child");
    }

    #[test]
    fn empty_output_is_no_updates_not_an_error() {
        assert_eq!(parse_update_ref_lines(""), Ok(vec![]));
        assert_eq!(parse_update_ref_lines("\n\n"), Ok(vec![]));
    }

    #[test]
    fn rejects_other_verbs_and_bad_shapes() {
        assert!(matches!(
            parse_update_ref_lines(&format!("delete refs/heads/feat {A}")),
            Err(RefUpdateParseError::UnsupportedVerb { .. })
        ));
        assert!(matches!(
            parse_update_ref_lines(&format!("update refs/heads/feat {A}")),
            Err(RefUpdateParseError::Malformed { .. })
        ));
        assert!(matches!(
            parse_update_ref_lines(&format!("update refs/heads/feat {A} {B} extra")),
            Err(RefUpdateParseError::Malformed { .. })
        ));
        assert!(matches!(
            parse_update_ref_lines("update refs/heads/feat abc def"),
            Err(RefUpdateParseError::BadOid { .. })
        ));
        assert!(matches!(
            parse_update_ref_lines("fatal: replaying merge commits is not supported yet!"),
            Err(RefUpdateParseError::UnsupportedVerb { .. })
        ));
    }

    #[test]
    fn transaction_round_trips_what_was_parsed() {
        let text = format!("update refs/heads/feat {A} {B}\nupdate refs/heads/x {C} {A}\n");
        let updates = parse_update_ref_lines(&text).unwrap();
        assert_eq!(
            update_ref_transaction(&updates),
            format!("start\n{text}commit\n")
        );
    }
}
