//! Pure Issues Composer rules. Persistence and input entities belong to callers.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IssueDraft {
    pub title: String,
    pub body: String,
    /// Capture on submission; a late success must not erase subsequent edits.
    pub revision: u64,
}

impl IssueDraft {
    /// Replace the editor contents, advancing the revision only for an actual edit.
    pub fn update(&mut self, title: String, body: String) -> bool {
        if self.title == title && self.body == body {
            return false;
        }
        self.title = title;
        self.body = body;
        self.revision = self
            .revision
            .checked_add(1)
            .expect("Issue draft revision exhausted");
        true
    }

    /// Keep an explicit title; otherwise use the first nonempty body line.
    /// The fallback is at most 60 Unicode scalar values, never a byte slice.
    /// This projection does not modify the original body or draft revision.
    pub fn effective_title(&self) -> String {
        let explicit = self.title.trim();
        if !explicit.is_empty() {
            return explicit.to_owned();
        }
        self.body
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or_default()
            .chars()
            .take(60)
            .collect()
    }

    /// Clear only the draft that was sent. Failed/unknown writes must not call this.
    pub fn clear_if_revision(&mut self, sent_revision: u64) -> bool {
        if self.revision != sent_revision {
            return false;
        }
        self.title.clear();
        self.body.clear();
        self.revision = self
            .revision
            .checked_add(1)
            .expect("Issue draft revision exhausted");
        true
    }
}

/// Wrap multiline clipboard code without changing its contents. Single lines and
/// Markdown already containing a complete fenced block remain byte-for-byte intact.
/// Callers decide where to insert this text; this never rewrites the whole draft.
pub fn fenced_code_paste(text: &str, filename_hint: Option<&str>) -> String {
    if text.lines().count() < 2 || contains_fenced_block(text) {
        return text.to_owned();
    }
    let longest_ticks = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(3.max(longest_ticks + 1));
    let language = filename_hint
        .and_then(language_from_filename)
        .unwrap_or_else(|| infer_language(text));
    let separator = if text.ends_with('\n') { "" } else { "\n" };
    format!("{fence}{language}\n{text}{separator}{fence}\n")
}

fn contains_fenced_block(text: &str) -> bool {
    let mut opener = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if line.len() - trimmed.len() > 3 {
            continue;
        }
        let marker = trimmed.chars().next().unwrap_or_default();
        if marker != '`' && marker != '~' {
            continue;
        }
        let count = trimmed.chars().take_while(|c| *c == marker).count();
        if count < 3 {
            continue;
        }
        let suffix = &trimmed[count..]; // markers are ASCII
        match opener {
            Some((opening_marker, opening_count)) => {
                if marker == opening_marker && count >= opening_count && suffix.trim().is_empty() {
                    return true;
                }
            }
            None if marker != '`' || !suffix.contains('`') => opener = Some((marker, count)),
            None => {}
        }
    }
    false
}

fn language_from_filename(filename: &str) -> Option<&'static str> {
    let extension = filename.rsplit('.').next()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "rs" => "rust",
        "py" => "python",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" => "typescript",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cc" | "cpp" | "hpp" => "cpp",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "sh" | "bash" | "zsh" => "bash",
        "html" => "html",
        "css" => "css",
        "sql" => "sql",
        "md" => "markdown",
        _ => return None,
    })
}

