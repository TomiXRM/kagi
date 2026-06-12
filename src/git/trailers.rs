//! Commit message trailer parsing — W18-COAUTHOR-COPY
//!
//! Pure, UI-independent helpers that extract structured information from a raw
//! commit message.  Currently this is limited to `Co-authored-by:` trailers,
//! but the module is the natural home for any future trailer parsing.
//!
//! The functions here make **no** network calls and depend only on `std`.

// ──────────────────────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────────────────────

/// A single co-author parsed from a `Co-authored-by:` trailer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoAuthor {
    /// Display name, e.g. `"Alice Example"`.  May be empty when the trailer
    /// only carried an email.
    pub name: String,
    /// Email address, e.g. `"alice@example.com"`.  May be empty when the
    /// trailer carried no `<...>` part.
    pub email: String,
}

// ──────────────────────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────────────────────

/// Parse all `Co-authored-by:` trailers from a raw commit `message`.
///
/// Behaviour:
/// - The trailer **key** match is case-insensitive (`Co-authored-by`,
///   `co-authored-by`, `CO-AUTHORED-BY`, … all match).
/// - Multiple co-authors are supported; the returned `Vec` preserves the order
///   in which the trailers appear in the message.
/// - The value is parsed as `Name <email>`.  When the `<...>` part is absent
///   the whole value becomes the name and the email is empty.  When the value
///   is *only* `<email>` the name is empty.
/// - Leading whitespace before the key is tolerated, but the key must be the
///   first non-whitespace token on the line (so a `Co-authored-by:` appearing
///   mid-prose is not matched).
/// - Duplicate co-authors (same name **and** email, case-insensitive on the
///   email) are de-duplicated, keeping the first occurrence.
/// - Entries with both an empty name **and** an empty email are skipped.
///
/// Returns an empty `Vec` when the message contains no co-author trailers.
///
/// This function is `chars()`-safe: it never slices into the middle of a
/// multi-byte UTF-8 sequence, so CJK names are preserved intact.
pub fn parse_coauthors(message: &str) -> Vec<CoAuthor> {
    const KEY: &str = "co-authored-by:";

    let mut out: Vec<CoAuthor> = Vec::new();

    for line in message.lines() {
        let trimmed = line.trim_start();
        // Case-insensitive prefix match on the trailer key.  We only need to
        // lower-case the key-length prefix, not the whole (potentially long)
        // line, and only when the line is at least key-length.
        if trimmed.len() < KEY.len() {
            continue;
        }
        let (head, value) = trimmed.split_at(KEY.len());
        if !head.eq_ignore_ascii_case(KEY) {
            continue;
        }

        let value = value.trim();
        let (name, email) = split_name_email(value);

        if name.is_empty() && email.is_empty() {
            continue;
        }

        let coauthor = CoAuthor { name, email };

        // De-duplicate on (name, lower-cased email).
        let is_dup = out.iter().any(|existing| {
            existing.name == coauthor.name
                && existing.email.eq_ignore_ascii_case(&coauthor.email)
        });
        if !is_dup {
            out.push(coauthor);
        }
    }

    out
}

// ──────────────────────────────────────────────────────────────
// Internal helpers
// ──────────────────────────────────────────────────────────────

/// Split a `Name <email>` trailer value into `(name, email)`.
///
/// Only splits on the ASCII markers `<` and `>`, never byte-slicing into a
/// multi-byte sequence.  The name is trimmed; the email is taken verbatim
/// between the angle brackets (also trimmed).
fn split_name_email(value: &str) -> (String, String) {
    if let Some(lt) = value.find('<') {
        let name = value[..lt].trim().to_string();
        let rest = &value[lt + 1..];
        let email = match rest.find('>') {
            Some(gt) => rest[..gt].trim().to_string(),
            None => rest.trim().to_string(),
        };
        (name, email)
    } else {
        (value.trim().to_string(), String::new())
    }
}
