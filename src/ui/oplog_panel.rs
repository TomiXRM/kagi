//! Operation log panel — self-contained state (ADR-0111 / Phase C).
//!
//! Extracted from `KagiApp` so the op-log ring buffer + display logic lives in
//! one testable struct. Previously `op_entries: VecDeque<OpLogEntry>` was a flat
//! field on the god-struct, pushed to from `record_op` and read from
//! `render_bottom_panel`.
//!
//! Held as an `Entity<OpLogPanel>` on `KagiApp` (ADR-0110 Phase 5 Step 5.1, the
//! same shape as `ToastStack`): the panel renders via `impl Render for
//! OpLogPanel` (in `render.rs`) and a push / row-expand re-renders only this
//! subtree, not the whole app. The per-panel UI state (the expanded row and the
//! scroll handle) lives here too. The disk-loaded startup tail is carried in
//! `KagiApp::op_log_seed` until `open_main_window` can create the entity (the
//! pure constructors have no `cx`). The data methods stay `cx`-free for tests.

use std::collections::VecDeque;
use std::path::PathBuf;

use gpui::AppContext as _;
use kagi_domain::oplog_reflog::{Attribution, ReflogLine, ReflogWindow};
use kagi_domain::plan_note::ShellKind;
use kagi_git::oplog::{OpLogEntry, OpOutcome};
use kagi_ui_core::i18n::{plan::plan_recovery_sentences, Msg};

/// Maximum entries kept in the in-memory ring buffer.
const OP_ENTRIES_MAX: usize = 200;

/// Self-contained operation log ring buffer + panel UI state.
pub struct OpLogPanel {
    entries: VecDeque<OpLogEntry>,
    /// Which row index (0 = newest) is currently expanded; `None` = none.
    expanded: Option<usize>,
    /// Scroll handle for the virtualized row list. Issue #468: a
    /// [`gpui::ListState`] (variable row height) rather than a
    /// `UniformListScrollHandle` — an expanded row is taller than a collapsed
    /// one, and `uniform_list` lays every row out at the FIRST row's height,
    /// so the overflow painted over the rows below. Same swap T-DIFF-WRAP-001
    /// made for the diff panes (`render_helpers::new_diff_list_state`).
    scroll_handle: gpui::ListState,
    /// #334: the selected entry's reflog lines, keyed by that entry — a push
    /// shifts row indices, and the detail must never land on another row.
    reflog: Option<(EntryKey, ReflogDetail)>,
    /// Bumped per request; a read that finishes under an older value is dropped.
    reflog_generation: u64,
}

/// Which entry a reflog detail belongs to. `id` alone is `0` for an attempted
/// entry whose append failed, so time and op name complete it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryKey {
    id: u64,
    timestamp: i64,
    op: String,
}

impl EntryKey {
    fn of(entry: &OpLogEntry) -> Self {
        Self {
            id: entry.id,
            timestamp: entry.timestamp,
            op: entry.op.clone(),
        }
    }
}

/// The reflog section of a selected row (#334 slice 1, ADR-0214). Read-only.
#[derive(Debug, Clone)]
pub enum ReflogDetail {
    Loading,
    Loaded {
        window: ReflogWindow,
        lines: Vec<(ReflogLine, Attribution)>,
    },
    Unavailable(String),
}

/// #334 slice 2b: a selected row asks the app to plan an op-revert or a
/// restore-to-point. The panel only names the entry; the app plans it for the
/// active repository and shows the card.
#[derive(Debug, Clone)]
pub enum OpLogPanelEvent {
    Restore(kagi_git::Operation),
}

impl gpui::EventEmitter<OpLogPanelEvent> for OpLogPanel {}

/// Whether an entry can be reverted / restored to: it carries recorded ref
/// moves (ADR-0214 §5 — never the estimate). A receipt whose append failed
/// has its record dropped in [`OpLogPanel::entry_for_recording`], so this
/// also means "persisted" (ids are 0-based, so `id` cannot tell).
pub fn restorable(entry: &OpLogEntry) -> bool {
    entry.ref_moves.is_some()
}

