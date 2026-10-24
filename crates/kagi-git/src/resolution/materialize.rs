use std::path::Path;

use git2::{FileMode, IndexEntry, MergeFileInput, MergeFileOptions, Repository};

use super::conflict_path;

/// Index-derived conflict text and the collision-free width of its delimiters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictMaterialization {
    pub text: String,
    pub marker_width: usize,
}

/// Prefer zdiff3, falling back to standard markers with the same safe width.
/// Single-sided conflicts and sources that cannot be materialized return `None`.
pub(super) fn materialize_zdiff3(
    repo: &Repository,
    path: &Path,
) -> Option<ConflictMaterialization> {
    let index = repo.index().ok()?;
    for entry in index.conflicts().ok()?.flatten() {
        if conflict_path(&entry)? != path {
            continue;
        }
        let (Some(our), Some(their)) = (entry.our, entry.their) else {
            return None;
        };
        let ancestor = entry.ancestor;
        if ancestor
            .iter()
            .chain([&our, &their])
            .any(|entry| !matches!(entry.mode, 0o100644 | 0o100755))
        {
            return None;
        }
        let ancestor_blob = match &ancestor {
            Some(entry) => Some(repo.find_blob(entry.id).ok()?),
            None => None,
        };
        let our_blob = repo.find_blob(our.id).ok()?;
        let their_blob = repo.find_blob(their.id).ok()?;
        let sources = [
            ancestor_blob
                .as_ref()
                .map_or(&b""[..], |blob| blob.content()),
            our_blob.content(),
            their_blob.content(),
        ];
        let marker_width = marker_width(&sources)?;
        let path = path.to_string_lossy();
        let ancestor_in = merge_input(sources[0], &path, ancestor.as_ref());
        let our_in = merge_input(sources[1], &path, Some(&our));
        let their_in = merge_input(sources[2], &path, Some(&their));
        let mut opts = MergeFileOptions::new();
        opts.ancestor_label("Base")
            .our_label("Current")
            .their_label("Incoming")
            .marker_size(marker_width)
            .style_zdiff3(true);
        let result = git2::merge_file(&ancestor_in, &our_in, &their_in, Some(&mut opts))
            .or_else(|_| {
                opts.style_zdiff3(false).style_standard(true);
                git2::merge_file(&ancestor_in, &our_in, &their_in, Some(&mut opts))
            })
            .ok()?;
        return Some(ConflictMaterialization {
            text: String::from_utf8_lossy(result.content()).into_owned(),
            marker_width: usize::from(marker_width),
        });
    }
    None
}

fn merge_input<'a>(
    content: &'a [u8],
    path: &str,
    entry: Option<&IndexEntry>,
) -> MergeFileInput<'a> {
    let mut input = MergeFileInput::new();
    let mode = if entry.is_some_and(|entry| entry.mode == 0o100755) {
        FileMode::BlobExecutable
    } else {
        FileMode::Blob
    };
    input.content(content).path(path).mode(Some(mode));
    input
}

/// Scan each source once; literal delimiter prefixes must always be shorter
/// than generated markers. Refuse widths that libgit2's u16 cannot represent.
fn marker_width(sources: &[&[u8]]) -> Option<u16> {
    let mut longest = 0;
    for source in sources {
        for line in source.split(|byte| *byte == b'\n') {
            let Some(first @ (b'<' | b'=' | b'>' | b'|')) = line.first() else {
                continue;
            };
            longest = longest.max(line.iter().take_while(|byte| *byte == first).count());
        }
    }
    u16::try_from(longest.checked_add(1)?.max(7)).ok()
}
