//! Bounded tail reads (#499).
//!
//! Every reader wants the newest few entries — `read_oplog_tail(1)` inside the
//! locked append picks the next id, the UI loads a page, the CLI/MCP show a
//! limit — but the previous implementation parsed the whole file for each of
//! them, so an append cost grew with total history.
//!
//! This scans backwards from the end of the file in chunks and stops as soon as
//! `n` matching entries are in hand. Two cases cannot be answered from a window
//! and fall back to the whole-file read that owns them:
//!
//! - a **legacy id-less line** (pre-ADR-0149) takes its `id`/`parent` from its
//!   position among the successfully parsed lines, which a backward scan does
//!   not know;
//! - **non-UTF-8 bytes**, which `read_to_string` rejects for the entire file.
//!
//! An unterminated final line (a process killed mid-append) is handled exactly
//! as before: it fails to parse and is skipped, leaving every complete entry
//! intact. Reads take no lock — the sidecar lock in `retention` serializes
//! writers (#568) and a single `write_all` of one line is what makes a
//! concurrent reader see whole lines only.

use super::{log_file_path, reading, OpLogEntry};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Backward read granularity. A receipt line is a few hundred bytes, so one
/// chunk already covers the common `n = 1` tail read in a single read.
const CHUNK: u64 = 8 * 1024;

pub(super) struct Tail {
    pub(super) entries: Vec<OpLogEntry>,
    /// Log bytes read to produce `entries` — bounded on the backward scan,
    /// the whole file on the fallback. The measurement behind #499.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) bytes: u64,
}

/// The newest up-to-`n` entries matching `keep`, newest first.
///
/// Returns an empty `Tail` when the log path cannot be resolved, the file does
/// not exist, or it cannot be read — same fail-safe as the readers before.
pub(super) fn read(n: usize, keep: &dyn Fn(&OpLogEntry) -> bool) -> Tail {
    match log_file_path() {
        Ok(Some(path)) => read_path(&path, n, keep),
        Ok(None) | Err(_) => Tail {
            entries: Vec::new(),
            bytes: 0,
        },
    }
}

pub(super) fn read_path(path: &Path, n: usize, keep: &dyn Fn(&OpLogEntry) -> bool) -> Tail {
    match bounded(path, n, keep) {
        Some(tail) => tail,
        None => whole(path, n, keep),
    }
}

/// Backward scan. `None` means the window cannot be interpreted on its own and
/// the caller must read the whole file.
fn bounded(path: &Path, n: usize, keep: &dyn Fn(&OpLogEntry) -> bool) -> Option<Tail> {
    let mut entries = Vec::new();
    let mut bytes = 0;
    if n == 0 {
        return Some(Tail { entries, bytes });
    }
    let mut file = File::open(path).ok()?;
    let mut pos = file.metadata().ok()?.len();
    // Bytes of a line whose start lies in a chunk not read yet.
    let mut pending: Vec<u8> = Vec::new();
    while pos > 0 && entries.len() < n {
        let span = CHUNK.min(pos);
        pos -= span;
        let mut buffer = vec![0u8; span as usize];
        file.seek(SeekFrom::Start(pos)).ok()?;
        file.read_exact(&mut buffer).ok()?;
        bytes += span;
        buffer.extend_from_slice(&pending);
        let split = match buffer.iter().position(|byte| *byte == b'\n') {
            // Everything up to the first newline continues an earlier line,
            // unless this chunk starts the file.
            Some(index) if pos > 0 => index + 1,
            None if pos > 0 => {
                pending = buffer;
                continue;
            }
            _ => 0,
        };
        let complete = buffer.split_off(split);
        pending = buffer;
        for line in complete.split(|byte| *byte == b'\n').rev() {
            if entries.len() == n {
                break;
            }
            let text = std::str::from_utf8(line).ok()?;
            if text.trim().is_empty() {
                continue;
            }
            match reading::parse_standalone(text) {
                Ok(Some(entry)) => {
                    if keep(&entry) {
                        entries.push(entry);
                    }
                }
                // Legacy line: its identity depends on its file position.
                Ok(None) => return None,
                // Unreadable line: skipped, as the whole-file read does.
                Err(_) => continue,
            }
        }
    }
    Some(Tail { entries, bytes })
}

/// Whole-file read, oldest-first, reconstructing id/parent for pre-ADR-0149
/// lines that lack an explicit `id`: id = 0-based index of the entry in the
/// file, parent = previous entry's id. New lines carry their own id/parent.
fn whole(path: &Path, n: usize, keep: &dyn Fn(&OpLogEntry) -> bool) -> Tail {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => {
            return Tail {
                entries: Vec::new(),
                bytes: 0,
            }
        }
    };
    // Every parsed line advances the legacy index, so `keep` filters after the
    // identity is reconstructed — never before.
    let mut reader = reading::Reader::default();
    let mut entries = Vec::new();
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        if let Ok((entry, _)) = reader.parse(line) {
            if keep(&entry) {
                entries.push(entry);
            }
        }
    }
    let start = entries.len().saturating_sub(n);
    let mut entries = entries.split_off(start);
    entries.reverse();
    Tail {
        entries,
        bytes: content.len() as u64,
    }
}