/// The working tree an entry ran in — the worktree badge and the reflog's
/// owner. Old lines without `worktree` fall back to `repo`. `Backend::run`
/// records the workdir with a trailing separator and other paths without, so
/// it is dropped: both spellings are one worktree (no I/O — this runs in UI
/// handlers).
pub fn entry_worktree(entry: &OpLogEntry) -> &str {
    let path = entry.worktree.as_deref().unwrap_or(&entry.repo);
    match path.trim_end_matches(['/', '\\']) {
        "" => path,
        trimmed => trimmed,
    }
}

/// Issue #468: a fresh op-log [`gpui::ListState`] (item count 0 — the render
/// syncs it to the real entry count each frame, the lifecycle documented on
/// `render_helpers::render_diff_list`). `px(1000.)` overdraw matches the diff
/// list, the other variable-height list in the app.
fn new_oplog_list_state() -> gpui::ListState {
    gpui::ListState::new(0, gpui::ListAlignment::Top, gpui::px(1000.))
}

impl OpLogPanel {
    /// Prepare the boundary receipt for display, including attempted entries
    /// whose append failed. This path performs no persistence.
    pub fn entry_for_recording(recording: &kagi_git::backend::recording::Recording) -> OpLogEntry {
        use kagi_git::backend::recording::Recording;
        let entry = recording.entry();
        let outcome = match recording {
            Recording::Appended { .. } => entry.outcome.clone(),
            Recording::Failed { error, .. } => match &entry.outcome {
                OpOutcome::Success { after }
                | OpOutcome::Partial { after, .. }
                | OpOutcome::Unknown { after, .. } => OpOutcome::Partial {
                    after: after.clone(),
                    error: format!("changed but not recorded: {error}"),
                },
                unchanged => unchanged.clone(),
            },
        };
        let mut displayed = entry.clone();
        // A receipt that never reached the log cannot be restored from (its
        // id is a placeholder that may name another entry): show its moves as
        // the estimate, like any entry without a record.
        if matches!(recording, Recording::Failed { .. }) {
            displayed.ref_moves = None;
        }
        displayed.outcome = outcome;
        displayed
    }

    pub fn new() -> Self {
        Self::from_entries(VecDeque::new())
    }

    /// Initialize from a pre-loaded tail (read from disk on tab open).
    pub fn from_entries(entries: VecDeque<OpLogEntry>) -> Self {
        Self {
            entries,
            expanded: None,
            scroll_handle: new_oplog_list_state(),
            reflog: None,
            reflog_generation: 0,
        }
    }

    /// Push a new entry to the front; drop the oldest if over the cap.
    pub fn push(&mut self, entry: OpLogEntry) {
        self.entries.push_front(entry);
        if self.entries.len() > OP_ENTRIES_MAX {
            self.entries.pop_back();
        }
    }

    /// Read-only access to the entries (for rendering).
    pub fn entries(&self) -> &VecDeque<OpLogEntry> {
        &self.entries
    }

    /// Number of entries currently held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The currently-expanded row index, if any.
    pub fn expanded(&self) -> Option<usize> {
        self.expanded
    }

    /// Toggle expansion of row `i` (collapse if already expanded).
    pub fn toggle_expanded(&mut self, i: usize) {
        self.expanded = if self.expanded == Some(i) {
            None
        } else {
            Some(i)
        };
    }

    /// Collapse any expanded row (called when new entries arrive).
    pub fn collapse(&mut self) {
        self.expanded = None;
    }

    /// Select (expand) or deselect row `i` — a click, or a failed op opening
    /// the log on its row — and read the selected entry's reflog lines in the
    /// background (#334). Reads only: no repository write, no oplog append.
    pub fn select_row(&mut self, i: usize, cx: &mut gpui::Context<Self>) {
        self.toggle_expanded(i);
        self.reflog_generation = self.reflog_generation.wrapping_add(1);
        self.reflog = None;
        if self.expanded != Some(i) {
            return;
        }
        let (Some(entry), Some(window)) = (self.entries.get(i), self.reflog_window(i)) else {
            return;
        };
        // #334 slice 2a: a recorded entry carries its moves; the time-window
        // estimate is only for entries without a record.
        if entry.ref_moves.is_some() {
            return;
        }
        let key = EntryKey::of(entry);
        let path = PathBuf::from(entry_worktree(entry));
        let generation = self.reflog_generation;
        self.reflog = Some((key.clone(), ReflogDetail::Loading));
        let read = cx.background_spawn(async move {
            kagi_git::Backend::open(&path)
                .and_then(|backend| backend.reflog_lines_between(window.after, window.until))
                .map(|lines| window.attribute(lines))
                .map_err(|error| error.to_string())
        });
        cx.spawn(async move |this, cx| {
            let result = read.await;
            let _ = this.update(cx, |panel, cx| {
                if panel.reflog_generation != generation {
                    return;
                }
                let detail = match result {
                    Ok(lines) => ReflogDetail::Loaded { window, lines },
                    Err(error) => ReflogDetail::Unavailable(error),
                };
                panel.reflog = Some((key, detail));
                cx.notify();
            });
        })
        .detach();
    }