fn infer_language(text: &str) -> &'static str {
    let first = text.trim_start();
    if first.starts_with("#!/bin/sh") || first.starts_with("#!/bin/bash") {
        "bash"
    } else if first.starts_with("def ") || first.starts_with("async def ") {
        "python"
    } else if first.starts_with("fn ") || first.starts_with("pub fn ") {
        "rust"
    } else if first.starts_with("function ") || first.starts_with("export function ") {
        "javascript"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_defaults_to_first_nonempty_line_without_mutating_body() {
        let draft = IssueDraft {
            body: "\n  USB 復旧に失敗  \r\n再現手順\n".into(),
            ..Default::default()
        };
        let original = draft.clone();
        assert_eq!(draft.effective_title(), "USB 復旧に失敗");
        assert_eq!(draft, original);
        assert_eq!(IssueDraft::default().effective_title(), "");
    }

    #[test]
    fn fallback_title_truncates_unicode_not_bytes() {
        let draft = IssueDraft {
            body: "鍵🔑".repeat(40),
            ..Default::default()
        };
        assert_eq!(draft.effective_title(), "鍵🔑".repeat(30));
        assert_eq!(draft.body.chars().count(), 80);
    }

    #[test]
    fn explicit_title_is_independent_and_not_truncated() {
        let mut draft = IssueDraft::default();
        let body = "Original body\nDo not rewrite it".to_owned();
        draft.update(" ".into(), body.clone());
        assert_eq!(draft.effective_title(), "Original body");
        draft.update(format!(" {} ", "題".repeat(70)), body.clone());
        assert_eq!(draft.effective_title(), "題".repeat(70));
        assert_eq!(draft.body, body);
    }

    #[test]
    fn successful_submission_does_not_erase_newer_edits() {
        let mut draft = IssueDraft::default();
        assert!(draft.update("title".into(), "sent".into()));
        let sent = draft.revision;
        assert!(!draft.update("title".into(), "sent".into()));
        assert_eq!(draft.revision, sent);
        draft.update("title".into(), "new edit".into());
        assert!(!draft.clear_if_revision(sent));
        assert_eq!(draft.body, "new edit");
        let current = draft.revision;
        assert!(draft.clear_if_revision(current));
        assert!(draft.title.is_empty() && draft.body.is_empty());
        assert!(!draft.clear_if_revision(current));
    }

    #[test]
    fn editing_away_then_back_is_still_a_new_draft() {
        let mut draft = IssueDraft::default();
        draft.update(String::new(), "original".into());
        let sent = draft.revision;
        draft.update(String::new(), "different".into());
        draft.update(String::new(), "original".into());
        assert!(!draft.clear_if_revision(sent));
    }

    #[test]
    fn empty_and_single_line_paste_are_unchanged() {
        for text in ["", "text", "一行だけ", "text\n", "text\r\n"] {
            assert_eq!(fenced_code_paste(text, Some("file.rs")), text);
        }
    }

    #[test]
    fn multiline_uses_hint_and_preserves_code_bytes() {
        assert_eq!(
            fenced_code_paste("fn main() {\n    run();\n}", Some("src/main.rs")),
            "```rust\nfn main() {\n    run();\n}\n```\n"
        );
        assert_eq!(
            fenced_code_paste("one\r\ntwo\r\n", None),
            "```\none\r\ntwo\r\n```\n"
        );
        assert_eq!(
            fenced_code_paste("one\ntwo", Some("FILE.PY")),
            "```python\none\ntwo\n```\n"
        );
    }

    #[test]
    fn existing_fenced_markdown_is_not_double_wrapped() {
        for text in [
            "```rust\nfn main() {}\n```",
            "Before\n~~~python\nx = 1\n~~~\nAfter",
            "````markdown\n```rust\nfn main() {}\n```\n````\n",
        ] {
            assert_eq!(fenced_code_paste(text, None), text);
        }
    }

    #[test]
    fn fence_cannot_collide_with_backticks_in_code() {
        assert_eq!(
            fenced_code_paste("let marker = \"```\";\nrun();", None),
            "````\nlet marker = \"```\";\nrun();\n````\n"
        );
        assert_eq!(
            fenced_code_paste("```rust\nfn main() {}", None),
            "````\n```rust\nfn main() {}\n````\n"
        );
    }

    #[test]
    fn unknown_hint_falls_back_to_conservative_heuristics() {
        for (text, language) in [
            ("def f():\n    pass", "python"),
            ("pub fn f() {\n}", "rust"),
            ("function f() {\n}", "javascript"),
            ("#!/bin/bash\necho ok", "bash"),
            ("some text\nmore text", ""),
        ] {
            let output = fenced_code_paste(text, Some("unknown.ext"));
            assert!(output.starts_with(&format!("```{language}\n")));
        }
    }
}
