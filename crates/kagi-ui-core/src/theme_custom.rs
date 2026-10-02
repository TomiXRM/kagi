//! #922 / ADR-0220: user theme files (`<settings dir>/themes/*.json`).
//!
//! A file is one JSON object. `slug` and `name` are required; every other key
//! is a [`Theme`] field name. `"extends": "<built-in slug>"` inherits the
//! remaining fields from that built-in theme (`syntax` merges per key);
//! without it every field is required. Colours are `"#rrggbb"` strings, the
//! one RGBA value is `term_selection: {"color": "#rrggbb", "alpha": 0..255}`,
//! `lane_hsl` is eight `[h, s, l]` triples and `avatar_*` are scalars, all in
//! `0..=1`. Anything else — unknown keys, wrong types, malformed colours,
//! out-of-range numbers, a slug a built-in or an earlier file already uses —
//! rejects that one file and leaves the rest loading.
//!
//! This module only parses; the runtime registry lives in [`crate::theme`].

use std::borrow::Cow;
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::theme::{index_of, SyntaxPalette, Theme, THEMES};

/// One theme file that could not be loaded, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeLoadError {
    /// The rejected file (or the themes folder, when it can't be listed).
    pub path: PathBuf,
    /// Human-readable reason, without the file name.
    pub reason: String,
}

impl fmt::Display for ThemeLoadError {
    /// `<file name>: <reason>` — the form logs and toasts show.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_else(|| self.path.to_string_lossy());
        write!(f, "{name}: {}", self.reason)
    }
}

/// The custom-theme folder: `themes/` next to `settings.json`
/// (`$KAGI_LOG_DIR/themes`, else `~/.kagi/themes`).
pub fn themes_dir() -> Option<PathBuf> {
    Some(crate::settings::settings_path()?.parent()?.join("themes"))
}

/// Read every `*.json` file directly inside `dir`, in file-name order.
///
/// A missing folder is not an error (most users never create one). On a slug
/// collision between two files the first by file name wins and the later one
/// is reported.
pub(crate) fn load_dir(dir: &Path) -> (Vec<Theme>, Vec<ThemeLoadError>) {
    let mut themes: Vec<Theme> = Vec::new();
    let mut errors = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (themes, errors),
        Err(e) => {
            errors.push(ThemeLoadError {
                path: dir.to_path_buf(),
                reason: format!("cannot read the themes folder: {e}"),
            });
            return (themes, errors);
        }
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
                && path.is_file()
        })
        .collect();
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    // Which file claimed each custom slug, for the collision message.
    let mut owners: Vec<&Path> = Vec::new();
    for path in &files {
        let parsed = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read the file: {e}"))
            .and_then(|text| parse_theme(&text));
        let result =
            parsed.and_then(
                |theme| match themes.iter().position(|t| t.slug == theme.slug) {
                    Some(i) => Err(format!(
                        "slug \"{}\" is already used by {}",
                        theme.slug,
                        owners[i]
                            .file_name()
                            .map(|n| n.to_string_lossy())
                            .unwrap_or_default()
                    )),
                    None => Ok(theme),
                },
            );
        match result {
            Ok(theme) => {
                themes.push(theme);
                owners.push(path);
            }
            Err(reason) => errors.push(ThemeLoadError {
                path: path.clone(),
                reason,
            }),
        }
    }
    (themes, errors)
}

/// Parse and validate one theme file's text into a complete [`Theme`].
pub(crate) fn parse_theme(text: &str) -> Result<Theme, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("invalid JSON: {e}"))?;
    let Value::Object(map) = value else {
        return Err("the file must contain one JSON object".into());
    };

    let unknown: Vec<String> = map
        .keys()
        .filter(|k| !is_top_level_key(k))
        .map(|k| format!("\"{k}\""))
        .collect();
    if !unknown.is_empty() {
        return Err(format!("unknown key {}", unknown.join(", ")));
    }

    let slug = required_str(&map, "slug")?;
    validate_slug(slug)?;
    if let Some(i) = index_of(slug) {
        return Err(format!(
            "slug \"{slug}\" is already used by built-in theme \"{}\"",
            THEMES[i].name
        ));
    }
    let name = required_str(&map, "name")?.trim();
    if name.is_empty() {
        return Err("\"name\" must not be empty".into());
    }
    let base = match map.get("extends") {
        None => None,
        Some(Value::String(parent)) => Some(
            THEMES
                .iter()
                .find(|t| t.slug == parent.as_str())
                .ok_or_else(|| format!("\"extends\": \"{parent}\" is not a built-in theme slug"))?,
        ),
        Some(other) => {
            return Err(format!(
                "\"extends\": expected a built-in theme slug, got {other}"
            ))
        }
    };

    let mut patch = Patch::default();
    for (key, value) in &map {
        if matches!(key.as_str(), "slug" | "name" | "extends") {
            continue;
        }
        patch
            .set(key, value)
            .map_err(|reason| format!("\"{key}\": {reason}"))?;
    }
    patch.build(
        Cow::Owned(slug.to_owned()),
        Cow::Owned(name.to_owned()),
        base,
    )
}