    /// The reflog window of row `i`, bounded by the nearest older and newer
    /// entries **of the same worktree** in the loaded log.
    pub fn reflog_window(&self, i: usize) -> Option<ReflogWindow> {
        let entry = self.entries.get(i)?;
        let worktree = entry_worktree(entry);
        let same = |e: &&OpLogEntry| entry_worktree(e) == worktree;
        let older = self
            .entries
            .iter()
            .skip(i + 1)
            .find(same)
            .map(|e| e.timestamp);
        let newer = self
            .entries
            .iter()
            .take(i)
            .rev()
            .find(same)
            .map(|e| e.timestamp);
        Some(ReflogWindow::for_entry(entry.timestamp, older, newer))
    }

    /// The reflog detail of the expanded row, if it is that row's.
    pub fn reflog_detail(&self) -> Option<&ReflogDetail> {
        let entry = self.entries.get(self.expanded?)?;
        self.reflog
            .as_ref()
            .filter(|(key, _)| *key == EntryKey::of(entry))
            .map(|(_, detail)| detail)
    }

    /// A clone of the scroll handle for `gpui::list` + the scrollbar overlay.
    pub fn scroll_handle(&self) -> gpui::ListState {
        self.scroll_handle.clone()
    }

    /// Issue #468: copy row `i`'s whole entry (the truncated summary AND the
    /// detail block) to the clipboard. Same path as `branch_menu::copy_*` /
    /// `context_menu::copy_full_sha`. Called by the row's copy button; also the
    /// seam the GUI E2E scenario drives.
    pub fn copy_entry(&self, i: usize, cx: &mut gpui::App) {
        let Some(entry) = self.entries.get(i) else {
            return;
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(entry_clipboard_text(entry)));
    }
}

/// One-line outcome summary shown on the (truncated) summary row. Pure so the
/// clipboard text and the rendered row cannot drift apart.
pub fn outcome_summary(outcome: &OpOutcome) -> String {
    match outcome {
        OpOutcome::Success { after } => format!("Success \u{2192} {}", after.head),
        OpOutcome::Partial { after, error } => {
            format!("Partial \u{2192} {}: {}", after.head, error)
        }
        OpOutcome::Unknown { evidence, .. } => format!("Unknown: {}", evidence),
        OpOutcome::Failed { error } => format!("Failed: {}", error),
        OpOutcome::Refused { blockers } => format!(
            "Refused ({} blocker{})",
            blockers.len(),
            if blockers.len() == 1 { "" } else { "s" }
        ),
    }
}

/// The expanded-row detail lines (before/after state, error/blockers, recovery).
pub fn detail_lines(entry: &OpLogEntry) -> Vec<String> {
    let mut lines = base_detail_lines(entry);
    if let Some(recovery) = recovery_lines(entry) {
        lines.extend(recovery);
    }
    lines
}

/// The existing receipt facts; recovery is drawn as its own AX-labelled group.
pub(crate) fn base_detail_lines(entry: &OpLogEntry) -> Vec<String> {
    let mut lines = vec![
        format!("  before:  {}", entry.before.head),
        format!("  dirty:   {}", entry.before.dirty),
    ];
    // What an issue-create asked for, whatever came of it (#904 review).
    if let Some(fields) = &entry.issue_fields {
        if !fields.labels.is_empty() {
            lines.push(format!("  labels:  {}", fields.labels.join(", ")));
        }
        if !fields.assignees.is_empty() {
            lines.push(format!("  assignee: {}", fields.assignees.join(", ")));
        }
    }
    match &entry.outcome {
        OpOutcome::Success { after } => {
            lines.push(format!("  after:   {}", after.head));
            lines.push(format!("  dirty:   {}", after.dirty));
        }
        OpOutcome::Unknown {
            after,
            evidence: error,
        }
        | OpOutcome::Partial { after, error } => {
            lines.push(format!("  after:   {}", after.head));
            lines.push(format!("  dirty:   {}", after.dirty));
            lines.push(format!("  error:   {}", error));
        }
        OpOutcome::Failed { error } => lines.push(format!("  error:   {}", error)),
        OpOutcome::Refused { blockers } => {
            for b in blockers {
                lines.push(format!("  blocker: {}", b));
            }
        }
    }
    lines
}

