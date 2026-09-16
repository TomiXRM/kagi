//! The external-change banner: what a working-tree watcher event means for an
//! open buffer, and the Reload that answers it.
//!
//! Split out of `lib.rs` (its LOC ceiling is a shrink-only ratchet) when #736
//! made the watcher path probe the file instead of trusting the event.

use std::path::PathBuf;

use gpui::{AppContext as _, Context};

use super::{buffer_sig, EditorWorkspaceView};

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
    /// re-read in the background and compared against the bytes that buffer
    /// loaded; only a real difference raises the banner. A buffer whose
    /// content could not be hashed at load (binary, too large, unreadable —
    /// `content_sig == 0`) keeps the old conservative banner: it cannot be
    /// proven unchanged, and an edit must never be clobbered.
    pub fn on_worktree_changed(&mut self, cx: &mut Context<Self>) {
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
        let mut probes: Vec<(PathBuf, u64)> = Vec::new();
        if self.dirty {
            if let Some(path) = self.open_path.clone() {
                probes.push((path, self.content_sig));
            }
        }
        for (path, buf) in &self.tab_cache {
            if buf.dirty {
                probes.push((path.clone(), buf.content_sig));
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
                .filter(|(path, sig)| {
                    *sig == 0
                        || std::fs::read_to_string(repo_path.join(path))
                            .map_or(true, |text| buffer_sig(path, &text) != *sig)
                })
                .map(|(path, _)| path)
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |view, acx| {
            let changed = task.await;
            let _ = view.update(acx, |v, cx| {
                for path in changed {
                    // Re-checked on landing: the user may have saved, switched
                    // tabs or closed the file while the probe was in flight.
                    if v.dirty && v.open_path.as_deref() == Some(path.as_path()) {
                        v.external_changed = true;
                    }
                    if let Some(buf) = v.tab_cache.get_mut(&path) {
                        if buf.dirty {
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
