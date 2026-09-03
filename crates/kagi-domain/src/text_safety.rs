//! Text safety — issue #356.
//!
//! Pure, dependency-free neutralization and detection of dangerous codepoints
//! in text that originates from an **untrusted remote** (commit messages, PR
//! bodies and review comments, branch / tag names, trailer values, worktree
//! paths and lock reasons, …). Two independent concerns:
//!
//! * [`sanitize_control_bytes`] — rewrite terminal / display control bytes
//!   (C0 except TAB/LF, the C1 range, and DEL) as visible `\xHH` escapes so a
//!   crafted string cannot move the cursor, clear the screen, or inject ANSI
//!   colours into a text-display path. Everything printable — ASCII, CJK,
//!   emoji, combining marks — is preserved verbatim (an acceptance criterion).
//!
//! * [`scan_unsafe_unicode`] — locate bidirectional-control (the "Trojan
//!   Source" family) and zero-width / invisible codepoints so the UI can flag
//!   the affected row. This scan does **not** rewrite text; the characters may
//!   be legitimate, so the decision to warn (not mutate) is deliberate.
//!
//! No homoglyph detection (scope §5, PM-locked). Detection is hand-written
//! codepoint arithmetic — no `unicode-bidi`, keeping `kagi-domain` dependency
//! -free.

// ──────────────────────────────────────────────────────────────
// Control-byte sanitization
// ──────────────────────────────────────────────────────────────

/// True for a control byte that must never reach a text-display path: the C0
/// range (U+0000–U+001F) **except** TAB and LF, the DEL character (U+007F),
/// and the C1 range (U+0080–U+009F, which includes the ANSI CSI byte 0x9B).
/// CR (U+000D) is intentionally *not* excepted — a bare CR can rewind the
/// cursor to the start of the line in a terminal, so it is escaped.
fn is_display_control(c: char) -> bool {
    let u = c as u32;
    (u < 0x20 && c != '\t' && c != '\n') || u == 0x7F || (0x80..=0x9F).contains(&u)
}