/// Approved-plan guidance is shown only for outcomes that may have changed
/// state. Missing guidance is explicitly unknown, never inferred from handles.
pub fn recovery_lines(entry: &OpLogEntry) -> Option<Vec<String>> {
    if !entry.outcome.may_have_changed() {
        return None;
    }
    let mut lines = vec![format!("  {}:", Msg::OpLogRecovery.t())];
    match &entry.recovery_plan {
        Some(plan) => {
            lines.extend(
                plan_recovery_sentences(Some(plan))
                    .into_iter()
                    .map(|line| format!("  {line}")),
            );
            lines.extend(
                plan.commands_for(ShellKind::current())
                    .iter()
                    .map(|command| format!("  {} {command}", Msg::OpLogRecoveryCommand.t())),
            );
        }
        None => lines.push(format!("  {}", Msg::OpLogRecoveryNotRecorded.t())),
    }
    Some(lines)
}
/// One recorded ref move as text — the expanded row and the copied entry
/// share it (#871 review). `full` keeps whole OIDs (the copy, for
/// investigation); the row shows 8 characters. `absent` names a side where
/// the ref did not exist.
pub fn ref_move_text(m: &kagi_domain::ref_moves::RefMove, full: bool, absent: &str) -> String {
    let oid = |oid: &Option<String>| match oid {
        Some(oid) if full => oid.clone(),
        Some(oid) => oid.get(..8).unwrap_or(oid).to_string(),
        None => absent.to_string(),
    };
    let target = |symbolic: &Option<String>| {
        symbolic
            .as_deref()
            .map(|s| format!("{} ", s.trim_start_matches("refs/heads/")))
            .unwrap_or_default()
    };
    format!(
        "{}  {}{}→ {}{}",
        m.refname,
        target(&m.old_symbolic),
        oid(&m.old),
        target(&m.new_symbolic),
        oid(&m.new)
    )
}

/// Issue #468: the whole entry as one readable multi-line string — the summary
/// header (time / op / outcome) plus every detail line, i.e. exactly what the
/// expanded row shows, including the tail the summary row truncates away.
/// The recorded ref moves are part of it, with whole OIDs (#871 review).
/// Pure (no gpui, no `cx`) so it is unit-testable.
pub fn entry_clipboard_text(entry: &OpLogEntry) -> String {
    let mut out = format!(
        "{}  {}  {}\n",
        super::format_hms(entry.timestamp),
        entry.op,
        outcome_summary(&entry.outcome)
    );
    for line in detail_lines(entry) {
        out.push_str(&line);
        out.push('\n');
    }
    match entry.ref_moves.as_deref() {
        None => {}
        Some([]) => out.push_str("  ref:     none moved\n"),
        Some(moves) => {
            for m in moves {
                out.push_str(&format!("  ref:     {}\n", ref_move_text(m, true, "-")));
            }
        }
    }
    out
}

