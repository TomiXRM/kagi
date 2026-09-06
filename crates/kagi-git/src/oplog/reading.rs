//! One JSON/legacy identity interpretation for readers and destructive retention.
use super::{parse_oplog_line, retention::io, OpLogEntry};
use crate::GitError;

#[derive(Default)]
pub(super) struct Reader {
    index: u64,
    previous: Option<u64>,
}
impl Reader {
    pub(super) fn parse(&mut self, line: &str) -> Result<(OpLogEntry, bool), GitError> {
        let value: serde_json::Value = serde_json::from_str(line).map_err(io)?;
        let refs = match value.get("backup_refs") {
            None => Vec::new(),
            Some(value) => value
                .as_array()
                .ok_or_else(|| io("invalid backup_refs; preserving all roots"))?
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| io("invalid backup_refs; preserving all roots"))
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        // Canonical JSON also makes legacy scalar parsing independent of whitespace.
        let canonical = serde_json::to_string(&value).map_err(io)?;
        let mut entry = parse_oplog_line(&canonical)
            .ok_or_else(|| io("unreadable log entry; preserving all roots"))?;
        entry.backup_refs = refs;
        let legacy = value.get("id").is_none();
        if legacy {
            entry.id = self.index;
            entry.parent = self.previous;
        }
        self.previous = Some(entry.id);
        self.index += 1;
        Ok((entry, legacy))
    }
}
