//! Conversion between the mutable resolution buffer and C1's frozen draft.

use super::*;

impl ResolutionBuffer {
    /// Freeze one file's current resolution draft without repository I/O.
    pub fn conflict_draft(
        &self,
        path: &Path,
    ) -> Option<kagi_domain::conflict_family::ConflictDraft> {
        if let Some(raw) = self.raw_resolution(path) {
            return Some(kagi_domain::conflict_family::ConflictDraft::Raw {
                oid: raw.oid.to_string(),
                mode: raw.mode,
            });
        }
        self.resolved_text(path)
            .map(|text| kagi_domain::conflict_family::ConflictDraft::Text(text.into_bytes()))
    }

    /// Rebuild the minimum Backend buffer consumed by the established Save executor.
    pub(crate) fn from_conflict_draft(
        repo_path: &Path,
        path: &Path,
        draft: &kagi_domain::conflict_family::ConflictDraft,
    ) -> Result<Self, GitError> {
        let mut buffer = Self::new(repo_path);
        let mut file = FileResolution::empty();
        match draft {
            kagi_domain::conflict_family::ConflictDraft::Text(bytes) => {
                let text = std::str::from_utf8(bytes).map_err(|_| {
                    GitError::Other(format!(
                        "resolution for {} is not UTF-8 text",
                        path.display()
                    ))
                })?;
                file.result = Some(text_to_lines(text, LineOrigin::Manual));
            }
            kagi_domain::conflict_family::ConflictDraft::Raw { oid, mode } => {
                file.raw = true;
                file.raw_result = Some(RawResolution {
                    oid: git2::Oid::from_str(oid)
                        .map_err(|e| GitError::Other(format!("invalid resolution OID: {e}")))?,
                    mode: *mode,
                });
            }
        }
        buffer.files.insert(path.to_path_buf(), file);
        Ok(buffer)
    }
}
