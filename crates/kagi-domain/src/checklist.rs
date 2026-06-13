//! Commit checklist rule logic — pure byte/path classification, no git2.
//!
//! The git2-backed wrapper that reads staged index BLOBs lives in the
//! git-backend layer (`kagi::git::checklist`).

use std::path::Path;

// ────────────────────────────────────────────────────────────
// Constants / thresholds
// ────────────────────────────────────────────────────────────

/// Maximum number of bytes scanned for conflict markers per staged BLOB.
/// ADR-0043 §rule 4 ("先頭 N、例 1MiB").
const MARKER_SCAN_BYTES: usize = 1024 * 1024; // 1 MiB

/// Maximum number of bytes scanned for secret content per staged BLOB.
/// ADR-0043 §rule 5 ("先頭数 KiB").
const SECRET_SCAN_BYTES: usize = 8 * 1024; // 8 KiB

/// Default large-binary threshold (5 MiB).
pub const DEFAULT_LARGE_BLOB_BYTES: u64 = 5 * 1024 * 1024;

/// Number of leading bytes inspected by the NUL-byte binary heuristic.
const NUL_PROBE_BYTES: usize = 8 * 1024;

// ────────────────────────────────────────────────────────────
// Public pure entry points
// ────────────────────────────────────────────────────────────

/// Run the staged-content checklist rules for one staged file's content and
/// return `(blockers, warnings)`.
///
/// `is_binary` is supplied by the git backend because libgit2's BLOB heuristic
/// must stay outside the domain crate. This function performs the pure rule
/// classification and scans only bounded prefixes for text content.
pub fn evaluate_staged_blob(
    path: &Path,
    content: &[u8],
    is_binary: bool,
    large_threshold: u64,
) -> (Vec<String>, Vec<String>) {
    let mut blockers = Vec::new();
    let mut warnings = evaluate_staged_path(path);

    let (content_blockers, content_warnings) =
        evaluate_staged_blob_content(path, content, is_binary, large_threshold);
    blockers.extend(content_blockers);
    warnings.extend(content_warnings);

    (blockers, warnings)
}

/// Run path-only staged checklist rules and return warnings.
///
/// These rules do not require a readable BLOB, so the git backend runs them
/// even for deletions and gitlinks.
pub fn evaluate_staged_path(path: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    if path_is_secret_name(path) {
        warnings.push(format!(
            "Possible secret file staged: {} — confirm before committing.",
            path.to_string_lossy()
        ));
    }
    warnings
}

/// Run content-dependent staged checklist rules for one staged BLOB and return
/// `(blockers, warnings)`.
pub fn evaluate_staged_blob_content(
    path: &Path,
    content: &[u8],
    is_binary: bool,
    large_threshold: u64,
) -> (Vec<String>, Vec<String>) {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    let path_str = path.to_string_lossy();
    let total_len = content.len() as u64;

    // Rule 6 — large binary (warn).  Binary only; text large files skip.
    if is_binary && total_len > large_threshold {
        warnings.push(format!(
            "Large binary file staged: {} ({}). Confirm before committing.",
            path_str,
            human_bytes(total_len)
        ));
    }

    // Content rules only apply to text BLOBs.
    if is_binary {
        return (blockers, warnings);
    }

    // Rule 4 — conflict marker (block).
    let marker_scan = &content[..content.len().min(MARKER_SCAN_BYTES)];
    if has_conflict_marker(marker_scan) {
        blockers.push(format!(
            "Conflict marker found in staged file: {}. \
             Resolve the merge conflict before committing.",
            path_str
        ));
    }

    // Rule 5b — secret by content (warn).
    let secret_scan = &content[..content.len().min(SECRET_SCAN_BYTES)];
    if content_has_secret(secret_scan) {
        warnings.push(format!(
            "Possible secret content in staged file: {} — confirm before committing.",
            path_str
        ));
    }

    (blockers, warnings)
}

/// Return `true` if `text` contains a git conflict marker line
/// (`<<<<<<< ` / `=======` / `>>>>>>> `), reusing the same line-oriented
/// detection as the staged-content checklist (ADR-0043 §rule 4).
///
/// This is the public entry point shared with the conflict-resolution buffer
/// (ADR-0057 marker-residue gate) so both paths agree on what counts as a
/// residual marker.  Operates on a `&str` because the resolution buffer holds
/// decoded text; the scan compares only ASCII marker prefixes so it is
/// UTF-8-safe.
pub fn text_has_conflict_marker(text: &str) -> bool {
    has_conflict_marker(text.as_bytes())
}

