//! One JSON/legacy identity interpretation for readers and destructive retention.
use super::{codec, retention::io, OpLogEntry};
use crate::GitError;

#[derive(Default)]
pub(super) struct Reader {
    index: u64,
    previous: Option<u64>,
}
impl Reader {
    pub(super) fn parse(&mut self, line: &str) -> Result<(OpLogEntry, bool), GitError> {
        let value: serde_json::Value = serde_json::from_str(line).map_err(io)?;
        let legacy = value.get("id").is_none();
        let mut entry = codec::from_value(value)
            .ok_or_else(|| io("unreadable log entry; preserving all roots"))?;
        if legacy {
            entry.id = self.index;
            entry.parent = self.previous;
        }
        self.previous = Some(entry.id);
        self.index += 1;
        Ok((entry, legacy))
    }
}

/// Parse one line in isolation for the bounded tail read (#499).
///
/// `None` for a legacy id-less line: its `id`/`parent` come from its position
/// in the file, which only a whole-file read knows.
pub(super) fn parse_standalone(line: &str) -> Result<Option<OpLogEntry>, GitError> {
    let (entry, legacy) = Reader::default().parse(line)?;
    Ok((!legacy).then_some(entry))
}
