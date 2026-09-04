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

/// Plan notes for the tag op family (create, push).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagNote {
    /// blocker (`plan_create_tag`) — a keyed tag-name validation failure.
    NameError(TagNameError),
    /// blocker (`plan_create_tag`) — the target commit does not exist.
    CommitMissing { sha: String },
    /// blocker (`plan_push_tag`) — no local tag by that name.
    NotFound { name: String },
    /// blocker (`plan_push_tag`) — the repository has no remote to push to.
    NoRemote,
    /// warning (`plan_push_tag`) — this leaves the machine. Said out loud
    /// because every other tag operation is purely local.
    PushRemoteSideEffect { remote: String, name: String },
    /// warning (`plan_push_tag`) — what the remote does if the tag is already
    /// there under a different commit.
    PushRejectedIfMoved { name: String },
}

impl TagNote {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            TagNote::NameError(e) => e.message_en(),
            TagNote::CommitMissing { sha } => {
                format!("Commit '{}' does not exist in this repository.", sha)
            }
            TagNote::NotFound { name } => {
                format!("No local tag named '{}'.", name)
            }
            TagNote::NoRemote => {
                "This repository has no remote configured, so there is nowhere to push a tag to."
                    .to_string()
            }
            TagNote::PushRemoteSideEffect { remote, name } => format!(
                "This publishes tag '{}' to '{}'. Unlike every other tag action in kagi, it leaves this machine and others will see it.",
                name, remote
            ),
            TagNote::PushRejectedIfMoved { name } => format!(
                "If '{}' already exists on the remote pointing at a different commit, the remote rejects the push rather than moving it. kagi never force-pushes a tag.",
                name
            ),
        }
    }
}

/// Plan titles for the tag op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagTitle {
    /// `plan_create_tag` — `Create tag '<name>' @ <at>`.
    CreateTag { name: String, at: String },
    /// `plan_push_tag` — `Push tag '<name>' to '<remote>'`.
    PushTag { name: String, remote: String },
}

impl TagTitle {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            TagTitle::CreateTag { name, at } => format!("Create tag '{}' @ {}", name, at),
            TagTitle::PushTag { name, remote } => {
                format!("Push tag '{}' to '{}'", name, remote)
            }
        }
    }
}

/// Recovery kinds for the tag op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagRecovery {
    /// `plan_create_tag` — the new tag can simply be `git tag -d`'d.
    CreateTag { name: String },
    /// `plan_push_tag` — undoing means deleting the tag on the remote, which
    /// only works if nobody has fetched it yet.
    PushTag { name: String, remote: String },
}

impl TagRecovery {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            TagRecovery::CreateTag { name } => format!(
                "The new tag '{}' can be removed without side effects:\n  git tag -d {}",
                name, name
            ),
            TagRecovery::PushTag { name, remote } => format!(
                "The tag can be removed from the remote:\n  git push {} --delete {}\nThis only helps until someone else fetches it — a published tag that others have pulled cannot be recalled.",
                remote, name
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
    fn push_tag_blockers() {
        assert_eq!(
            TagNote::NotFound {
                name: "v1.0.0".into()
            }
            .message_en(),
            "No local tag named 'v1.0.0'."
        );
        assert_eq!(
            TagNote::NoRemote.message_en(),
            "This repository has no remote configured, so there is nowhere to push a tag to."
        );
    }

    #[test]
    fn push_tag_warnings() {
        assert_eq!(
            TagNote::PushRemoteSideEffect {
                remote: "origin".into(),
                name: "v1.0.0".into()
            }
            .message_en(),
            "This publishes tag 'v1.0.0' to 'origin'. Unlike every other tag action in kagi, it leaves this machine and others will see it."
        );
        // kagi never force-pushes a tag — the note must say so.
        assert_eq!(
            TagNote::PushRejectedIfMoved {
                name: "v1.0.0".into()
            }
            .message_en(),
            "If 'v1.0.0' already exists on the remote pointing at a different commit, the remote rejects the push rather than moving it. kagi never force-pushes a tag."
        );
    }

    #[test]
    fn push_tag_title_and_recovery() {
        assert_eq!(
            TagTitle::PushTag {
                name: "v1.0.0".into(),
                remote: "origin".into()
            }
            .message_en(),
            "Push tag 'v1.0.0' to 'origin'"
        );
        assert_eq!(
            TagRecovery::PushTag {
                name: "v1.0.0".into(),
                remote: "origin".into()
            }
            .message_en(),
            "The tag can be removed from the remote:\n  git push origin --delete v1.0.0\nThis only helps until someone else fetches it — a published tag that others have pulled cannot be recalled."
        );
    }

    #[test]
    fn create_tag_recovery() {
        assert_eq!(
            TagRecovery::CreateTag {
                name: "v1.0.0".into()
            }
            .message_en(),
            "The new tag 'v1.0.0' can be removed without side effects:\n  git tag -d v1.0.0"
        );
    }
}
