//! GitHub avatar resolution & fetching — W11-AVATAR (ADR-0037)
//!
//! This module is the *non-UI* half of the avatar feature.  It is responsible
//! for turning commit author emails into avatar image bytes for repositories
//! whose remote points at `github.com`.  All UI wiring (the avatar store on
//! `KagiApp`, background spawns, render swaps) lives in `mod.rs`/`inspector.rs`;
//! everything here is pure data + blocking IO that runs on a background thread.
//!
//! Resolution order (ADR-0037):
//!   1. **noreply parse** — `<id>+<user>@users.noreply.github.com` or
//!      `<user>@users.noreply.github.com` → `https://avatars.githubusercontent.com/<user>?s=64`
//!      (no API call, immediate).
//!   2. **Commits API batch** — `GET /repos/{owner}/{repo}/commits?per_page=100`
//!      (unauthenticated, a few pages) builds an `email → avatar_url` map.
//!   3. unresolved → caller falls back to the initial circle.
//!
//! Privacy: the user's email is **never** sent as a search query.  We only send
//! the repo coordinates (owner/repo) and fetch avatar URLs.  `KAGI_OFFLINE=1`
//! disables all network access.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{Image, ImageFormat};

/// Network timeout for a single HTTP request.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Maximum number of Commits-API pages fetched per repo (≤3, see ADR-0037).
const MAX_COMMIT_PAGES: u32 = 3;

/// User-Agent sent with every request (GitHub requires a UA header).
const USER_AGENT: &str = "kagi-git-client";

// ──────────────────────────────────────────────────────────────────────────
// Offline switch
// ──────────────────────────────────────────────────────────────────────────

/// Whether network access is disabled (`KAGI_OFFLINE=1`).
///
/// Headless tests always run with this set so avatar behaviour is deterministic
/// and no requests escape during CI / fixture runs.
pub fn offline() -> bool {
    std::env::var("KAGI_OFFLINE").as_deref() == Ok("1")
}

// ──────────────────────────────────────────────────────────────────────────
// noreply email → username
// ──────────────────────────────────────────────────────────────────────────

/// Parse a GitHub `noreply` commit email into its `username`.
///
/// Recognises both historical formats:
///   * new: `<id>+<user>@users.noreply.github.com`
///   * old: `<user>@users.noreply.github.com`
///
/// Returns `None` for any non-noreply email (real emails are *not* parsed —
/// they go through the Commits API path instead so we never leak them).
///
/// String handling is `chars()`-based (no byte slicing) per the project rule.
pub fn parse_noreply_username(email: &str) -> Option<String> {
    // Case-insensitive domain match; usernames are case-insensitive on GitHub.
    let email = email.trim();
    let at = email.rfind('@')?;
    let (local, domain) = (&email[..at], &email[at + 1..]);
    if !domain.eq_ignore_ascii_case("users.noreply.github.com") {
        return None;
    }
    if local.is_empty() {
        return None;
    }
    // New format carries a numeric id prefix: `<id>+<user>`.
    let user = match local.split_once('+') {
        Some((id, user)) => {
            // The prefix must be all digits to be the id form; otherwise the
            // whole local-part is the username (a `+` can legally appear in a
            // username-only address only via the id form, so a non-numeric
            // prefix means we keep the entire local part).
            if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
                user
            } else {
                local
            }
        }
        None => local,
    };
    if user.is_empty() {
        return None;
    }
    Some(user.to_string())
}

/// Build the avatars CDN URL for a resolved GitHub `username`.
pub fn avatar_url_for_username(username: &str) -> String {
    format!("https://avatars.githubusercontent.com/{username}?s=64")
}

// ──────────────────────────────────────────────────────────────────────────
// remote URL → (owner, repo)
// ──────────────────────────────────────────────────────────────────────────

