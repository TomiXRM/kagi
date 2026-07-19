//! Public avatar lookups (ADR-0122): Gravatar + GitHub user search.
//!
//! The two email-derived lookup sources added by ADR-0122, split out of
//! [`super::avatar_fetch`] (which keeps the noreply/Commits-API resolution,
//! caching and HTTP plumbing). Pure URL builders + one blocking API call;
//! callers gate network access via [`super::avatar_fetch::offline`].

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use sha2::{Digest, Sha256};

use super::avatar_fetch::{avatar_url_for_username, http_get_bytes};

/// Gravatar URL for `email` (ADR-0122): SHA-256 of the trimmed, lowercased
/// address. `d=404` makes unregistered emails return 404 instead of a
/// generated placeholder — the initial circle is kagi's own placeholder.
pub fn gravatar_url_for_email(email: &str) -> String {
    let normalized = email.trim().to_lowercase();
    let digest = Sha256::digest(normalized.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("https://www.gravatar.com/avatar/{hex}?s=64&d=404")
}

/// Build the user-search API URL for a public-profile-email lookup.
///
/// The email is percent-encoded as a query value (`+` in a local part must not
/// read as a space); the trailing `+in:email` scopes the search to profile
/// emails so name/login matches can't hijack the result.
fn github_search_query_url(email: &str) -> String {
    let q = utf8_percent_encode(email, NON_ALPHANUMERIC);
    format!("https://api.github.com/search/users?q={q}+in:email&per_page=1")
}

/// Look up a GitHub account whose **public profile email** matches `email`
/// (ADR-0122) and return its avatars CDN URL. `None` on no match / rate limit
/// / network error.
pub fn github_search_avatar_url(email: &str) -> Option<String> {
    let bytes = http_get_bytes(&github_search_query_url(email))?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let login = json
        .get("items")?
        .as_array()?
        .first()?
        .get("login")?
        .as_str()?;
    if login.is_empty() {
        return None;
    }
    Some(avatar_url_for_username(login))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gravatar_url_uses_sha256_of_normalized_email() {
        // sha256("abc") is the RFC 6234 test vector; trim + lowercase must be
        // applied before hashing.
        assert_eq!(
            gravatar_url_for_email("  ABC "),
            "https://www.gravatar.com/avatar/ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad?s=64&d=404"
        );
        // Same normalization → same URL.
        assert_eq!(
            gravatar_url_for_email("Alice@Example.com"),
            gravatar_url_for_email("alice@example.com")
        );
    }

    #[test]
    fn search_query_url_percent_encodes_email() {
        // `+` must not read as a query-string space; `@` and `.` are encoded
        // too (NON_ALPHANUMERIC). The `+in:email` scope stays literal.
        assert_eq!(
            github_search_query_url("a+b@x.com"),
            "https://api.github.com/search/users?q=a%2Bb%40x%2Ecom+in:email&per_page=1"
        );
    }
}
