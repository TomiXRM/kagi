//! Pure auto-update model (ADR-0082).
//!
//! Version parsing/compare, GitHub release-JSON parsing, and per-platform asset
//! selection. No I/O, no git2, no gpui — the `ureq` fetch + download/verify/
//! install live in `src/update/`. Everything here is unit-testable from a string.

use std::cmp::Ordering;

// ────────────────────────────────────────────────────────────
// Version
// ────────────────────────────────────────────────────────────

/// A semantic version `major.minor.patch` with an optional pre-release tag.
///
/// A final release sorts **after** any pre-release of the same `x.y.z`
/// (`1.0.0 > 1.0.0-beta`), matching semver ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl Version {
    /// Parse `"v1.2.3"`, `"1.2.3"`, or `"1.2.3-beta.1"`. Returns `None` on a
    /// malformed string (wrong arity, non-numeric core).
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix('v').unwrap_or(s);
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, Some(p.to_string())),
            _ => (s, None),
        };
        let mut it = core.split('.');
        let major = it.next()?.parse().ok()?;
        let minor = it.next()?.parse().ok()?;
        let patch = it.next()?.parse().ok()?;
        if it.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }

    /// `true` if this is a final (non-pre-release) version.
    pub fn is_stable(&self) -> bool {
        self.pre.is_none()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                // A final release outranks any pre-release of the same core.
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// ────────────────────────────────────────────────────────────
// Release model
// ────────────────────────────────────────────────────────────

/// A single downloadable release asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// A GitHub release, parsed from the `releases/latest` JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag: String,
    pub version: Version,
    pub notes: String,
    pub assets: Vec<Asset>,
}

/// The concrete plan to update from `current` to a newer release, with the
/// asset chosen for the host platform. Built by [`plan_update`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePlan {
    pub current: Version,
    pub latest: Version,
    pub tag: String,
    pub notes: String,
    pub asset: Asset,
}

/// Select the asset matching the host OS/arch.
///
/// `os` = [`std::env::consts::OS`] (`"macos"`/`"linux"`/`"windows"`), `arch` =
/// [`std::env::consts::ARCH`] (`"aarch64"`/`"x86_64"`). Mirrors the names emitted
/// by `release.yml` (ADR-0047): `Kagi-<v>-arm64.dmg`, `kagi-<v>-<arch>.tar.gz`,
/// `kagi-<v>-x86_64-windows.zip`. The Linux AppImage zip is intentionally not
/// chosen (the tar.gz carries the bare binary, which is simplest to swap).
pub fn pick_asset<'a>(assets: &'a [Asset], os: &str, arch: &str) -> Option<&'a Asset> {
    match os {
        "macos" => {
            let a = if arch == "aarch64" { "arm64" } else { "x86_64" };
            assets
                .iter()
                .find(|x| x.name.ends_with(".dmg") && x.name.contains(a))
        }
        "windows" => assets
            .iter()
            .find(|x| x.name.contains("windows") && x.name.ends_with(".zip")),
        "linux" => {
            let a = if arch == "aarch64" {
                "aarch64"
            } else {
                "x86_64"
            };
            assets
                .iter()
                .find(|x| x.name.ends_with(".tar.gz") && x.name.contains(a))
        }
        _ => None,
    }
}

/// Build an [`UpdatePlan`] iff `release` is a stable version strictly newer than
/// `current`, is not the `skipped` tag, and a matching asset exists for the host.
/// Returns `None` otherwise (up to date / pre-release / skipped / no asset).
pub fn plan_update(
    current: &Version,
    release: &ReleaseInfo,
    os: &str,
    arch: &str,
    skipped: Option<&str>,
) -> Option<UpdatePlan> {
    if !release.version.is_stable() {
        return None; // stable channel ignores pre-releases
    }
    if release.version <= *current {
        return None;
    }
    if skipped == Some(release.tag.as_str()) {
        return None;
    }
    let asset = pick_asset(&release.assets, os, arch)?.clone();
    Some(UpdatePlan {
        current: current.clone(),
        latest: release.version.clone(),
        tag: release.tag.clone(),
        notes: release.notes.clone(),
        asset,
    })
}

/// Find the expected lowercase SHA-256 hex for `asset_name` in the concatenated
/// text of one or more `SHA256SUMS` files (lines: `<hex>␠␠<filename>`).
pub fn find_checksum(checksums: &str, asset_name: &str) -> Option<String> {
    for line in checksums.lines() {
        let line = line.trim();
        let mut it = line.split_whitespace();
        let (Some(hex), Some(name)) = (it.next(), it.next()) else {
            continue;
        };
        // `shasum`/`sha256sum` may prefix the name with `*` (binary mode).
        let name = name.trim_start_matches('*');
        if name == asset_name && hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(hex.to_ascii_lowercase());
        }
    }
    None
}

// ────────────────────────────────────────────────────────────
// Release JSON parsing (no serde — string-aware hand scan, like avatar_fetch)
// ────────────────────────────────────────────────────────────

