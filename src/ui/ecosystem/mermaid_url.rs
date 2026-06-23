//! mermaid.live deep-link building for the Coupling Mermaid sub-view (ADR-0119).
//!
//! Pure string work (no gpui / git), kept out of `mod.rs` so the entity module
//! stays focused on view state. The diagram travels entirely in the URL
//! fragment as URL-safe base64 of the editor's state JSON — mermaid.live decodes
//! it client-side (the fragment is never sent to a server), so nothing leaks.

/// Build a `https://mermaid.live/edit#base64:…` URL that pre-loads `code`.
/// The editor state is JSON (`code` + a minimal `mermaid` config) encoded as
/// URL-safe, unpadded base64 — the `base64:` (uncompressed) variant the live
/// editor accepts, so no deflate dependency is needed.
pub(super) fn mermaid_live_url(code: &str) -> String {
    let state = format!(
        "{{\"code\":\"{}\",\"mermaid\":\"{{\\n  \\\"theme\\\": \\\"default\\\"\\n}}\",\"autoSync\":true,\"updateDiagram\":true}}",
        json_escape(code)
    );
    format!(
        "https://mermaid.live/edit#base64:{}",
        base64_url_nopad(state.as_bytes())
    )
}

/// Escape a string for embedding inside a JSON `"…"` literal.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// URL-safe base64 (RFC 4648 §5) without padding — matches js-base64's
/// `toBase64(state, true)` that the mermaid live editor decodes.
fn base64_url_nopad(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(T[((n >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(T[(n & 63) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_url_matches_known_vectors() {
        assert_eq!(base64_url_nopad(b""), "");
        assert_eq!(base64_url_nopad(b"f"), "Zg");
        assert_eq!(base64_url_nopad(b"fo"), "Zm8");
        assert_eq!(base64_url_nopad(b"foo"), "Zm9v");
        assert_eq!(base64_url_nopad(b"foob"), "Zm9vYg");
        // URL-safe alphabet: bytes that would yield '+' and '/' in std base64.
        assert_eq!(base64_url_nopad(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn url_has_prefix_and_encodes_code() {
        let url = mermaid_live_url("flowchart LR\n  a --> b\n");
        assert!(url.starts_with("https://mermaid.live/edit#base64:"));
        // The payload must be non-empty and contain only URL-safe base64 chars.
        let payload = url.rsplit(':').next().unwrap();
        assert!(!payload.is_empty());
        assert!(payload
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn json_escape_handles_quotes_and_newlines() {
        assert_eq!(json_escape("a\"b\nc"), "a\\\"b\\nc");
    }
}
