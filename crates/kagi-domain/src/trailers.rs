//! Commit message trailer parsing — W18-COAUTHOR-COPY / issue #336
//!
//! Pure, UI-independent helpers that extract structured information from a raw
//! commit message: the narrow [`parse_coauthors`] extractor and the general
//! [`parse_trailers`] block parser that mirrors `git interpret-trailers`.
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
        // Case-insensitive prefix match on the trailer key.  `get` (not
        // `split_at`) so a multi-byte char straddling the key-length byte
        // offset yields None instead of panicking (e.g. Japanese prose lines).
        let head = match trimmed.get(..KEY.len()) {
            Some(h) => h,
            None => continue,
        };
        if !head.eq_ignore_ascii_case(KEY) {
            continue;
        }
        let value = &trimmed[KEY.len()..];

        let value = value.trim();
        let (name, email) = split_name_email(value);

        if name.is_empty() && email.is_empty() {
            continue;
        }

        let coauthor = CoAuthor { name, email };

        // De-duplicate on (name, lower-cased email).
        let is_dup = out.iter().any(|existing| {
            existing.name == coauthor.name && existing.email.eq_ignore_ascii_case(&coauthor.email)
        });
        if !is_dup {
            out.push(coauthor);
        }
    }

    out
}

// ──────────────────────────────────────────────────────────────
// General trailer block parsing (issue #336)
// ──────────────────────────────────────────────────────────────

/// A single parsed trailer: `key: value`.
///
/// `key` never contains whitespace (git's token rule). Continuation lines are
/// unfolded into `value` joined by a single space, matching
/// `git interpret-trailers --parse` (which implies `--unfold`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trailer {
    /// The trailer token, verbatim as written (case preserved), e.g.
    /// `"Co-authored-by"` or `"Amp-Thread-ID"`.
    pub key: String,
    /// The trailer value, control/ANSI bytes NOT yet escaped — call
    /// [`sanitize_trailer_value`] before rendering.
    pub value: String,
}

/// Trailer tokens git recognizes as "generated" by default (case-insensitive).
///
/// This gates the 25% rule below. **Matching git faithfully, only
/// `Signed-off-by` counts by default** — `Co-authored-by`, `Refs`, etc. do NOT
/// rescue a mostly-prose trailing block (verified against
/// `git interpret-trailers` 2.x). A block made entirely of trailers is parsed
/// regardless, which is the common agent-commit case (subject + blank + a pure
/// block of `Amp-Thread-ID:` / `Co-authored-by:` lines).
const KNOWN_TRAILERS: &[&str] = &["signed-off-by"];