/// Parse the GitHub `releases/latest` JSON into a [`ReleaseInfo`].
///
/// Hand-rolled (no serde dependency — consistent with `avatar_fetch`), but
/// string-aware: the `assets` array is bracket-matched and each object scanned
/// in isolation, so the top-level `"name"`/`"body"` never collide with an
/// asset's `"name"`.
pub fn parse_release_json(json: &str) -> Option<ReleaseInfo> {
    let tag = field_string(json, "tag_name")?;
    let version = Version::parse(&tag)?;
    let notes = field_string(json, "body").unwrap_or_default();
    let assets = parse_assets(json);
    Some(ReleaseInfo {
        tag,
        version,
        notes,
        assets,
    })
}

/// Read the JSON string value for the first `"key": "..."` in `json`. Handles
/// `\"`, `\\`, `\n`, `\t`, `\r`, `\/`, and `\uXXXX` (BMP) escapes.
fn field_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let key_pos = json.find(&needle)?;
    let after = key_pos + needle.len();
    let colon = json[after..].find(':')? + after;
    let q = json[colon + 1..].find('"')? + colon + 1;
    scan_json_string(json.as_bytes(), q).map(|(v, _)| v)
}

/// Read the JSON number value for the first `"key": <digits>` in `json`.
fn field_u64(json: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{}\"", key);
    let key_pos = json.find(&needle)?;
    let after = key_pos + needle.len();
    let colon = json[after..].find(':')? + after;
    let rest = json[colon + 1..].trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Scan a JSON string whose opening quote is at byte index `open_quote`.
/// Returns the decoded value and the index just past the closing quote.
fn scan_json_string(bytes: &[u8], open_quote: usize) -> Option<(String, usize)> {
    let mut i = open_quote + 1;
    let mut out: Vec<u8> = Vec::new();
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 1;
                let e = *bytes.get(i)?;
                match e {
                    b'"' => out.push(b'"'),
                    b'\\' => out.push(b'\\'),
                    b'/' => out.push(b'/'),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0c),
                    b'u' => {
                        if i + 4 < bytes.len() {
                            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 5]) {
                                if let Ok(cp) = u32::from_str_radix(hex, 16) {
                                    if let Some(ch) = char::from_u32(cp) {
                                        let mut buf = [0u8; 4];
                                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                                    }
                                }
                            }
                            i += 4;
                        }
                    }
                    other => out.push(other),
                }
                i += 1;
            }
            b'"' => return Some((String::from_utf8_lossy(&out).into_owned(), i + 1)),
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    None
}

/// Parse the `"assets":[ {…}, {…} ]` array into [`Asset`]s. String-aware bracket
/// matching so braces/brackets inside string values don't confuse the scan.
fn parse_assets(json: &str) -> Vec<Asset> {
    let Some((arr_start, arr_end)) = locate_array(json, "assets") else {
        return Vec::new();
    };
    let arr = &json[arr_start..=arr_end];
    let mut out = Vec::new();
    for obj in top_level_objects(arr) {
        if let Some(a) = parse_one_asset(obj) {
            out.push(a);
        }
    }
    out
}

fn parse_one_asset(obj: &str) -> Option<Asset> {
    let name = field_string(obj, "name")?;
    let url = field_string(obj, "browser_download_url")?;
    let size = field_u64(obj, "size").unwrap_or(0);
    Some(Asset { name, url, size })
}