/// Parse a git remote URL and return `(owner, repo)` when it points at
/// `github.com`.  Returns `None` for any non-GitHub host.
///
/// Handles the common forms:
///   * `https://github.com/owner/repo.git`
///   * `https://github.com/owner/repo`
///   * `git@github.com:owner/repo.git`
///   * `ssh://git@github.com/owner/repo.git`
pub fn github_owner_repo(remote_url: &str) -> Option<(String, String)> {
    let url = remote_url.trim();

    // Locate `github.com` and take everything after the following separator.
    let path = if let Some(rest) = url.strip_prefix("git@github.com:") {
        rest
    } else if let Some(idx) = url.find("github.com") {
        let after = &url[idx + "github.com".len()..];
        // After the host comes either `/` (https/ssh) or `:` (scp-like).
        after
            .strip_prefix('/')
            .or_else(|| after.strip_prefix(':'))?
    } else {
        return None;
    };

    // `owner/repo[.git][/]` → split on '/'.
    let mut parts = path.trim_matches('/').splitn(2, '/');
    let owner = parts.next().filter(|s| !s.is_empty())?;
    let repo_raw = parts.next().filter(|s| !s.is_empty())?;
    // Strip a trailing `.git` and anything after a further slash.
    let repo = repo_raw
        .split('/')
        .next()
        .unwrap_or(repo_raw)
        .strip_suffix(".git")
        .unwrap_or_else(|| repo_raw.split('/').next().unwrap_or(repo_raw));
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

/// Resolve the first GitHub `(owner, repo)` from a repository's remotes.
///
/// Opens the repo read-only through the backend and inspects every remote's fetch URL.
/// Returns `None` if the repo can't be opened or no remote points at GitHub.
pub fn repo_github_coords(repo_path: &std::path::Path) -> Option<(String, String)> {
    let backend = kagi::git::Backend::open(repo_path).ok()?;
    for url in backend.remote_urls().ok()? {
        if let Some(coords) = github_owner_repo(&url) {
            return Some(coords);
        }
    }
    None
}

// ──────────────────────────────────────────────────────────────────────────
// Disk cache
// ──────────────────────────────────────────────────────────────────────────

/// Resolve the avatar cache directory: `$KAGI_LOG_DIR/avatars/` first, then
/// `$HOME/.kagi/avatars/`.  Returns `None` when no base directory is known.
pub fn cache_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("KAGI_LOG_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("avatars"));
        }
    }
    let home = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .filter(|s| !s.is_empty())?;
    Some(PathBuf::from(home).join(".kagi").join("avatars"))
}

/// 64-bit FNV-1a hash, rendered as a stable 16-hex string.
///
/// Used as the on-disk filename for a cached avatar URL.  A self-contained
/// hash avoids pulling in a sha1 crate; collisions across avatar URLs are
/// astronomically unlikely and a collision would at worst show the wrong
/// avatar for one author (never a crash), so cryptographic strength is moot.
fn url_hash_hex(url: &str) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for b in url.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(PRIME);
    }
    format!("{h:016x}")
}

/// Full on-disk path for a cached avatar URL (no extension; format is detected
/// from the bytes on load).
pub fn cache_path_for_url(url: &str) -> Option<PathBuf> {
    Some(cache_dir()?.join(url_hash_hex(url)))
}

/// Read cached avatar bytes for `url` from disk, if present.
fn read_disk_cache(url: &str) -> Option<Vec<u8>> {
    let path = cache_path_for_url(url)?;
    std::fs::read(&path).ok().filter(|b| !b.is_empty())
}

/// Persist avatar bytes for `url` to the disk cache (best-effort; failures are
/// silently ignored so a read-only HOME never breaks rendering).
fn write_disk_cache(url: &str, bytes: &[u8]) {
    let Some(path) = cache_path_for_url(url) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, bytes);
}

// ──────────────────────────────────────────────────────────────────────────
// Image decoding helpers
// ──────────────────────────────────────────────────────────────────────────

