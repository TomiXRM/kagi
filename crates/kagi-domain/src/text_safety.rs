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

/// Append the display-safe form directly into an existing label. Unlike
/// `sanitize_control_bytes`, clean input needs no intermediate allocation.
pub fn append_sanitized_control_bytes(out: &mut String, s: &str) {
    if !s.chars().any(is_display_control) {
        out.push_str(s);
        return;
    }
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for c in s.chars() {
        if is_display_control(c) {
            let byte = c as u8;
            out.push('\\');
            out.push('x');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0xF) as usize] as char);
        } else {
            out.push(c);
        }
    }
}

/// Rewrite every display-control byte in `s` as a visible `\xHH` escape,
/// preserving all other characters (printable ASCII, CJK, emoji, combining
/// marks, TAB, LF) exactly.
pub fn sanitize_control_bytes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    append_sanitized_control_bytes(&mut out, s);
    out
}

/// Complete display subject, except for an explicit 4 KiB UTF-8-safe bound.
/// Bound the input work before escaping controls, which can expand bytes.
pub fn display_subject(subject: &str) -> String {
    const MAX_BYTES: usize = 4096;
    let mut input_end = subject.len().min(MAX_BYTES);
    while !subject.is_char_boundary(input_end) {
        input_end -= 1;
    }
    let mut safe = sanitize_control_bytes(&subject[..input_end]);
    if input_end < subject.len() || safe.len() > MAX_BYTES {
        let mut end = MAX_BYTES - '…'.len_utf8();
        while !safe.is_char_boundary(end) {
            end -= 1;
        }
        safe.truncate(end);
        safe.push('…');
    }
    safe
}

/// The first diagnostic line, not a trailing stack frame or blank line.
/// Apply the same display-control escaping and UTF-8 bound as other subjects.
/// No diagnostic is distinct from a renderer-provided explanation.
pub fn diagnostic_summary(stderr: &str) -> Option<String> {
    stderr
        .trim_start()
        .lines()
        .next()
        .map(|line| display_subject(line.trim_end()))
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

    #[test]
    fn diagnostic_summary_keeps_the_cause_before_stack_frames_and_blank_lines() {
        for newline in ["\n", "\r\n"] {
            let stderr = format!(
                "{newline}Error: Could not find chrome-headless-shell.{newline}\
                 Install the required browser.{newline}\
                 at ChromeLauncher.launch (launcher.js:95){newline}{newline}"
            );
            assert_eq!(
                diagnostic_summary(&stderr).as_deref(),
                Some("Error: Could not find chrome-headless-shell.")
            );
        }
        for stderr in ["", "\n\n", " \t\r\n  "] {
            assert_eq!(diagnostic_summary(stderr), None);
        }
        assert_eq!(
            diagnostic_summary("\nError: 図\x1b[31m\n\n").as_deref(),
            Some("Error: 図\\x1B[31m")
        );
    }

    #[test]
    fn display_subject_keeps_complete_identity_until_utf8_byte_bound() {
        for subject in [
            "日😀".repeat(300),
            "safe\x1b[31m".repeat(300),
            "x".repeat(4096),
        ] {
            assert_eq!(display_subject(&subject), sanitize_control_bytes(&subject));
        }
        for subject in ["日😀".repeat(1000), "\x1b".repeat(2000), "x".repeat(4097)] {
            let safe = display_subject(&subject);
            assert!(safe.len() <= 4096);
            assert!(safe.ends_with('…'));
            assert!(!safe.contains('\x1b'));
        }
    }

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

    #[test]
    fn append_sanitized_preserves_prefix_and_neutralizes_controls() {
        let mut out = String::from("branch: ");
        append_sanitized_control_bytes(&mut out, "日本語\x1b[31m \u{9B}\t");
        assert_eq!(out, "branch: 日本語\\x1B[31m \\x9B\t");
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
