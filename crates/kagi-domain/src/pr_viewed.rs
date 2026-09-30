//! Per-file "viewed" marks on a pull request (#351, ADR-0207).
//!
//! A mark is the file's head-side blob id at the moment the user marked it.
//! The file counts as viewed only while the PR head still has that exact
//! blob: once the head moves and the file's content changes, it is unviewed
//! again (GitHub's rule). A file whose blob is unchanged stays viewed across
//! head moves. Pure: storage lives in the UI layer.

use std::collections::BTreeMap;

/// The marks of one pull request: repository path → head blob id when marked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewedFiles {
    marks: BTreeMap<String, String>,
}

impl ViewedFiles {
    pub fn from_marks(marks: BTreeMap<String, String>) -> Self {
        Self { marks }
    }

    /// Every stored mark, including stale ones (blob since changed).
    pub fn marks(&self) -> &BTreeMap<String, String> {
        &self.marks
    }

    /// Viewed: marked, and the head still has the blob that was marked. An
    /// unknown head blob (`""`) is never viewed.
    pub fn is_viewed(&self, path: &str, head_blob: &str) -> bool {
        !head_blob.is_empty()
            && self
                .marks
                .get(path)
                .is_some_and(|marked| marked == head_blob)
    }

    /// Mark `path` viewed at `head_blob`, or clear its mark. Marking a file
    /// whose head blob is unknown clears it instead: there is nothing to
    /// compare a later head against.
    pub fn set(&mut self, path: &str, head_blob: &str, viewed: bool) {
        if viewed && !head_blob.is_empty() {
            self.marks.insert(path.to_string(), head_blob.to_string());
        } else {
            self.marks.remove(path);
        }
    }

    /// How many of `files` (path, head blob) are viewed.
    pub fn viewed_count<'a>(&self, files: impl IntoIterator<Item = (&'a str, &'a str)>) -> usize {
        files
            .into_iter()
            .filter(|(path, blob)| self.is_viewed(path, blob))
            .count()
    }
}

/// `<owner>-<repo>-<number>.json` for a pull request in `<host>/<owner>/<repo>`.
/// `None` when the identity is unreadable or a segment is not a plain name
/// (anything but ASCII letters, digits, `-`, `_`, `.`; or `.` / `..`), so the
/// name can never leave its directory.
pub fn viewed_file_name(base_repo: &str, number: u64) -> Option<String> {
    let mut segments = base_repo.split('/');
    let (_host, owner, repo) = (segments.next()?, segments.next()?, segments.next()?);
    if segments.next().is_some() || number == 0 {
        return None;
    }
    let plain = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    (plain(owner) && plain(repo)).then(|| format!("{owner}-{repo}-{number}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "1111111111111111111111111111111111111111";
    const B: &str = "2222222222222222222222222222222222222222";

    fn marked(path: &str, blob: &str) -> ViewedFiles {
        let mut viewed = ViewedFiles::default();
        viewed.set(path, blob, true);
        viewed
    }

    #[test]
    fn a_file_marked_at_the_current_head_blob_is_viewed() {
        assert!(marked("src/a.rs", A).is_viewed("src/a.rs", A));
    }

    #[test]
    fn a_changed_head_blob_unviews_the_file_and_the_same_blob_keeps_it() {
        let viewed = marked("src/a.rs", A);
        assert!(
            !viewed.is_viewed("src/a.rs", B),
            "the head changed the file"
        );
        assert!(
            viewed.is_viewed("src/a.rs", A),
            "a later head with the same blob"
        );
        assert_eq!(viewed.marks().get("src/a.rs").map(String::as_str), Some(A));
    }

    #[test]
    fn an_unmarked_file_or_an_unknown_blob_is_not_viewed() {
        let viewed = marked("src/a.rs", A);
        assert!(!viewed.is_viewed("src/b.rs", A));
        assert!(!viewed.is_viewed("src/a.rs", ""));
        let mut unknown = ViewedFiles::default();
        unknown.set("src/a.rs", "", true);
        assert!(
            unknown.marks().is_empty(),
            "nothing to compare a later head against"
        );
    }

    #[test]
    fn unmarking_and_re_marking_at_a_new_blob() {
        let mut viewed = marked("src/a.rs", A);
        viewed.set("src/a.rs", A, false);
        assert!(!viewed.is_viewed("src/a.rs", A));
        viewed.set("src/a.rs", B, true);
        assert!(viewed.is_viewed("src/a.rs", B));
        assert!(!viewed.is_viewed("src/a.rs", A));
    }

    #[test]
    fn the_count_uses_the_current_blobs() {
        let mut viewed = marked("a", A);
        viewed.set("b", A, true);
        let files = [("a", A), ("b", B), ("c", A)];
        assert_eq!(viewed.viewed_count(files), 1);
    }

    #[test]
    fn file_names_are_plain_and_stay_in_their_directory() {
        assert_eq!(
            viewed_file_name("github.com/acme/widgets", 7).as_deref(),
            Some("acme-widgets-7.json")
        );
        assert_eq!(
            viewed_file_name("ghe.example.com/a.b/c_d", 12).as_deref(),
            Some("a.b-c_d-12.json")
        );
        for bad in [
            "",
            "github.com/acme",
            "github.com/acme/widgets/extra",
            "github.com/../widgets",
            "github.com/acme/..",
            "github.com/ac me/widgets",
            "github.com/acme\\x/widgets",
        ] {
            assert_eq!(viewed_file_name(bad, 7), None, "{bad:?}");
        }
        assert_eq!(viewed_file_name("github.com/acme/widgets", 0), None);
    }
}
