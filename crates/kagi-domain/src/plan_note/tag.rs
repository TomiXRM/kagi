//! TagNote — create-tag-here (ADR-0129-style typed plan text, new op family).
//!
//! `create-tag`'s tag-name-validity blockers reuse the same shape as
//! `create-branch`'s (empty / invalid ref / already exists), but tags and
//! branches share no ref namespace, so they get their own
//! [`TagNameError`] rather than overloading `BranchNameError`.

/// Keyed tag-name validation errors (mirrors `BranchNameError`, scoped to
/// `refs/tags/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagNameError {
    /// The name is empty.
    Empty,
    /// `git2::Reference::is_valid_name("refs/tags/<name>")` rejected it.
    InvalidRef(String),
    /// The name starts with `-` (ambiguous as a CLI flag, even though git2
    /// accepts it as a valid ref name).
    LeadingDash(String),
    /// A tag with this name already exists.
    Exists(String),
}

impl TagNameError {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            TagNameError::Empty => "Tag name cannot be empty.".to_string(),
            TagNameError::InvalidRef(name) => {
                format!("'{}' is not a valid tag name.", name)
            }
            TagNameError::LeadingDash(name) => format!(
                "Tag name '{}' starts with '-', which is ambiguous on the command line.",
                name
            ),
            TagNameError::Exists(name) => format!("A tag named '{}' already exists.", name),
        }
    }
}

/// Plan notes for the tag op family (create).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagNote {
    /// blocker (`plan_create_tag`) — a keyed tag-name validation failure.
    NameError(TagNameError),
    /// blocker (`plan_create_tag`) — the target commit does not exist.
    CommitMissing { sha: String },
}

impl TagNote {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            TagNote::NameError(e) => e.message_en(),
            TagNote::CommitMissing { sha } => {
                format!("Commit '{}' does not exist in this repository.", sha)
            }
        }
    }
}

/// Plan titles for the tag op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagTitle {
    /// `plan_create_tag` — `Create tag '<name>' @ <at>`.
    CreateTag { name: String, at: String },
}

impl TagTitle {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            TagTitle::CreateTag { name, at } => format!("Create tag '{}' @ {}", name, at),
        }
    }
}

/// Recovery kinds for the tag op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagRecovery {
    /// `plan_create_tag` — the new tag can simply be `git tag -d`'d.
    CreateTag { name: String },
}

impl TagRecovery {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            TagRecovery::CreateTag { name } => format!(
                "The new tag '{}' can be removed without side effects:\n  git tag -d {}\n(Tag creation does not move HEAD or alter the working tree.)",
                name, name
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_error_messages() {
        assert_eq!(
            TagNameError::Empty.message_en(),
            "Tag name cannot be empty."
        );
        assert_eq!(
            TagNameError::InvalidRef("bad name".into()).message_en(),
            "'bad name' is not a valid tag name."
        );
        assert_eq!(
            TagNameError::LeadingDash("-oops".into()).message_en(),
            "Tag name '-oops' starts with '-', which is ambiguous on the command line."
        );
        assert_eq!(
            TagNameError::Exists("v1.0.0".into()).message_en(),
            "A tag named 'v1.0.0' already exists."
        );
    }

    #[test]
    fn commit_missing() {
        assert_eq!(
            TagNote::CommitMissing {
                sha: "a1b2c3d4".into()
            }
            .message_en(),
            "Commit 'a1b2c3d4' does not exist in this repository."
        );
    }

    #[test]
    fn create_tag_title() {
        assert_eq!(
            TagTitle::CreateTag {
                name: "v1.0.0".into(),
                at: "a1b2c3d4".into()
            }
            .message_en(),
            "Create tag 'v1.0.0' @ a1b2c3d4"
        );
    }

    #[test]
    fn create_tag_recovery() {
        assert_eq!(
            TagRecovery::CreateTag {
                name: "v1.0.0".into()
            }
            .message_en(),
            "The new tag 'v1.0.0' can be removed without side effects:\n  git tag -d v1.0.0\n(Tag creation does not move HEAD or alter the working tree.)"
        );
    }
}
