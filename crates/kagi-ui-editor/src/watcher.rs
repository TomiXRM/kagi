//! The external-change banner: what a working-tree watcher event means for an
//! open buffer, and the Reload that answers it.
//!
//! Split out of `lib.rs` (its LOC ceiling is a shrink-only ratchet) when #736
//! made the watcher path probe the file instead of trusting the event.

use std::path::PathBuf;

use gpui::{AppContext as _, Context};

use super::EditorWorkspaceView;

/// One dirty buffer's file, and the identity of the buffer that asked for it.
/// `loaded` is the text that buffer read from disk — `None` when it had none
/// (binary, too large, unreadable), which counts as changed.
struct Probe {
    path: PathBuf,
    generation: u64,
    sig: u64,
    loaded: Option<String>,
}

impl EditorWorkspaceView {
    /// FS-watcher nudge (T-WS-EDITOR-002 §4), called from
    /// `KagiApp::refresh_working_tree_external` on every debounced
    /// `WatchEvent::WorkTree`. Always refreshes the tree/badges (also covers
    /// the tree-side of a just-completed save); additionally re-reads the
    /// open file's content when the buffer is clean (the tree reload itself
    /// is highlight-only per the spec change), or raises the "changed on
    /// disk" banner when it's dirty (never clobbers an edit).
    ///
    /// The event says only "something under the working tree changed", and
    /// kagi's own fetch or save is enough to fire it — so a dirty buffer is
    /// not bannered on the event alone (#736). Each dirty buffer's own file is
    /// re-read in the background and compared against the text that buffer
    /// loaded; only a real difference raises the banner. A buffer with no
    /// loaded text (binary, too large, unreadable) keeps the old conservative
    /// banner: it cannot be proven unchanged, and an edit must never be
    /// clobbered.
    ///
    /// The comparison is against the loaded text itself, not `content_sig`,
    /// because that hash includes the path: a rename remaps a dirty buffer's
    /// path without reloading it, and a path-keyed hash would then differ from
    /// every future read of identical bytes. The probe still carries the
    /// `(generation, content_sig)` it started from and drops its result if
    /// either moved, so a save — or a close and reopen — landing mid-flight
    /// cannot banner the buffer that replaced it.
    pub fn on_worktree_changed(&mut self, cx: &mut Context<Self>) {
        // Every event supersedes the probes before it, including one that finds
        // nothing to probe: a buffer can go clean and come back dirty while an
        // older probe is still in flight, and its answer describes a file state
        // two events ago.
        self.probe_req = self.probe_req.wrapping_add(1);
        let req = self.probe_req;
        self.start_load(cx);
        if !self.dirty {
            // Clean buffer: re-read content + diff. The content-sig guard in
            // `sync_editor` makes an unchanged re-read a no-op push, so the
            // cursor/scroll survive routine watcher ticks.
            self.load_selected(cx);
        }
        // Backgrounded tabs are probed the same way; one that really did
        // change gets its banner when the user comes back to it. A clean one
        // re-reads on activation anyway (`open_tab`'s clean-refresh).
        let mut probes: Vec<Probe> = Vec::new();
        if self.dirty {
            if let Some(path) = self.open_path.clone() {
                probes.push(Probe {
                    path,
                    generation: self.buf_gen,
                    sig: self.content_sig,
                    loaded: self.content.clone(),
                });
            }
        }
        for (path, buf) in &self.tab_cache {
            if buf.dirty {
                probes.push(Probe {
                    path: path.clone(),
                    generation: buf.generation,
                    sig: buf.content_sig,
                    loaded: buf.content.clone(),
                });
            }
        }
        if probes.is_empty() {
            cx.notify();
            return;
        }
        let repo_path = self.repo_path.clone();
        let task = cx.background_spawn(async move {
            probes
                .into_iter()
                .filter(|probe| match &probe.loaded {
                    // Nothing comparable was loaded: stay conservative.
                    None => true,
                    Some(loaded) => changed_on_disk(&repo_path.join(&probe.path), loaded),
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |view, acx| {
            let changed = task.await;
            let _ = view.update(acx, |v, cx| {
                // A newer watcher event's probe describes the file as it is
                // now; this one may have read an intermediate state that has
                // since been undone, and neither generation nor signature
                // moves across such a round trip.
                if v.probe_req != req {
                    return;
                }
                for probe in changed {
                    // The buffer the probe started from must still be the one
                    // here: a save (new `content_sig`), or a close and reopen
                    // (new `generation`), makes the result obsolete.
                    if v.dirty
                        && v.open_path.as_deref() == Some(probe.path.as_path())
                        && v.buf_gen == probe.generation
                        && v.content_sig == probe.sig
                    {
                        v.external_changed = true;
                    }
                    if let Some(buf) = v.tab_cache.get_mut(&probe.path) {
                        if buf.dirty
                            && buf.generation == probe.generation
                            && buf.content_sig == probe.sig
                        {
                            buf.external_changed = true;
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Discard the buffer and re-read the open file from disk (the confirmed
    /// outcome of the external-change banner's Reload button — the button
    /// itself opens the dirty guard first, T-WS-EDITOR-002 §5 spec change).
    pub fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        self.dirty = false;
        self.external_changed = false;
        // Force the editor push even when the disk text hashes back to the
        // pre-edit snapshot (the user's edit is being discarded either way).
        self.pushed_sig = 0;
        self.load_selected(cx);
        cx.notify();
    }
}

/// Does `full_path` hold something other than `loaded`?
///
/// The read stops one byte past what the buffer loaded, which is both exact and
/// bounded: a longer file already differs by that byte, a shorter or different
/// one differs outright, and nothing larger than the text the editor is already
/// holding is ever allocated. `load_selected` refuses files past
/// `MAX_EDITOR_BYTES`, and this path must not become the one that slurps
/// gigabytes because an external process swapped the file for a huge one.
/// Checking `metadata().len()` first would not bound it — the file can grow
/// between that check and the read.
fn changed_on_disk(full_path: &std::path::Path, loaded: &str) -> bool {
    use std::io::Read as _;
    let Ok(file) = std::fs::File::open(full_path) else {
        return true;
    };
    let mut bytes = Vec::with_capacity(loaded.len() + 1);
    if file
        .take(loaded.len() as u64 + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return true;
    }
    bytes != loaded.as_bytes()
}