impl Default for OpLogPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_git::oplog::OpOutcome;

    fn dummy_entry(op: &str) -> OpLogEntry {
        OpLogEntry::new(
            op,
            "repo",
            kagi_git::ops::StateSummary {
                head: "HEAD → main".to_string(),
                dirty: "clean".to_string(),
            },
            OpOutcome::Success {
                after: kagi_git::ops::StateSummary {
                    head: "main".to_string(),
                    dirty: "clean".to_string(),
                },
            },
        )
    }

    #[test]
    fn recovery_section_follows_changed_outcomes_not_refusals_or_failures() {
        use kagi_domain::plan_note::{PlanRecovery, RecoveryKind};
        let state = kagi_git::ops::StateSummary {
            head: "main".into(),
            dirty: "clean".into(),
        };
        let recovery = PlanRecovery {
            kind: RecoveryKind::Discard,
            commands: vec!["git cat-file -p abc".into()],
        };
        for outcome in [
            OpOutcome::Success {
                after: state.clone(),
            },
            OpOutcome::Partial {
                after: state.clone(),
                error: "some changes succeeded".into(),
            },
            OpOutcome::Unknown {
                after: state.clone(),
                evidence: "may have changed".into(),
            },
        ] {
            let mut entry = OpLogEntry::new("op", "repo", state.clone(), outcome);
            entry.recovery_plan = Some(recovery.clone());
            let section = recovery_lines(&entry).unwrap().join("\n");
            assert!(
                section.contains("This discards your unstaged changes"),
                "{section}"
            );
            assert!(
                section.contains(&format!(
                    "{} git cat-file -p abc",
                    Msg::OpLogRecoveryCommand.t()
                )),
                "{section}"
            );
        }
        for outcome in [
            OpOutcome::Failed {
                error: "unchanged".into(),
            },
            OpOutcome::Refused {
                blockers: vec!["blocked".into()],
            },
        ] {
            let mut entry = OpLogEntry::new("op", "repo", state.clone(), outcome);
            entry.recovery_plan = Some(recovery.clone());
            assert!(recovery_lines(&entry).is_none());
            assert!(!entry_clipboard_text(&entry).contains("Recovery:"));
        }
    }

    /// #904 review: an issue-create's receipt shows the labels and assignees
    /// it asked for, in the row and in the copied entry.
    #[test]
    fn an_issue_create_shows_what_it_asked_for() {
        let entry = dummy_entry("issue-create").with_issue_fields(
            &kagi_domain::github::IssueCreateFields {
                labels: vec!["bug".into(), "docs".into()],
                assignees: vec!["hubot".into()],
            },
        );
        let lines = detail_lines(&entry);
        assert!(
            lines.contains(&"  labels:  bug, docs".to_string()),
            "{lines:?}"
        );
        assert!(
            lines.contains(&"  assignee: hubot".to_string()),
            "{lines:?}"
        );
        assert!(entry_clipboard_text(&entry).contains("  labels:  bug, docs\n"));
        assert!(!detail_lines(&dummy_entry("issue-create"))
            .iter()
            .any(|line| line.starts_with("  labels:") || line.starts_with("  assignee:")));
    }

    #[test]
    fn the_copied_entry_carries_its_recorded_ref_moves_whole() {
        let full_old = "a".repeat(40);
        let full_new = "b".repeat(40);
        let mut entry = dummy_entry("commit");
        entry.ref_moves = Some(vec![
            kagi_domain::ref_moves::RefMove {
                refname: "refs/heads/main".into(),
                old: Some(full_old.clone()),
                new: Some(full_new.clone()),
                old_symbolic: None,
                new_symbolic: None,
            },
            kagi_domain::ref_moves::RefMove {
                refname: "refs/heads/gone".into(),
                old: Some(full_old.clone()),
                new: None,
                old_symbolic: None,
                new_symbolic: None,
            },
        ]);
        let text = entry_clipboard_text(&entry);
        assert!(
            text.contains(&format!(
                "  ref:     refs/heads/main  {full_old}→ {full_new}\n"
            )),
            "{text}"
        );
        assert!(
            text.contains(&format!("  ref:     refs/heads/gone  {full_old}→ -\n")),
            "{text}"
        );

        entry.ref_moves = Some(Vec::new());
        assert!(entry_clipboard_text(&entry).contains("  ref:     none moved\n"));
        entry.ref_moves = None;
        assert!(
            !entry_clipboard_text(&entry).contains("ref:"),
            "not recorded: no line"
        );
    }

    #[test]
    fn push_and_cap() {
        let mut panel = OpLogPanel::new();
        for i in 0..(OP_ENTRIES_MAX + 5) {
            panel.push(dummy_entry(&format!("op-{i}")));
        }
        assert_eq!(panel.len(), OP_ENTRIES_MAX);
        // Oldest should have been dropped.
        assert!(!panel.entries().iter().any(|e| e.op == "op-0"));
    }

    #[test]
    fn push_front_ordering() {
        let mut panel = OpLogPanel::new();
        panel.push(dummy_entry("first"));
        panel.push(dummy_entry("second"));
        assert_eq!(panel.entries().front().unwrap().op, "second");
    }

    /// Issue #468: the clipboard text carries the whole entry — the header the
    /// summary row truncates AND every detail line, long `error:` included.
    #[test]
    fn clipboard_text_carries_the_whole_entry() {
        let long_error = "x".repeat(200);
        let entry = OpLogEntry::new(
            "checkout",
            "repo",
            kagi_git::ops::StateSummary {
                head: "HEAD → main".to_string(),
                dirty: "clean".to_string(),
            },
            OpOutcome::Failed {
                error: long_error.clone(),
            },
        );
        let text = entry_clipboard_text(&entry);
        let lines: Vec<&str> = text.lines().collect();
        // header + before + dirty + error
        assert_eq!(lines.len(), 4, "unexpected shape: {text:?}");
        assert!(lines[0].ends_with(&format!("  checkout  Failed: {long_error}")));
        assert_eq!(lines[1], "  before:  HEAD → main");
        assert_eq!(lines[2], "  dirty:   clean");
        assert_eq!(lines[3], format!("  error:   {long_error}"));
    }

    #[test]
    fn clipboard_text_lists_every_blocker() {
        let entry = OpLogEntry::new(
            "merge",
            "repo",
            kagi_git::ops::StateSummary {
                head: "main".to_string(),
                dirty: "dirty".to_string(),
            },
            OpOutcome::Refused {
                blockers: vec!["uncommitted changes".into(), "detached HEAD".into()],
            },
        );
        let text = entry_clipboard_text(&entry);
        assert!(text.contains("Refused (2 blockers)"), "{text}");
        assert!(text.contains("  blocker: uncommitted changes"), "{text}");
        assert!(text.contains("  blocker: detached HEAD"), "{text}");
    }

    #[test]
    fn from_entries_preserves_order() {
        let mut vd = VecDeque::new();
        vd.push_back(dummy_entry("a"));
        vd.push_back(dummy_entry("b"));
        let panel = OpLogPanel::from_entries(vd);
        assert_eq!(panel.len(), 2);
        assert_eq!(panel.entries().front().unwrap().op, "a");
    }

    /// #334: a row's reflog window is bounded by its own worktree's
    /// neighbours, not by whatever row happens to sit next to it.
    #[test]
    fn reflog_window_skips_other_worktrees() {
        let at = |op: &str, worktree: &str, ts: i64| {
            let mut e = dummy_entry(op).with_worktree(Some(worktree.to_string()));
            e.timestamp = ts;
            e
        };
        // Newest first, two worktrees interleaved; `Backend::run` spells
        // wt1 with a trailing slash, the GUI without.
        let panel = OpLogPanel::from_entries(VecDeque::from(vec![
            at("a", "/wt1", 30),
            at("b", "/wt2", 25),
            at("c", "/wt1/", 20),
            at("d", "/wt2", 10),
        ]));
        let newest = panel.reflog_window(0).unwrap();
        assert_eq!((newest.after, newest.until), (20, 30));
        assert!(!newest.open_start && !newest.shared_second);
        let oldest_wt1 = panel.reflog_window(2).unwrap();
        assert!(oldest_wt1.open_start, "no older wt1 entry is loaded");
        assert_eq!(oldest_wt1.until, 20);
        let b = panel.reflog_window(1).unwrap();
        assert_eq!((b.after, b.until), (10, 25));
    }

    /// #334 slice 2b: a receipt that never reached the log cannot be restored
    /// from — its id is a placeholder (ids are 0-based, so 0 may name the
    /// first real entry). A persisted one with id 0 is restorable.
    #[test]
    fn only_a_persisted_record_is_restorable() {
        use kagi_git::backend::recording::Recording;
        let mut entry = dummy_entry("create-branch");
        entry.ref_moves = Some(Vec::new());
        let appended = Recording::Appended {
            path: "ops.jsonl".into(),
            entry: entry.clone(),
        };
        let failed = Recording::Failed {
            attempted: entry,
            error: "disk full".into(),
        };
        let shown = OpLogPanel::entry_for_recording(&appended);
        assert_eq!(shown.id, 0);
        assert!(restorable(&shown), "the first real entry has id 0");
        assert!(!restorable(&OpLogPanel::entry_for_recording(&failed)));
    }
}