/// Return `true` if `content` contains a NUL byte in the leading probe window.
///
/// This is the pure fallback heuristic used by the git backend's
/// `blob_is_binary` wrapper after consulting libgit2.
pub fn content_looks_binary(content: &[u8]) -> bool {
    let probe = &content[..content.len().min(NUL_PROBE_BYTES)];
    probe.contains(&0u8)
}

// ────────────────────────────────────────────────────────────
// Rule 4 — conflict markers
// ────────────────────────────────────────────────────────────

/// Return `true` if `bytes` contains a line whose start matches a git conflict
/// marker: `<<<<<<< ` / `=======` / `>>>>>>> ` (ADR-0043 §rule 4).
///
/// Scanning is line-oriented over bytes so UTF-8 boundaries are never split:
/// we only compare ASCII marker prefixes, which are 7 identical ASCII bytes.
fn has_conflict_marker(bytes: &[u8]) -> bool {
    for line in split_lines(bytes) {
        if line_is_conflict_marker(line) {
            return true;
        }
    }
    false
}

/// Test a single line (without trailing newline) for a conflict-marker start.
///
/// - `<<<<<<< ` — 7 `<` then a space (start of "ours")
/// - `>>>>>>> ` — 7 `>` then a space (start of "theirs")
/// - `=======`  — exactly 7 `=` as the whole line, or `======= ` followed by
///   more (the divider).  Matching the bare 7-`=` line is the ADR-0043 rule.
fn line_is_conflict_marker(line: &[u8]) -> bool {
    is_marker_run(line, b'<') || is_marker_run(line, b'>') || is_equals_marker(line)
}

/// `byte` repeated exactly 7 times followed by an ASCII space.
fn is_marker_run(line: &[u8], byte: u8) -> bool {
    line.len() >= 8 && line[..7].iter().all(|&b| b == byte) && line[7] == b' '
}

/// A `=======` divider: exactly 7 `=` as the whole line, or 7 `=` followed by a
/// space and (optionally) more text.
fn is_equals_marker(line: &[u8]) -> bool {
    if line.len() < 7 || !line[..7].iter().all(|&b| b == b'=') {
        return false;
    }
    // Whole line is exactly 7 `=` → divider.
    if line.len() == 7 {
        return true;
    }
    // `======= ...` (8th char is a space) → divider with trailing label.
    line[7] == b' '
}

// ────────────────────────────────────────────────────────────
// Rule 5 — secret / .env detection
// ────────────────────────────────────────────────────────────

/// Return `true` if the staged path's **file name** looks like a secret
/// (ADR-0043 §rule 5 file-name heuristics).
fn path_is_secret_name(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };
    let lower = name.to_lowercase();

    // `.env` and `.env.*`, excluding example/sample/template variants.
    if lower == ".env" || lower.starts_with(".env.") {
        if lower == ".env.example" || lower == ".env.sample" || lower == ".env.template" {
            return false;
        }
        return true;
    }

    // Exact-name private keys / credentials.
    if name == "id_rsa" || name == "id_ed25519" || lower == "credentials" {
        return true;
    }

    // `secrets.*` (any extension) and a bare `secrets`.
    if lower == "secrets" || lower.starts_with("secrets.") {
        return true;
    }

    // Extension-based: *.pem / *.key / *.pfx / *.p12.
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_lowercase();
        if matches!(ext_lower.as_str(), "pem" | "key" | "pfx" | "p12") {
            return true;
        }
    }

    false
}

/// Return `true` if the scanned BLOB prefix contains secret-looking content
/// (ADR-0043 §rule 5 content heuristics): a PRIVATE KEY header, an AWS access
/// key id (`AKIA…`), or a known token prefix (`ghp_` / `xoxb-`).
///
/// Conservative by design to avoid warning spam on ordinary code.
fn content_has_secret(bytes: &[u8]) -> bool {
    // Work on a lossy string view; secret patterns are ASCII so this is safe.
    let text = String::from_utf8_lossy(bytes);

    if text.contains("-----BEGIN ") && text.contains("PRIVATE KEY-----") {
        return true;
    }
    if contains_aws_access_key(&text) {
        return true;
    }
    if text.contains("ghp_") || text.contains("xoxb-") {
        return true;
    }
    false
}

