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

    /// Keep an explicit title; otherwise use the first meaningful body line.
    /// Markdown structure prefixes are omitted and fence marker lines are skipped.
    /// The fallback is at most 60 Unicode scalar values, never a byte slice.
    /// This projection does not modify the original body or draft revision.
    pub fn effective_title(&self) -> String {
        let explicit = self.title.trim();
        if !explicit.is_empty() {
            return explicit.to_owned();
        }
        self.body
            .lines()
            .find_map(title_candidate)
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

fn title_candidate(line: &str) -> Option<&str> {
    let mut candidate = line.trim();
    loop {
        let stripped = strip_markdown_prefix(candidate);
        if stripped == candidate {
            break;
        }
        candidate = stripped.trim_start();
    }
    if candidate.is_empty() || is_fence_marker(candidate) {
        None
    } else {
        Some(candidate.trim_end())
    }
}

fn strip_markdown_prefix(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix('>') {
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return rest;
        }
    }

    let heading_len = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&heading_len) {
        let rest = &line[heading_len..]; // '#' is ASCII
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return rest;
        }
    }

    if let Some(rest) = line.strip_prefix(|c| matches!(c, '-' | '+' | '*')) {
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return rest;
        }
    }

    let digit_len = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digit_len > 0 {
        let rest = &line[digit_len..]; // digits are ASCII
        if let Some(rest) = rest.strip_prefix(|c| matches!(c, '.' | ')')) {
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                return rest;
            }
        }
    }

    line
}

fn is_fence_marker(line: &str) -> bool {
    let marker = line.chars().next().unwrap_or_default();
    (marker == '`' || marker == '~') && line.chars().take_while(|c| *c == marker).count() >= 3
}

/// Wrap multiline clipboard code without changing its contents. Single lines and
/// Markdown already containing a complete fenced block remain byte-for-byte intact.
/// Callers decide where to insert this text; this never rewrites the whole draft.
/// `filename_hint` is reserved for future context-aware callers; the production UI
/// currently passes `None`.
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

/// What a multiline clipboard becomes when it is dropped on the New Issue
/// *title*: its first line names the Issue, everything after that first
/// newline is its body, byte-for-byte (#751).
///
/// The title is a single-line input, so pasting a whole Issue into it
/// otherwise flattens the document into the title and leaves the body empty.
/// Splitting — rather than fencing, which stays [`fenced_code_paste`]'s rule
/// for the body input — is what keeps the pasted Markdown renderable in
/// Preview: an outer fence would draw the whole clipboard as code.
///
/// CRLF is normalised so the split never strands a `\r` at the end of the
/// title. Nothing else is rewritten: no trimming, no dropped blank line, no
/// added trailing newline. `None` means "one line" — an ordinary paste the
/// input itself still owns, on the same rule `fenced_code_paste` uses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TitlePaste {
    /// Inserted into the title input at its current selection.
    pub title: String,
    /// Inserted into the body input, newlines and Markdown intact.
    pub body: String,
}

/// Split a title paste; see [`TitlePaste`].
pub fn title_paste_split(text: &str) -> Option<TitlePaste> {
    // Split before normalising. Every paste reaches this, most of them one
    // line, so the rejected path must not build anything: it stops at the
    // first newline and allocates nothing. `body.is_empty()` is
    // `lines().count() < 2` without the second walk — a trailing newline does
    // not start a line. The two `String`s the caller gets are the only
    // allocations, and normalising the body is part of copying it, not an
    // extra pass over a document-sized temporary.
    let (title, body) = text.split_once('\n')?;
    if body.is_empty() {
        return None;
    }
    Some(TitlePaste {
        title: title.strip_suffix('\r').unwrap_or(title).to_owned(),
        body: body.replace("\r\n", "\n"),
    })
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
    fn code_only_body_skips_fence_markers_for_its_title() {
        let draft = IssueDraft {
            body: "```rust\nfn main() {\n    let ptr = *ptr;\n}\n```\n".into(),
            ..Default::default()
        };
        let original = draft.clone();

        assert_eq!(draft.effective_title(), "fn main() {");
        assert_eq!(draft, original);
    }

    #[test]
    fn fallback_title_strips_markdown_structure_prefixes() {
        for (body, expected) in [
            ("# Heading", "Heading"),
            ("- list item", "list item"),
            ("1. ordered item", "ordered item"),
            ("> quoted item", "quoted item"),
            ("> - ## nested item", "nested item"),
        ] {
            let draft = IssueDraft {
                body: body.into(),
                ..Default::default()
            };
            assert_eq!(draft.effective_title(), expected);
        }
    }

    #[test]
    fn fallback_title_keeps_code_that_only_resembles_markdown() {
        for line in ["*ptr", "#include <stdio.h>", "-negative", ">= threshold"] {
            let draft = IssueDraft {
                body: line.into(),
                ..Default::default()
            };
            assert_eq!(draft.effective_title(), line);
        }
    }

    #[test]
    fn empty_or_syntax_only_body_has_no_effective_title() {
        for body in ["", " \n\t", "\n```rust\n```\n~~~\n~~~\n#\n-\n>\n"] {
            let draft = IssueDraft {
                body: body.into(),
                ..Default::default()
            };
            assert_eq!(draft.effective_title(), "");
        }
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

    #[test]
    fn single_line_title_paste_stays_the_input_s_own_paste() {
        for text in ["", "text", "一行だけ", "text\n", "text\r\n"] {
            assert_eq!(title_paste_split(text), None);
        }
    }

    #[test]
    fn multiline_title_paste_keeps_the_body_verbatim() {
        assert_eq!(
            title_paste_split("USB が復帰しない\n## 再現\n- [ ] 抜き差し\n"),
            Some(TitlePaste {
                title: "USB が復帰しない".into(),
                body: "## 再現\n- [ ] 抜き差し\n".into(),
            }),
            "the body keeps its Markdown and its trailing newline: Preview renders it"
        );
        assert_eq!(
            title_paste_split("Title\r\nBody line\r\n"),
            Some(TitlePaste {
                title: "Title".into(),
                body: "Body line\n".into(),
            }),
            "CRLF must not strand a carriage return at the end of the title"
        );
        assert_eq!(
            title_paste_split("Title\n\nFirst paragraph"),
            Some(TitlePaste {
                title: "Title".into(),
                body: "\nFirst paragraph".into(),
            }),
            "the blank line separating title from prose is the author's, not ours"
        );
    }

    #[test]
    fn title_paste_never_fences_what_the_body_would() {
        let code = "fn main() {\n    run();\n}";
        let split = title_paste_split(code).expect("multiline");
        assert_eq!(split.title, "fn main() {");
        assert_eq!(split.body, "    run();\n}");
        assert_ne!(
            fenced_code_paste(code, None),
            code,
            "the body input still fences the same clipboard"
        );
    }
}