fn required_str<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    match map.get(key) {
        Some(Value::String(s)) => Ok(s),
        Some(other) => Err(format!("\"{key}\": expected a string, got {other}")),
        None => Err(format!("missing required key \"{key}\"")),
    }
}

/// Slugs end up in `settings.json`, `KAGI_THEME` and command ids, so keep them
/// to the same lowercase shape the built-ins use.
fn validate_slug(slug: &str) -> Result<(), String> {
    let valid = (1..=64).contains(&slug.len())
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !slug.starts_with('-');
    if valid {
        Ok(())
    } else {
        Err(format!(
            "\"slug\": \"{slug}\" must be 1-64 characters of a-z, 0-9 and '-', not starting with '-'"
        ))
    }
}

// ── Value parsers ────────────────────────────────────────────────────────

fn parse_rgb(v: &Value) -> Result<u32, String> {
    let bad = || format!("expected a \"#rrggbb\" colour, got {v}");
    let s = v.as_str().ok_or_else(bad)?;
    let hex = s.strip_prefix('#').ok_or_else(bad)?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    u32::from_str_radix(hex, 16).map_err(|_| bad())
}

fn split_rgb(c: u32) -> (u8, u8, u8) {
    ((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn parse_bool(v: &Value) -> Result<bool, String> {
    v.as_bool()
        .ok_or_else(|| format!("expected true or false, got {v}"))
}

fn parse_unit(v: &Value) -> Result<f32, String> {
    match v.as_f64() {
        Some(x) if (0.0..=1.0).contains(&x) => Ok(x as f32),
        _ => Err(format!("expected a number from 0 to 1, got {v}")),
    }
}

fn parse_lanes(v: &Value) -> Result<[(f32, f32, f32); 8], String> {
    let bad = || format!("expected 8 [h, s, l] triples, got {v}");
    let items = v.as_array().filter(|a| a.len() == 8).ok_or_else(bad)?;
    let mut lanes = [(0.0, 0.0, 0.0); 8];
    for (i, item) in items.iter().enumerate() {
        let triple = item
            .as_array()
            .filter(|a| a.len() == 3)
            .ok_or_else(|| format!("lane {i}: expected [h, s, l], got {item}"))?;
        let c = |j: usize| parse_unit(&triple[j]).map_err(|e| format!("lane {i}: {e}"));
        lanes[i] = (c(0)?, c(1)?, c(2)?);
    }
    Ok(lanes)
}

fn parse_term_selection(v: &Value) -> Result<(u8, u8, u8, u8), String> {
    let obj = v.as_object().ok_or_else(|| {
        format!("expected {{\"color\": \"#rrggbb\", \"alpha\": 0..255}}, got {v}")
    })?;
    if let Some(k) = obj
        .keys()
        .find(|k| !matches!(k.as_str(), "color" | "alpha"))
    {
        return Err(format!("unknown key \"{k}\""));
    }
    let color = obj
        .get("color")
        .ok_or("missing key \"color\"")
        .map_err(str::to_owned)
        .and_then(|c| parse_rgb(c).map_err(|e| format!("\"color\": {e}")))?;
    let alpha = obj.get("alpha").ok_or("missing key \"alpha\"")?;
    let alpha = alpha
        .as_u64()
        .and_then(|a| u8::try_from(a).ok())
        .ok_or_else(|| format!("\"alpha\": expected an integer from 0 to 255, got {alpha}"))?;
    let (r, g, b) = split_rgb(color);
    Ok((r, g, b, alpha))
}

/// `Some(v)` from the file, else the base theme's value, else record the key
/// as missing (the placeholder is discarded with the theme).
fn pick<T: Copy + Default>(
    value: Option<T>,
    base: Option<&Theme>,
    get: fn(&Theme) -> T,
    key: &'static str,
    missing: &mut Vec<&'static str>,
) -> T {
    match (value, base) {
        (Some(v), _) => v,
        (None, Some(b)) => get(b),
        (None, None) => {
            missing.push(key);
            T::default()
        }
    }
}

// ── Field table ──────────────────────────────────────────────────────────
//
// One list of field names drives the key whitelist, the parser and the
// `Theme` literal. The literals below are exhaustive (no `..`), so adding a
// field to `Theme` or `SyntaxPalette` fails to compile until it is listed.

macro_rules! theme_fields {
    (
        rgb: [$($rgb:ident),* $(,)?],
        term: [$($term:ident),* $(,)?],
        syntax: [$($syn:ident),* $(,)?] $(,)?
    ) => {
        fn is_top_level_key(key: &str) -> bool {
            matches!(
                key,
                "slug" | "name" | "extends" | "dark" | "lane_hsl" | "avatar_sat"
                    | "avatar_light" | "term_selection" | "syntax"
                    $(| stringify!($rgb))* $(| stringify!($term))*
            )
        }

        #[derive(Default)]
        struct SyntaxPatch {
            $($syn: Option<u32>,)*
        }

        #[derive(Default)]
        struct Patch {
            dark: Option<bool>,
            $($rgb: Option<u32>,)*
            lane_hsl: Option<[(f32, f32, f32); 8]>,
            avatar_sat: Option<f32>,
            avatar_light: Option<f32>,
            $($term: Option<(u8, u8, u8)>,)*
            term_selection: Option<(u8, u8, u8, u8)>,
            /// `None` when the file has no `syntax` key at all.
            syntax: Option<SyntaxPatch>,
        }

        impl Patch {
            fn set(&mut self, key: &str, v: &Value) -> Result<(), String> {
                match key {
                    "dark" => self.dark = Some(parse_bool(v)?),
                    "lane_hsl" => self.lane_hsl = Some(parse_lanes(v)?),
                    "avatar_sat" => self.avatar_sat = Some(parse_unit(v)?),
                    "avatar_light" => self.avatar_light = Some(parse_unit(v)?),
                    "term_selection" => self.term_selection = Some(parse_term_selection(v)?),
                    "syntax" => self.syntax = Some(Self::syntax(v)?),
                    $(stringify!($rgb) => self.$rgb = Some(parse_rgb(v)?),)*
                    $(stringify!($term) => self.$term = Some(split_rgb(parse_rgb(v)?)),)*
                    other => return Err(format!("unknown key \"{other}\"")),
                }
                Ok(())
            }

            fn syntax(v: &Value) -> Result<SyntaxPatch, String> {
                let obj = v
                    .as_object()
                    .ok_or_else(|| format!("expected an object of \"#rrggbb\" colours, got {v}"))?;
                let mut patch = SyntaxPatch::default();
                for (key, value) in obj {
                    let colour = || parse_rgb(value).map_err(|e| format!("\"{key}\": {e}"));
                    match key.as_str() {
                        $(stringify!($syn) => patch.$syn = Some(colour()?),)*
                        other => return Err(format!("unknown key \"{other}\"")),
                    }
                }
                Ok(patch)
            }

            fn build(
                self,
                slug: Cow<'static, str>,
                name: Cow<'static, str>,
                base: Option<&Theme>,
            ) -> Result<Theme, String> {
                let mut missing: Vec<&'static str> = Vec::new();
                let syntax = match self.syntax {
                    Some(s) => SyntaxPalette {
                        $($syn: pick(
                            s.$syn,
                            base,
                            |t| t.syntax.$syn,
                            concat!("syntax.", stringify!($syn)),
                            &mut missing,
                        ),)*
                    },
                    None => match base {
                        Some(b) => b.syntax,
                        None => {
                            missing.push("syntax");
                            SyntaxPalette { $($syn: 0,)* }
                        }
                    },
                };
                let theme = Theme {
                    slug,
                    name,
                    dark: pick(self.dark, base, |t| t.dark, "dark", &mut missing),
                    $($rgb: pick(self.$rgb, base, |t| t.$rgb, stringify!($rgb), &mut missing),)*
                    lane_hsl: pick(self.lane_hsl, base, |t| t.lane_hsl, "lane_hsl", &mut missing),
                    avatar_sat: pick(self.avatar_sat, base, |t| t.avatar_sat, "avatar_sat", &mut missing),
                    avatar_light: pick(
                        self.avatar_light,
                        base,
                        |t| t.avatar_light,
                        "avatar_light",
                        &mut missing,
                    ),
                    $($term: pick(self.$term, base, |t| t.$term, stringify!($term), &mut missing),)*
                    term_selection: pick(
                        self.term_selection,
                        base,
                        |t| t.term_selection,
                        "term_selection",
                        &mut missing,
                    ),
                    syntax,
                };
                if missing.is_empty() {
                    Ok(theme)
                } else {
                    Err(format!(
                        "missing {} (required without \"extends\")",
                        missing
                            .iter()
                            .map(|k| format!("\"{k}\""))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                }
            }
        }
    };
}

theme_fields! {
    rgb: [
        bg_base, bg_row_alt, surface, selected, panel, sidebar, modal, modal_overlay,
        text_main, text_sub, text_muted, text_label,
        color_head, color_branch, color_remote, color_tag, selection_tint,
        color_success, color_warning, color_blocker, color_blocker_muted,
        diff_added_bg, diff_removed_bg, diff_hunk,
        change_added, change_modified, change_deleted, change_renamed, change_typechange,
        change_dir,
        accent,
    ],
    term: [
        term_bg, term_fg, term_cursor,
        term_black, term_red, term_green, term_yellow,
        term_blue, term_magenta, term_cyan, term_white,
        term_bright_black, term_bright_red, term_bright_green, term_bright_yellow,
        term_bright_blue, term_bright_magenta, term_bright_cyan, term_bright_white,
    ],
    syntax: [
        keyword, string, comment, type_name, function, number, operator, punctuation,
        variable, attribute,
    ],
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialise a theme into the file format, so the no-`extends` test uses
    /// every field the struct has rather than a hand-kept list.
    fn file_json(t: &Theme, slug: &str, name: &str) -> Value {
        let hex = |v: &Value| {
            let c = v.as_array().expect("rgb triple");
            Value::String(format!(
                "#{:02x}{:02x}{:02x}",
                c[0].as_u64().unwrap(),
                c[1].as_u64().unwrap(),
                c[2].as_u64().unwrap()
            ))
        };
        let Value::Object(mut map) = serde_json::to_value(t).unwrap() else {
            unreachable!()
        };
        for (key, value) in map.iter_mut() {
            if key.starts_with("term_") && key != "term_selection" {
                *value = hex(value);
            } else if let Some(n) = value.as_u64() {
                *value = Value::String(format!("#{n:06x}"));
            }
        }
        let sel = map["term_selection"].as_array().unwrap().clone();
        map.insert(
            "term_selection".into(),
            serde_json::json!({ "color": hex(&Value::Array(sel[..3].to_vec())), "alpha": sel[3] }),
        );
        let syntax = map["syntax"].as_object().unwrap().clone();
        map.insert(
            "syntax".into(),
            Value::Object(
                syntax
                    .into_iter()
                    .map(|(k, v)| (k, Value::String(format!("#{:06x}", v.as_u64().unwrap()))))
                    .collect(),
            ),
        );
        map.insert("slug".into(), slug.into());
        map.insert("name".into(), name.into());
        Value::Object(map)
    }

    #[test]
    fn a_full_file_without_extends_reproduces_every_field() {
        let src = &THEMES[3];
        let text = file_json(src, "mine", "Mine").to_string();
        let t = parse_theme(&text).expect("complete file parses");
        let (mut want, mut got) = (
            serde_json::to_value(src).unwrap(),
            serde_json::to_value(&t).unwrap(),
        );
        for v in [&mut want, &mut got] {
            v.as_object_mut().unwrap().remove("slug");
            v.as_object_mut().unwrap().remove("name");
        }
        assert_eq!(got, want);
        assert_eq!((&*t.slug, &*t.name), ("mine", "Mine"));
    }

    #[test]
    fn without_extends_every_missing_field_is_named() {
        let mut v = file_json(&THEMES[0], "mine", "Mine");
        let map = v.as_object_mut().unwrap();
        map.remove("accent");
        map.remove("term_selection");
        map["syntax"].as_object_mut().unwrap().remove("comment");
        let err = parse_theme(&v.to_string()).unwrap_err();
        assert!(err.contains("\"accent\""), "{err}");
        assert!(err.contains("\"term_selection\""), "{err}");
        assert!(err.contains("\"syntax.comment\""), "{err}");
        assert!(!err.contains("\"bg_base\""), "{err}");

        let err = parse_theme(r#"{"slug":"x","name":"X"}"#).unwrap_err();
        assert!(
            err.contains("\"syntax\"") && err.contains("\"dark\""),
            "{err}"
        );
    }

    #[test]
    fn extends_overrides_only_listed_keys() {
        let t = parse_theme(
            r##"{
              "slug": "my-dracula", "name": "My Dracula", "extends": "dracula",
              "bg_base": "#101010", "term_red": "#FF0000",
              "term_selection": {"color": "#010203", "alpha": 0},
              "syntax": {"keyword": "#abcdef"}
            }"##,
        )
        .expect("extends file parses");
        let base = THEMES.iter().find(|t| t.slug == "dracula").unwrap();
        assert_eq!(t.bg_base, 0x101010);
        assert_eq!(t.term_red, (0xff, 0, 0));
        assert_eq!(t.term_selection, (1, 2, 3, 0));
        assert_eq!(t.syntax.keyword, 0xabcdef);
        assert_eq!(t.syntax.string, base.syntax.string);
        assert_eq!(t.text_main, base.text_main);
        assert_eq!(t.lane_hsl, base.lane_hsl);
        assert_eq!(t.dark, base.dark);
    }

    #[test]
    fn invalid_values_are_rejected_with_the_key() {
        let cases = [
            (r##""bg_base": "#12345""##, "\"bg_base\""),
            (r##""bg_base": "123456""##, "\"bg_base\""),
            (r##""bg_base": null"##, "\"bg_base\""),
            (r##""term_fg": "#gg0000""##, "\"term_fg\""),
            (r##""dark": "yes""##, "\"dark\""),
            (r##""avatar_sat": 1.5"##, "\"avatar_sat\""),
            (r##""lane_hsl": [[0,0,0]]"##, "\"lane_hsl\""),
            (
                r##""term_selection": {"color": "#000000", "alpha": 256}"##,
                "\"alpha\"",
            ),
            (r##""term_selection": {"color": "#000000"}"##, "\"alpha\""),
            (
                r##""term_selection": {"color": "#000000", "alpha": 1, "x": 1}"##,
                "\"x\"",
            ),
            (
                r##""syntax": {"keyword": "#000000", "kw": "#000000"}"##,
                "\"kw\"",
            ),
            (r##""colour_head": "#000000""##, "\"colour_head\""),
        ];
        for (field, needle) in cases {
            let text = format!(r#"{{"slug":"x","name":"X","extends":"catppuccin",{field}}}"#);
            let err = parse_theme(&text).expect_err(field);
            assert!(err.contains(needle), "{field}: {err}");
        }
    }

    #[test]
    fn identity_and_extends_are_checked() {
        let cases = [
            (r#"{"name":"X","extends":"catppuccin"}"#, "\"slug\""),
            (r#"{"slug":"x","extends":"catppuccin"}"#, "\"name\""),
            (
                r#"{"slug":"x","name":"  ","extends":"catppuccin"}"#,
                "\"name\"",
            ),
            (
                r#"{"slug":"Bad Slug","name":"X","extends":"catppuccin"}"#,
                "\"slug\"",
            ),
            (r#"{"slug":"x","name":"X","extends":"nope"}"#, "\"extends\""),
            (
                r#"{"slug":"x","name":"X","extends":"xcode-dark"}"#,
                "\"extends\"",
            ),
            (
                r#"{"slug":"dracula","name":"X","extends":"catppuccin"}"#,
                "built-in",
            ),
            (
                r#"{"slug":"xcode-dark","name":"X","extends":"catppuccin"}"#,
                "built-in",
            ),
            (r#"[1]"#, "object"),
            (r#"{"slug":"#, "invalid JSON"),
        ];
        for (text, needle) in cases {
            let err = parse_theme(text).expect_err(text);
            assert!(err.contains(needle), "{text}: {err}");
        }
    }

    #[test]
    fn a_folder_loads_valid_files_and_isolates_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let ok =
            |slug: &str| format!(r#"{{"slug":"{slug}","name":"{slug}","extends":"one-dark"}}"#);
        std::fs::write(dir.path().join("b.json"), ok("dup")).unwrap();
        std::fs::write(dir.path().join("a.json"), ok("first")).unwrap();
        std::fs::write(dir.path().join("c.JSON"), ok("dup")).unwrap();
        std::fs::write(dir.path().join("d.json"), "{ broken").unwrap();
        std::fs::write(dir.path().join("e.json"), ok("last")).unwrap();
        std::fs::write(dir.path().join("notes.txt"), "ignored").unwrap();
        std::fs::create_dir(dir.path().join("nested.json")).unwrap();

        let (themes, errors) = load_dir(dir.path());
        let slugs: Vec<&str> = themes.iter().map(|t| &*t.slug).collect();
        assert_eq!(slugs, ["first", "dup", "last"]);
        let rejected: Vec<String> = errors.iter().map(ToString::to_string).collect();
        assert_eq!(rejected.len(), 2, "{rejected:?}");
        assert!(rejected[0].starts_with("c.JSON: slug \"dup\" is already used by b.json"));
        assert!(
            rejected[1].starts_with("d.json: invalid JSON"),
            "{rejected:?}"
        );

        let (themes, errors) = load_dir(&dir.path().join("absent"));
        assert!(themes.is_empty() && errors.is_empty());
    }
}