/// Detect an AWS access key id: `AKIA` followed by exactly 16 uppercase
/// alphanumerics. Hand-rolled (no regex), `chars()`-based.
fn contains_aws_access_key(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i + 4 <= n {
        if chars[i] == 'A' && chars[i + 1] == 'K' && chars[i + 2] == 'I' && chars[i + 3] == 'A' {
            // Need 16 following uppercase [A-Z0-9].
            let mut count = 0;
            let mut j = i + 4;
            while j < n && count < 16 {
                let c = chars[j];
                if c.is_ascii_uppercase() || c.is_ascii_digit() {
                    count += 1;
                    j += 1;
                } else {
                    break;
                }
            }
            if count == 16 {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ────────────────────────────────────────────────────────────
// Byte / line utilities
// ────────────────────────────────────────────────────────────

/// Split `bytes` into lines on `\n`, stripping a trailing `\r` so CRLF files
/// match the same markers as LF files.  Does not allocate per line.
fn split_lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes.split(|&b| b == b'\n').map(|line| {
        if let [rest @ .., b'\r'] = line {
            rest
        } else {
            line
        }
    })
}

/// Format a byte count as a short human-readable string (e.g. `6.0 MiB`).
fn human_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{} B", bytes)
    }
}

// ────────────────────────────────────────────────────────────
// Unit tests (pure helpers; BLOB-level behaviour covered by integration tests
// against real tempdir repos)
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn conflict_marker_lines() {
        assert!(line_is_conflict_marker(b"<<<<<<< HEAD"));
        assert!(line_is_conflict_marker(b">>>>>>> feature"));
        assert!(line_is_conflict_marker(b"======="));
        assert!(line_is_conflict_marker(b"======= label"));
        // Not markers:
        assert!(!line_is_conflict_marker(b"<<<<<< only six"));
        assert!(!line_is_conflict_marker(b"<<<<<<<no space"));
        assert!(!line_is_conflict_marker(b"====== six equals"));
        assert!(!line_is_conflict_marker(b"normal code line"));
        // Markdown-ish 8 equals is not a 7-equals divider.
        assert!(!line_is_conflict_marker(b"========"));
    }

    #[test]
    fn conflict_marker_in_text() {
        let text =
            b"fn main() {\n<<<<<<< HEAD\nlet a = 1;\n=======\nlet a = 2;\n>>>>>>> other\n}\n";
        assert!(has_conflict_marker(text));

        let clean = b"fn main() {\nlet a = 1;\n}\n";
        assert!(!has_conflict_marker(clean));
    }

    #[test]
    fn secret_file_names() {
        assert!(path_is_secret_name(&PathBuf::from(".env")));
        assert!(path_is_secret_name(&PathBuf::from(
            "config/.env.production"
        )));
        assert!(path_is_secret_name(&PathBuf::from("id_rsa")));
        assert!(path_is_secret_name(&PathBuf::from("keys/server.pem")));
        assert!(path_is_secret_name(&PathBuf::from("server.key")));
        assert!(path_is_secret_name(&PathBuf::from("cert.pfx")));
        assert!(path_is_secret_name(&PathBuf::from("cert.p12")));
        assert!(path_is_secret_name(&PathBuf::from("credentials")));
        assert!(path_is_secret_name(&PathBuf::from("secrets.yaml")));
        // Excluded / non-secret:
        assert!(!path_is_secret_name(&PathBuf::from(".env.example")));
        assert!(!path_is_secret_name(&PathBuf::from(".env.sample")));
        assert!(!path_is_secret_name(&PathBuf::from(".env.template")));
        assert!(!path_is_secret_name(&PathBuf::from("src/main.rs")));
        assert!(!path_is_secret_name(&PathBuf::from("README.md")));
    }

    #[test]
    fn secret_content() {
        assert!(content_has_secret(
            b"-----BEGIN RSA PRIVATE KEY-----\nMIIE..."
        ));
        assert!(content_has_secret(b"-----BEGIN OPENSSH PRIVATE KEY-----\n"));
        assert!(content_has_secret(b"aws_key = AKIAIOSFODNN7EXAMPLE\n"));
        assert!(content_has_secret(b"token: ghp_abcdefghijklmnop\n"));
        assert!(content_has_secret(b"slack: xoxb-123-456\n"));
        // Non-secret ordinary content:
        assert!(!content_has_secret(b"let x = 42;\nfn helper() {}\n"));
        assert!(!content_has_secret(b"AKIA but too short tail\n"));
    }

    #[test]
    fn human_bytes_fmt() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(6 * 1024 * 1024), "6.0 MiB");
    }

    #[test]
    fn crlf_lines_match_markers() {
        let text = b"a\r\n<<<<<<< HEAD\r\nb\r\n";
        assert!(has_conflict_marker(text));
    }

    #[test]
    fn nul_probe_marks_binary() {
        assert!(content_looks_binary(b"abc\0def"));
        assert!(!content_looks_binary(b"plain text"));
    }
}