/// Find `"key": [ ... ]` and return the inclusive byte range of the `[`..`]`.
fn locate_array(json: &str, key: &str) -> Option<(usize, usize)> {
    let needle = format!("\"{}\"", key);
    let kp = json.find(&needle)?;
    let open = json[kp..].find('[')? + kp;
    let bytes = json.as_bytes();
    let (mut i, mut depth, mut in_str, mut esc) = (open, 0i32, false, false);
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'[' | b'{' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((open, i));
                    }
                }
                b'}' => depth -= 1,
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Split a `[ {…}, {…} ]` slice into its top-level `{…}` object substrings.
fn top_level_objects(arr: &str) -> Vec<&str> {
    let bytes = arr.as_bytes();
    let (mut i, mut depth, mut start, mut in_str, mut esc) = (0usize, 0i32, 0usize, false, false);
    let mut out = Vec::new();
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'{' => {
                    if depth == 0 {
                        start = i;
                    }
                    depth += 1;
                }
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        out.push(&arr[start..=i]);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_orders_versions() {
        assert_eq!(
            Version::parse("v0.3.3"),
            Some(Version {
                major: 0,
                minor: 3,
                patch: 3,
                pre: None
            })
        );
        assert_eq!(Version::parse("1.2.3").unwrap().major, 1);
        assert!(Version::parse("0.3").is_none());
        assert!(Version::parse("x.y.z").is_none());

        let v033 = Version::parse("0.3.3").unwrap();
        let v034 = Version::parse("0.3.4").unwrap();
        let v0310 = Version::parse("0.3.10").unwrap();
        assert!(v034 > v033);
        assert!(v0310 > v034); // numeric, not lexicographic
                               // final outranks its own pre-release
        let beta = Version::parse("0.3.4-beta.1").unwrap();
        assert!(v034 > beta);
        assert!(beta > v033);
        assert!(!beta.is_stable());
    }

    fn sample_json() -> &'static str {
        // Trimmed but structurally faithful GitHub releases/latest JSON, with a
        // top-level "name"/"body" plus an assets array whose objects also have
        // "name" — the parser must not confuse them.
        r#"{
          "tag_name": "v0.3.4",
          "name": "v0.3.4",
          "body": "Line one\nLine two with a \"quote\" and emoji 🚀",
          "assets": [
            {"name":"Kagi-0.3.4-arm64.dmg","size":1111,"browser_download_url":"https://example.com/Kagi-0.3.4-arm64.dmg"},
            {"name":"kagi-0.3.4-x86_64.tar.gz","size":2222,"browser_download_url":"https://example.com/kagi-0.3.4-x86_64.tar.gz"},
            {"name":"kagi-0.3.4-aarch64.tar.gz","size":3333,"browser_download_url":"https://example.com/kagi-0.3.4-aarch64.tar.gz"},
            {"name":"kagi-0.3.4-x86_64-windows.zip","size":4444,"browser_download_url":"https://example.com/kagi-0.3.4-x86_64-windows.zip"},
            {"name":"SHA256SUMS-macos-arm64.txt","size":55,"browser_download_url":"https://example.com/SHA256SUMS-macos-arm64.txt"}
          ]
        }"#
    }

    #[test]
    fn parses_release_json() {
        let r = parse_release_json(sample_json()).expect("parse");
        assert_eq!(r.tag, "v0.3.4");
        assert_eq!(r.version, Version::parse("0.3.4").unwrap());
        assert!(r.notes.contains("Line one\nLine two"));
        assert!(r.notes.contains("\"quote\""));
        assert!(r.notes.contains('🚀')); // \uXXXX decoded
        assert_eq!(r.assets.len(), 5);
        let dmg = r.assets.iter().find(|a| a.name.ends_with(".dmg")).unwrap();
        assert_eq!(dmg.url, "https://example.com/Kagi-0.3.4-arm64.dmg");
        assert_eq!(dmg.size, 1111);
    }

    #[test]
    fn picks_per_platform_assets() {
        let r = parse_release_json(sample_json()).unwrap();
        assert!(pick_asset(&r.assets, "macos", "aarch64")
            .unwrap()
            .name
            .ends_with("arm64.dmg"));
        assert_eq!(
            pick_asset(&r.assets, "linux", "x86_64").unwrap().name,
            "kagi-0.3.4-x86_64.tar.gz"
        );
        assert_eq!(
            pick_asset(&r.assets, "linux", "aarch64").unwrap().name,
            "kagi-0.3.4-aarch64.tar.gz"
        );
        assert_eq!(
            pick_asset(&r.assets, "windows", "x86_64").unwrap().name,
            "kagi-0.3.4-x86_64-windows.zip"
        );
    }

    #[test]
    fn plan_update_gates() {
        let r = parse_release_json(sample_json()).unwrap();
        let cur = Version::parse("0.3.3").unwrap();
        // newer → plan
        assert!(plan_update(&cur, &r, "macos", "aarch64", None).is_some());
        // up to date → none
        let same = Version::parse("0.3.4").unwrap();
        assert!(plan_update(&same, &r, "macos", "aarch64", None).is_none());
        // newer current → none
        let ahead = Version::parse("0.4.0").unwrap();
        assert!(plan_update(&ahead, &r, "macos", "aarch64", None).is_none());
        // skipped → none
        assert!(plan_update(&cur, &r, "macos", "aarch64", Some("v0.3.4")).is_none());
        // unknown platform → none (no asset)
        assert!(plan_update(&cur, &r, "freebsd", "x86_64", None).is_none());
    }

    #[test]
    fn finds_checksum_line() {
        let sums = "abc  not-it.txt\n\
            d4f1e2a3b4c5d6e7f80911223344556677889900aabbccddeeff001122334455  kagi-0.3.4-x86_64.tar.gz\n\
            00ff  short.txt\n";
        assert_eq!(
            find_checksum(sums, "kagi-0.3.4-x86_64.tar.gz").as_deref(),
            Some("d4f1e2a3b4c5d6e7f80911223344556677889900aabbccddeeff001122334455")
        );
        assert!(find_checksum(sums, "missing.zip").is_none());
        // binary-mode '*' prefix tolerated
        let star = "aa11bb22cc33dd44ee55ff6677889900aabbccddeeff00112233445566778899 *kagi.exe\n";
        assert!(find_checksum(star, "kagi.exe").is_some());
    }

    #[test]
    fn pre_release_is_not_offered_on_stable() {
        let json = r#"{"tag_name":"v0.4.0-beta.1","name":"beta","body":"",
            "assets":[{"name":"kagi-0.4.0-x86_64.tar.gz","size":1,"browser_download_url":"https://e/x"}]}"#;
        let r = parse_release_json(json).unwrap();
        let cur = Version::parse("0.3.3").unwrap();
        assert!(plan_update(&cur, &r, "linux", "x86_64", None).is_none());
    }
}