/// Detect a supported raster image format from magic bytes.
///
/// Returns `None` for unrecognised / corrupt data so callers fall back to the
/// initial circle instead of handing gpui bytes it cannot decode.
fn detect_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.len() >= 8 && bytes[..8] == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        Some(ImageFormat::Png)
    } else if bytes.len() >= 3 && bytes[..3] == [0xff, 0xd8, 0xff] {
        Some(ImageFormat::Jpeg)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else if bytes.len() >= 6 && (&bytes[..6] == b"GIF87a" || &bytes[..6] == b"GIF89a") {
        Some(ImageFormat::Gif)
    } else {
        None
    }
}

/// Wrap avatar bytes into a `gpui::Image` if the format is recognised.
///
/// gpui decodes png/jpeg/webp/gif internally via its bundled `image` crate, so
/// no self-decode dependency is needed.
pub fn image_from_bytes(bytes: Vec<u8>) -> Option<std::sync::Arc<Image>> {
    let format = detect_format(&bytes)?;
    Some(std::sync::Arc::new(Image::from_bytes(format, bytes)))
}

// ──────────────────────────────────────────────────────────────────────────
// HTTP
// ──────────────────────────────────────────────────────────────────────────

/// Blocking GET returning the response body bytes (≤ a few MB; avatars are
/// tiny).  Returns `None` on any error or non-2xx status.
fn http_get_bytes(url: &str) -> Option<Vec<u8>> {
    let mut resp = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .config()
        .timeout_global(Some(HTTP_TIMEOUT))
        .build()
        .call()
        .ok()?;
    if resp.status().as_u16() != 200 {
        return None;
    }
    resp.body_mut().read_to_vec().ok()
}

/// Blocking GET of avatar image bytes for `url`, using the disk cache first.
///
/// On a cache miss the bytes are fetched over HTTP and written back to disk so
/// subsequent launches need no network.  Returns the raw bytes (undecoded).
pub fn fetch_avatar_bytes(url: &str) -> Option<Vec<u8>> {
    if let Some(bytes) = read_disk_cache(url) {
        return Some(bytes);
    }
    if offline() {
        return None;
    }
    let bytes = http_get_bytes(url)?;
    if bytes.is_empty() {
        return None;
    }
    write_disk_cache(url, &bytes);
    Some(bytes)
}

/// Fetch up to [`MAX_COMMIT_PAGES`] pages of the unauthenticated Commits API and
/// build an `email → avatar_url` map.
///
/// Used for authors whose commit email is *not* a parseable noreply address.
/// Returns an empty map when offline, on network error, or for private repos
/// (unauthenticated 404) — callers then fall back to the initial circle.
pub fn fetch_commit_author_avatars(owner: &str, repo: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    if offline() {
        return out;
    }
    for page in 1..=MAX_COMMIT_PAGES {
        let url =
            format!("https://api.github.com/repos/{owner}/{repo}/commits?per_page=100&page={page}");
        let Some(bytes) = http_get_bytes(&url) else {
            break;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            break;
        };
        let entries = parse_commits_api(&text);
        if entries.is_empty() {
            break;
        }
        let count = entries.len();
        out.extend(entries);
        // Last page reached (GitHub returns <100 on the final page).
        if count < 100 {
            break;
        }
    }
    out
}