/// Parse the trailer block of a raw commit `message` into ordered `(key,
/// value)` pairs, mirroring `git interpret-trailers`.
///
/// Detection rules (git-faithful):
/// - The candidate is the **last block** of non-blank lines, and it must be
///   **preceded by a blank line** (a single-paragraph message has no trailers).
/// - A line is *trailer-form* when it is `token: value` where `token` is
///   non-empty and contains no whitespace.
/// - A line starting with whitespace is a **continuation** of the previous
///   line's value (unfolded with a single space).
/// - Comment lines (`#…`) are ignored.
/// - The block qualifies iff it has ≥1 trailer AND either **every** logical
///   line is a trailer, OR ≥25% are trailers **and** ≥1 is a known/generated
///   trailer ([`KNOWN_TRAILERS`]).
///
/// Duplicate keys (e.g. two `Co-authored-by`) are all retained, in order.
/// Non-trailer lines inside a qualifying block are dropped (as git does).
///
/// Values are returned raw; run them through [`sanitize_trailer_value`] before
/// display.
pub fn parse_trailers(message: &str) -> Vec<Trailer> {
    let lines: Vec<&str> = message.lines().collect();

    // Trim trailing blank lines.
    let mut end = lines.len();
    while end > 0 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    if end == 0 {
        return Vec::new();
    }
    // Walk back to the blank line that separates the trailing block.
    let mut start = end;
    while start > 0 && !lines[start - 1].trim().is_empty() {
        start -= 1;
    }
    // git requires a blank line before the block: a message that is a single
    // paragraph (block starts at line 0) has no trailers.
    if start == 0 {
        return Vec::new();
    }

    // Fold the block into logical lines: (is_trailer, key, value).
    let mut logical: Vec<(bool, String, String)> = Vec::new();
    for &raw in &lines[start..end] {
        let starts_with_ws = raw.starts_with(' ') || raw.starts_with('\t');
        if starts_with_ws {
            // Continuation of the previous logical line (unfold with a space).
            if let Some(last) = logical.last_mut() {
                if !last.2.is_empty() {
                    last.2.push(' ');
                }
                last.2.push_str(raw.trim());
            }
            continue;
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue; // comments / stray blanks are ignored, not counted
        }
        match split_trailer(trimmed) {
            Some((key, value)) => logical.push((true, key, value)),
            None => logical.push((false, String::new(), trimmed.to_string())),
        }
    }

    let total = logical.len();
    let trailer_count = logical.iter().filter(|l| l.0).count();
    if trailer_count == 0 {
        return Vec::new();
    }
    let has_known = logical
        .iter()
        .any(|l| l.0 && KNOWN_TRAILERS.iter().any(|k| l.1.eq_ignore_ascii_case(k)));
    // all-trailer OR (>=25% AND a known trailer present). `t*4 >= total` is the
    // integer form of `t/total >= 0.25` (git uses `t*3 >= non_trailer`).
    let qualifies = trailer_count == total || (has_known && trailer_count * 4 >= total);
    if !qualifies {
        return Vec::new();
    }

    logical
        .into_iter()
        .filter(|l| l.0)
        .map(|(_, key, value)| Trailer { key, value })
        .collect()
}

/// True when `value` is an `http`/`https` URL (used to render it as a link).
pub fn is_url(value: &str) -> bool {
    let v = value.trim();
    v.starts_with("http://") || v.starts_with("https://")
}