/// Rewrite every display-control byte in `s` as a visible `\xHH` escape,
/// preserving all other characters (printable ASCII, CJK, emoji, combining
/// marks, TAB, LF) exactly.
///
/// Returns an owned `String`. When `s` contains no control bytes the input is
/// cloned unchanged (the common case), so callers can wrap freely.
pub fn sanitize_control_bytes(s: &str) -> String {
    if !s.chars().any(is_display_control) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if is_display_control(c) {
            // All targeted codepoints are <= 0x9F, so two hex digits suffice.
            out.push_str(&format!("\\x{:02X}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

// ──────────────────────────────────────────────────────────────
// Unsafe-Unicode detection (bidi + zero-width)
// ──────────────────────────────────────────────────────────────

/// Category of a codepoint flagged by [`scan_unsafe_unicode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsafeKind {
    /// Bidirectional formatting control — U+202A–202E (LRE/RLE/PDF/LRO/RLO)
    /// and U+2066–2069 (LRI/RLI/FSI/PDI). The "Trojan Source" family: can
    /// reorder rendered text so it reads differently from its byte order.
    Bidi,
    /// Zero-width / invisible — U+200B–200D (ZWSP/ZWNJ/ZWJ) and U+FEFF (BOM /
    /// zero-width no-break space). Can hide or splice tokens invisibly.
    ZeroWidth,
}

/// One flagged codepoint located by byte offset within the scanned string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsafeMark {
    /// Byte offset of the character within the scanned `&str`.
    pub offset: usize,
    /// The flagged character.
    pub ch: char,
    /// Which category the character falls into.
    pub kind: UnsafeKind,
}

/// Classify a single character, or `None` if it is safe.
fn classify(c: char) -> Option<UnsafeKind> {
    match c {
        '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' => Some(UnsafeKind::Bidi),
        '\u{200B}'..='\u{200D}' | '\u{FEFF}' => Some(UnsafeKind::ZeroWidth),
        _ => None,
    }
}

/// Locate every bidi-control and zero-width codepoint in `s`, in order, each
/// tagged with its byte offset and category. Empty when `s` is safe.
pub fn scan_unsafe_unicode(s: &str) -> Vec<UnsafeMark> {
    s.char_indices()
        .filter_map(|(offset, ch)| classify(ch).map(|kind| UnsafeMark { offset, ch, kind }))
        .collect()
}

/// Cheap predicate: does `s` contain any unsafe codepoint? Use in per-frame
/// render paths where allocating the full [`UnsafeMark`] list is wasteful.
pub fn has_unsafe_unicode(s: &str) -> bool {
    s.chars().any(|c| classify(c).is_some())
}

// ──────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── sanitize_control_bytes ──────────────────────────────────

    #[test]
    fn escapes_ansi_escape_and_bell() {
        // ESC (0x1B) + bell (0x07) — the classic ANSI-injection vector.
        let s = "hello\x1b[31mred\x07";
        let out = sanitize_control_bytes(s);
        assert_eq!(out, "hello\\x1B[31mred\\x07");
        assert!(!out.contains('\x1b'));
        assert!(!out.contains('\x07'));
    }

    #[test]
    fn escapes_bare_cr_but_keeps_tab_and_lf() {
        let out = sanitize_control_bytes("a\tb\nc\rd");
        assert_eq!(out, "a\tb\nc\\x0Dd");
    }

    #[test]
    fn escapes_del_and_c1() {
        // DEL (0x7F) and a C1 control (0x9B = ANSI CSI).
        let out = sanitize_control_bytes("x\u{7F}y\u{9B}z");
        assert_eq!(out, "x\\x7Fy\\x9Bz");
    }

    #[test]
    fn preserves_cjk_and_emoji() {
        // Acceptance: legitimate multibyte text must pass through untouched.
        let s = "日本語テスト 🎉👨‍👩‍👧 café";
        assert_eq!(sanitize_control_bytes(s), s);
    }

    #[test]
    fn plain_ascii_unchanged() {
        let s = "a normal branch-name/v1.2.3";
        assert_eq!(sanitize_control_bytes(s), s);
    }

    #[test]
    fn empty_string_unchanged() {
        assert_eq!(sanitize_control_bytes(""), "");
    }

    // ── scan_unsafe_unicode ─────────────────────────────────────

    #[test]
    fn detects_bidi_override() {
        // RLO (U+202E) — the Trojan-Source reordering character.
        let marks = scan_unsafe_unicode("if (x)\u{202E} // safe");
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].kind, UnsafeKind::Bidi);
        assert_eq!(marks[0].ch, '\u{202E}');
        // Offset must be a valid byte boundary pointing at the char.
        assert_eq!(marks[0].offset, "if (x)".len());
    }

    #[test]
    fn detects_isolate_range() {
        // FSI (U+2068) and PDI (U+2069) from the isolate family.
        let marks = scan_unsafe_unicode("a\u{2068}b\u{2069}c");
        assert_eq!(marks.len(), 2);
        assert!(marks.iter().all(|m| m.kind == UnsafeKind::Bidi));
    }

    #[test]
    fn detects_zero_width() {
        // ZWSP, ZWNJ, ZWJ, BOM.
        let marks = scan_unsafe_unicode("a\u{200B}b\u{200C}c\u{200D}d\u{FEFF}e");
        assert_eq!(marks.len(), 4);
        assert!(marks.iter().all(|m| m.kind == UnsafeKind::ZeroWidth));
    }

    #[test]
    fn plain_ascii_has_no_marks() {
        assert!(scan_unsafe_unicode("perfectly normal text").is_empty());
        assert!(!has_unsafe_unicode("perfectly normal text"));
    }

    #[test]
    fn cjk_and_emoji_are_not_flagged() {
        // ZWJ inside an emoji sequence IS flagged (it is a real U+200D), but
        // ordinary CJK / emoji without joiners must not be.
        assert!(!has_unsafe_unicode("日本語 café 🎉"));
        assert!(scan_unsafe_unicode("日本語 café 🎉").is_empty());
    }

    #[test]
    fn has_unsafe_matches_scan() {
        let s = "clean\u{202E}dirty";
        assert_eq!(has_unsafe_unicode(s), !scan_unsafe_unicode(s).is_empty());
    }
}