/// Extract `(commit.author.email, author.avatar_url)` pairs from a Commits API
/// JSON response.
///
/// A dependency-free scanner: it walks each top-level commit object and pulls
/// the author email (under `"commit":{"author":{"email":...}}`) and the GitHub
/// account avatar URL (the top-level `"author":{"avatar_url":...}`).  This is
/// resilient to field order and ignores entries missing either field.
pub fn parse_commits_api(json: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    // GitHub returns a JSON array of commit objects. We scan for each
    // `"avatar_url"` (only present on the account author/committer objects) and
    // pair it with the nearest preceding `"email"`. To bind them to the *commit
    // author* specifically we anchor on the literal `"avatar_url"` that follows
    // an `"author"` object and back-reference the email captured from the
    // commit metadata. A full JSON parser is avoided (no serde dependency).
    //
    // Strategy: split into commit records on the `"sha"` top-level key is
    // fragile, so instead we pair each email with the first avatar_url that
    // appears after it within the same record window.
    for (email_pos, email) in find_quoted_values(json, "\"email\":") {
        // Find the next avatar_url after this email; bounded by the next email
        // so we don't bleed into the following record.
        let next_email = json[email_pos + 1..]
            .find("\"email\":")
            .map(|p| email_pos + 1 + p)
            .unwrap_or(json.len());
        if let Some(avatar) =
            find_first_quoted_value(&json[email_pos..next_email], "\"avatar_url\":")
        {
            if !email.is_empty() && !avatar.is_empty() {
                out.push((email, avatar));
            }
        }
    }
    out
}

/// Find all `(byte_pos, value)` pairs for a `"key":` whose value is a quoted
/// string.  `byte_pos` is the offset of the key in `json`.
fn find_quoted_values(json: &str, key: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = json[from..].find(key) {
        let key_pos = from + rel;
        let after = key_pos + key.len();
        if let Some(val) = read_json_string_after(&json[after..]) {
            out.push((key_pos, val));
        }
        from = after;
    }
    out
}

/// Find the first quoted value for `key` within `slice`.
fn find_first_quoted_value(slice: &str, key: &str) -> Option<String> {
    let rel = slice.find(key)?;
    read_json_string_after(&slice[rel + key.len()..])
}