/// Escape control / ANSI bytes in a trailer value as `\xHH` so a hostile
/// trailer cannot inject terminal escape sequences (interim guard until #356).
///
/// Legitimate multi-byte UTF-8 (CJK, emoji) is preserved unchanged — only
/// `char::is_control()` code points (C0, DEL, C1) are escaped, byte-by-byte.
pub fn sanitize_trailer_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut buf = [0u8; 4];
    for c in value.chars() {
        if c.is_control() {
            for b in c.encode_utf8(&mut buf).as_bytes() {
                out.push_str(&format!("\\x{b:02x}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Split a `token: value` line. Returns `None` when it is not trailer-form
/// (no separator, empty token, or a token containing whitespace).
fn split_trailer(line: &str) -> Option<(String, String)> {
    let colon = line.find(':')?;
    let key = &line[..colon];
    if key.is_empty() || key.chars().any(char::is_whitespace) {
        return None;
    }
    let value = line[colon + 1..].trim();
    Some((key.to_string(), value.to_string()))
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

// ──────────────────────────────────────────────────────────────
// Tests (issue #336 — golden vs `git interpret-trailers`)
// ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod trailer_tests {
    use super::*;

    fn kv(t: &[Trailer]) -> Vec<(&str, &str)> {
        t.iter()
            .map(|x| (x.key.as_str(), x.value.as_str()))
            .collect()
    }

    #[test]
    fn all_trailers_block() {
        // Golden: interpret-trailers --parse returns both lines.
        let m = "Subject line\n\nCo-authored-by: A <a@x.com>\nSigned-off-by: B <b@x.com>\n";
        assert_eq!(
            kv(&parse_trailers(m)),
            vec![
                ("Co-authored-by", "A <a@x.com>"),
                ("Signed-off-by", "B <b@x.com>"),
            ]
        );
    }

    #[test]
    fn prose_block_not_misdetected() {
        // Golden: nothing for a pure-prose trailing paragraph.
        let m = "Subject\n\nThis is a normal paragraph of prose text.\n\
                 It has multiple lines but is not trailers at all.\n\
                 Just regular writing here folks.\n";
        assert!(parse_trailers(m).is_empty());
    }

    #[test]
    fn twenty_five_percent_with_known_qualifies() {
        // 3 prose + 1 Signed-off-by = 25%, known present. Golden: returns only
        // the Signed-off-by. Guards the >=25%/known rule (mutation: reverting
        // to all-trailer-only makes this return empty).
        let m =
            "Subject\n\nprose one here\nprose two here\nprose three x\nSigned-off-by: B <b@x.com>\n";
        assert_eq!(
            kv(&parse_trailers(m)),
            vec![("Signed-off-by", "B <b@x.com>")]
        );
    }

    #[test]
    fn twenty_five_percent_without_known_rejected() {
        // Same 25% ratio but Co-authored-by is NOT a recognized generated
        // trailer, so nothing is returned. Guards KNOWN_TRAILERS (mutation:
        // making every key "known" makes this return the trailer).
        let m =
            "Subject\n\nprose one here\nprose two here\nprose three x\nCo-authored-by: A <a@x.com>\n";
        assert!(parse_trailers(m).is_empty());
    }

    #[test]
    fn below_threshold_rejected() {
        // 4 prose + 1 SoB = 20% < 25%. Golden: nothing.
        let m = "Subject\n\np1 line\np2 line\np3 line\np4 line\nSigned-off-by: B <b@x.com>\n";
        assert!(parse_trailers(m).is_empty());
    }

    #[test]
    fn duplicate_same_key_retained() {
        let m = "Subject\n\nCo-authored-by: A <a@x.com>\nCo-authored-by: A <a@x.com>\n";
        assert_eq!(
            kv(&parse_trailers(m)),
            vec![
                ("Co-authored-by", "A <a@x.com>"),
                ("Co-authored-by", "A <a@x.com>"),
            ]
        );
    }

    #[test]
    fn continuation_lines_unfolded() {
        // Golden: the indented continuation is unfolded into one space-joined
        // value ("Refs: bar continued value line").
        let m = "Subject\n\nAcked-by: Foo\nRefs: bar\n  continued value line\n";
        assert_eq!(
            kv(&parse_trailers(m)),
            vec![("Acked-by", "Foo"), ("Refs", "bar continued value line")]
        );
    }

    #[test]
    fn single_paragraph_no_blank_before_is_not_trailers() {
        // Golden: nothing (no preceding blank line).
        assert!(parse_trailers("Signed-off-by: B <b@x.com>\n").is_empty());
    }

    #[test]
    fn amp_thread_id_url_recognized() {
        let m = "Subject\n\nAmp-Thread-ID: https://ampcode.com/threads/T-abc-123\n";
        let t = parse_trailers(m);
        assert_eq!(
            kv(&t),
            vec![("Amp-Thread-ID", "https://ampcode.com/threads/T-abc-123")]
        );
        assert!(is_url(&t[0].value));
    }

    #[test]
    fn is_url_negatives() {
        assert!(!is_url("not a url"));
        assert!(!is_url("ftp://example.com"));
        assert!(!is_url("A <a@x.com>"));
        assert!(is_url("http://example.com"));
    }

    #[test]
    fn sanitizer_neutralizes_escape_sequences() {
        // ANSI CSI red-color injection: ESC [ 3 1 m ... — the ESC (0x1b) must
        // be neutralized. Mutation: an identity sanitizer leaves ESC and fails.
        let raw = "value\u{1b}[31mHACKED\u{1b}[0m";
        let out = sanitize_trailer_value(raw);
        assert!(!out.contains('\u{1b}'), "ESC must be escaped: {out:?}");
        assert_eq!(out, "value\\x1b[31mHACKED\\x1b[0m");
    }

    #[test]
    fn sanitizer_preserves_cjk_and_emoji() {
        let raw = "日本語 test 🎉 café";
        assert_eq!(sanitize_trailer_value(raw), raw);
    }
}