/// Read a JSON string value that begins (after optional whitespace) at the
/// start of `s` — i.e. the `"..."` following a `"key":`.  Handles `\"` escapes
/// and `null` (returns `None`).  Operates on `char_indices` to stay UTF-8 safe.
fn read_json_string_after(s: &str) -> Option<String> {
    let mut chars = s.char_indices().peekable();
    // Skip whitespace.
    while let Some(&(_, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    // Must be an opening quote (otherwise value is null/number/object — skip).
    match chars.peek() {
        Some(&(_, '"')) => {
            chars.next();
        }
        _ => return None,
    }
    let mut value = String::new();
    let mut escaped = false;
    for (_, c) in chars {
        if escaped {
            // Minimal unescape: pass through the escaped char (avatar URLs and
            // emails contain no unicode escapes in practice).
            value.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some(value);
        } else {
            value.push(c);
        }
    }
    None
}

// ──────────────────────────────────────────────────────────────────────────
// Resolution (runs on a background thread)
// ──────────────────────────────────────────────────────────────────────────

/// Outcome of a background avatar resolution pass.
pub struct ResolveOutcome {
    /// email → decoded avatar image.
    pub images: Vec<(String, std::sync::Arc<Image>)>,
    /// Number of distinct emails resolved to an image.
    pub resolved: usize,
    /// Number of distinct emails left unresolved (fallback to initial circle).
    pub pending: usize,
}

/// Resolve avatar images for a set of author emails in a GitHub repo.
///
/// This is the whole background job: it runs entirely off the UI thread (no
/// gpui context, only `Send` data in and out).  It follows ADR-0037's order:
///   1. noreply email → CDN URL (no network for the URL itself).
///   2. Commits API batch for the remaining emails (skipped offline / private).
///   3. fetch + decode each distinct URL (disk-cache first).
///
/// `emails` should already be de-duplicated by the caller; duplicates are
/// tolerated but wasteful.
pub fn resolve_avatars(owner: &str, repo: &str, emails: &[String]) -> ResolveOutcome {
    // email → avatar URL
    let mut url_for_email: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    // Step 1: noreply parse (free).
    let mut unresolved: Vec<String> = Vec::new();
    for email in emails {
        if let Some(user) = parse_noreply_username(email) {
            url_for_email.insert(email.clone(), avatar_url_for_username(&user));
        } else {
            unresolved.push(email.clone());
        }
    }

    // Step 2: Commits API batch for the remainder (only if any remain and a
    // network attempt is worthwhile).
    if !unresolved.is_empty() && !offline() {
        let api_map = fetch_commit_author_avatars(owner, repo);
        if !api_map.is_empty() {
            let lookup: std::collections::HashMap<&str, &str> = api_map
                .iter()
                .map(|(e, u)| (e.as_str(), u.as_str()))
                .collect();
            for email in &unresolved {
                if let Some(url) = lookup.get(email.as_str()) {
                    url_for_email.insert(email.clone(), (*url).to_string());
                }
            }
        }
    }

    // Step 3: fetch + decode each distinct URL once, then map back to emails.
    let mut image_for_url: std::collections::HashMap<String, std::sync::Arc<Image>> =
        std::collections::HashMap::new();
    let mut images: Vec<(String, std::sync::Arc<Image>)> = Vec::new();
    for (email, url) in &url_for_email {
        let img = if let Some(img) = image_for_url.get(url) {
            Some(img.clone())
        } else if let Some(bytes) = fetch_avatar_bytes(url) {
            let img = image_from_bytes(bytes);
            if let Some(ref img) = img {
                image_for_url.insert(url.clone(), img.clone());
            }
            img
        } else {
            None
        };
        if let Some(img) = img {
            images.push((email.clone(), img));
        }
    }

    let resolved = images.len();
    let total = emails.len();
    ResolveOutcome {
        images,
        resolved,
        pending: total.saturating_sub(resolved),
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Unit tests
// ──────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── noreply parse ──────────────────────────────────────────

    #[test]
    fn noreply_new_format() {
        assert_eq!(
            parse_noreply_username("12345678+octocat@users.noreply.github.com").as_deref(),
            Some("octocat")
        );
    }

    #[test]
    fn noreply_old_format() {
        assert_eq!(
            parse_noreply_username("octocat@users.noreply.github.com").as_deref(),
            Some("octocat")
        );
    }

    #[test]
    fn noreply_case_insensitive_domain() {
        assert_eq!(
            parse_noreply_username("octocat@users.noreply.GitHub.com").as_deref(),
            Some("octocat")
        );
    }

    #[test]
    fn noreply_non_numeric_prefix_keeps_local() {
        // A `+` with a non-numeric prefix is not the id form: keep whole local.
        assert_eq!(
            parse_noreply_username("foo+bar@users.noreply.github.com").as_deref(),
            Some("foo+bar")
        );
    }

    #[test]
    fn noreply_rejects_real_email() {
        // Real emails must NOT be parsed (privacy: never derive a username).
        assert_eq!(parse_noreply_username("alice@example.com"), None);
        assert_eq!(parse_noreply_username("dev@gmail.com"), None);
    }

    #[test]
    fn noreply_rejects_lookalike_domain() {
        assert_eq!(
            parse_noreply_username("x@users.noreply.github.com.evil.com"),
            None
        );
        assert_eq!(parse_noreply_username("x@github.com"), None);
    }

    #[test]
    fn noreply_rejects_empty_and_malformed() {
        assert_eq!(parse_noreply_username(""), None);
        assert_eq!(parse_noreply_username("nodomain"), None);
        assert_eq!(parse_noreply_username("@users.noreply.github.com"), None);
        assert_eq!(
            parse_noreply_username("12345+@users.noreply.github.com"),
            None
        );
    }

    #[test]
    fn avatar_url_format() {
        assert_eq!(
            avatar_url_for_username("octocat"),
            "https://avatars.githubusercontent.com/octocat?s=64"
        );
    }

    // ── github_owner_repo ──────────────────────────────────────

    #[test]
    fn coords_https_with_git() {
        assert_eq!(
            github_owner_repo("https://github.com/TomiXRM/kagi.git"),
            Some(("TomiXRM".to_string(), "kagi".to_string()))
        );
    }

    #[test]
    fn coords_https_no_git() {
        assert_eq!(
            github_owner_repo("https://github.com/TomiXRM/kagi"),
            Some(("TomiXRM".to_string(), "kagi".to_string()))
        );
    }

    #[test]
    fn coords_scp_like() {
        assert_eq!(
            github_owner_repo("git@github.com:TomiXRM/kagi.git"),
            Some(("TomiXRM".to_string(), "kagi".to_string()))
        );
    }

    #[test]
    fn coords_ssh_scheme() {
        assert_eq!(
            github_owner_repo("ssh://git@github.com/TomiXRM/kagi.git"),
            Some(("TomiXRM".to_string(), "kagi".to_string()))
        );
    }

    #[test]
    fn coords_trailing_slash() {
        assert_eq!(
            github_owner_repo("https://github.com/owner/repo/"),
            Some(("owner".to_string(), "repo".to_string()))
        );
    }

    #[test]
    fn coords_non_github() {
        assert_eq!(github_owner_repo("https://gitlab.com/owner/repo.git"), None);
        assert_eq!(github_owner_repo("git@bitbucket.org:owner/repo.git"), None);
        assert_eq!(github_owner_repo(""), None);
    }

    // ── url_hash / cache path ──────────────────────────────────

    #[test]
    fn url_hash_stable_and_hex() {
        let a = url_hash_hex("https://avatars.githubusercontent.com/octocat?s=64");
        let b = url_hash_hex("https://avatars.githubusercontent.com/octocat?s=64");
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        // Different URLs hash differently.
        assert_ne!(
            a,
            url_hash_hex("https://avatars.githubusercontent.com/other?s=64")
        );
    }

    // ── format detection ───────────────────────────────────────

    #[test]
    fn detect_png() {
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0];
        assert_eq!(detect_format(&png), Some(ImageFormat::Png));
    }

    #[test]
    fn detect_jpeg() {
        let jpg = [0xff, 0xd8, 0xff, 0xe0, 0, 0];
        assert_eq!(detect_format(&jpg), Some(ImageFormat::Jpeg));
    }

    #[test]
    fn detect_garbage_is_none() {
        assert_eq!(detect_format(b"not an image"), None);
        assert_eq!(detect_format(&[]), None);
    }

    // ── Commits API JSON scan ──────────────────────────────────

    #[test]
    fn parse_commits_api_basic() {
        // Minimal shape mirroring the real API: commit.author.email + top-level
        // author.avatar_url per record.
        let json = r#"[
          {
            "sha": "abc",
            "commit": { "author": { "name": "Alice", "email": "alice@example.com" } },
            "author": { "login": "alice", "avatar_url": "https://avatars.githubusercontent.com/u/1?v=4" }
          },
          {
            "sha": "def",
            "commit": { "author": { "name": "Bob", "email": "bob@example.com" } },
            "author": { "login": "bob", "avatar_url": "https://avatars.githubusercontent.com/u/2?v=4" }
          }
        ]"#;
        let map = parse_commits_api(json);
        assert!(map.contains(&(
            "alice@example.com".to_string(),
            "https://avatars.githubusercontent.com/u/1?v=4".to_string()
        )));
        assert!(map.contains(&(
            "bob@example.com".to_string(),
            "https://avatars.githubusercontent.com/u/2?v=4".to_string()
        )));
    }

    #[test]
    fn parse_commits_api_null_author() {
        // author == null (commit by a non-GitHub account) → no avatar pair.
        let json = r#"[
          {
            "commit": { "author": { "email": "ghost@example.com" } },
            "author": null
          }
        ]"#;
        let map = parse_commits_api(json);
        assert!(map.is_empty());
    }

    #[test]
    fn parse_commits_api_empty() {
        assert!(parse_commits_api("[]").is_empty());
        assert!(parse_commits_api("").is_empty());
    }
}
