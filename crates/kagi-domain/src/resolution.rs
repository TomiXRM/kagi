//! Conflict resolution domain models — pure data, no git2.
//!
//! The git2-backed `ResolutionBuffer` lives in the git-backend layer
//! (`kagi::git::resolution`) and re-exports these models.

#[path = "resolution/assembly.rs"]
mod assembly;

/// Which side(s) a per-file choice adopts (ADR-0057 / ADR-0058 §hunk buttons).
///
/// `Both*` variants make the ordering explicit (Combination current-first /
/// incoming-first), addressing the survey's "order ambiguity" finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionChoice {
    /// Keep the current branch's version only ("Keep current").
    Current,
    /// Take the incoming version only ("Take incoming").
    Incoming,
    /// Keep both, current side first ("Keep both (current first)").
    BothCurrentFirst,
    /// Keep both, incoming side first ("Keep both (incoming first)").
    BothIncomingFirst,
}

/// Which side of a directory/file conflict to keep (#320 / ADR-0164).
///
/// A directory/file conflict is a path that one side committed as a **file** and
/// the other as a **directory** (files under `path/`). The two cannot coexist in
/// one tree, so — unlike a content conflict — there is no merge, only a single
/// binary choice. This is the whole "FSM": two terminal states, no intermediate
/// hunk model. Pure data; the git layer maps it onto index entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirFileChoice {
    /// Keep the directory side: drop the file blob, keep the `path/` entries.
    KeepDirectory,
    /// Keep the file side: drop the `path/` entries, stage the file blob.
    KeepFile,
}

impl DirFileChoice {
    /// Stable identifier for oplog / tests / logging.
    pub fn slug(&self) -> &'static str {
        match self {
            DirFileChoice::KeepDirectory => "keep-directory",
            DirFileChoice::KeepFile => "keep-file",
        }
    }
}

/// Where a single Result line came from (per-line provenance, ADR-0057).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOrigin {
    /// From the current branch side (index stage 2).
    Current,
    /// From the incoming side (index stage 3).
    Incoming,
    /// Hand-typed by the user (manual edit) or otherwise synthesized.
    Manual,
    /// A non-conflict passthrough line shared by both sides (context).
    Context,
}

/// Output ordering for line-level selections: either by position with a
/// same-position tie-breaker, or grouped by side to preserve a hunk-level Both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOrder {
    /// At a shared position, emit the Current line before the Incoming line.
    CurrentFirst,
    /// At a shared position, emit the Incoming line before the Current line.
    IncomingFirst,
    /// Emit all taken Current lines, then all taken Incoming lines.
    CurrentBlockFirst,
    /// Emit all taken Incoming lines, then all taken Current lines.
    IncomingBlockFirst,
}

impl LineOrder {
    fn from_choice(choice: &HunkChoice) -> LineOrder {
        match choice {
            HunkChoice::BothCurrentFirst => LineOrder::CurrentBlockFirst,
            HunkChoice::BothIncomingFirst => LineOrder::IncomingBlockFirst,
            HunkChoice::AcceptCurrent
            | HunkChoice::AcceptIncoming
            | HunkChoice::Manual(_)
            | HunkChoice::Unresolved => LineOrder::CurrentFirst,
        }
    }
}

/// Which pane/side a line-level checkbox belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionSide {
    /// Current branch (A) side.
    Current,
    /// Incoming (B) side.
    Incoming,
}

/// Tri-state checkbox value for file/chunk/line selection aggregation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriState {
    /// Every child line/chunk on this side is selected.
    All,
    /// Some but not every child is selected.
    Partial,
    /// No child line/chunk on this side is selected.
    None,
}

impl TriState {
    /// Aggregate a slice of child selections.
    pub fn from_bools(values: &[bool]) -> TriState {
        let selected = values.iter().filter(|v| **v).count();
        if selected == 0 {
            TriState::None
        } else if selected == values.len() {
            TriState::All
        } else {
            TriState::Partial
        }
    }

    /// Aggregate child tri-states into a parent tri-state.
    pub fn from_children(values: &[TriState]) -> TriState {
        if values.is_empty() || values.iter().all(|v| *v == TriState::None) {
            TriState::None
        } else if values.iter().all(|v| *v == TriState::All) {
            TriState::All
        } else {
            TriState::Partial
        }
    }
}

/// Per-line selection state for a conflict hunk (ADR-0071).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineSelection {
    // Private masks keep their constructor-validated dimensions immutable.
    current_taken: Vec<bool>,
    incoming_taken: Vec<bool>,
    // Empty sides have no bits, but adopting a deletion is still a choice.
    empty_taken: [bool; 2],
    /// Output order when both sides have selected lines.
    pub order: LineOrder,
}

/// A restored selection mask does not match its materialized source sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineSelectionShapeError;

impl LineSelection {
    /// Restore masks only when both dimensions match the source hunk.
    pub fn new(
        current_len: usize,
        incoming_len: usize,
        current_taken: Vec<bool>,
        incoming_taken: Vec<bool>,
        order: LineOrder,
        empty_taken: [bool; 2],
    ) -> Result<Self, LineSelectionShapeError> {
        if current_taken.len() != current_len || incoming_taken.len() != incoming_len {
            return Err(LineSelectionShapeError);
        }
        debug_assert_eq!(
            (current_taken.len(), incoming_taken.len()),
            (current_len, incoming_len)
        );
        Ok(Self {
            current_taken,
            incoming_taken,
            order,
            empty_taken,
        })
    }

    /// Borrow immutable selection bits for one source side.
    pub fn taken(&self, side: SelectionSide) -> &[bool] {
        match side {
            SelectionSide::Current => &self.current_taken,
            SelectionSide::Incoming => &self.incoming_taken,
        }
    }

    /// Explicit adoption of zero-line sides, retained by the draft codec.
    pub fn empty_taken(&self) -> [bool; 2] {
        self.empty_taken
    }

    /// Empty selection for a hunk with `current_len` / `incoming_len` side lines.
    pub fn empty(current_len: usize, incoming_len: usize, order: LineOrder) -> LineSelection {
        LineSelection {
            current_taken: vec![false; current_len],
            incoming_taken: vec![false; incoming_len],
            order,
            empty_taken: [false; 2],
        }
    }

    /// Build a line selection that represents an existing hunk-level choice.
    pub fn from_choice(
        current_len: usize,
        incoming_len: usize,
        choice: &HunkChoice,
    ) -> LineSelection {
        let mut selection =
            LineSelection::empty(current_len, incoming_len, LineOrder::from_choice(choice));
        for side in [SelectionSide::Current, SelectionSide::Incoming] {
            selection.set_side(side, choice.takes_side(side));
        }
        selection
    }

    /// Tri-state for one side of this hunk.
    pub fn side_state(&self, side: SelectionSide) -> TriState {
        let index = match side {
            SelectionSide::Current => 0,
            SelectionSide::Incoming => 1,
        };
        let taken = self.taken(side);
        if taken.is_empty() && self.empty_taken[index] {
            TriState::All
        } else {
            TriState::from_bools(taken)
        }
    }

    /// Set every line on one side, implementing parent→child propagation.
    pub fn set_side(&mut self, side: SelectionSide, taken: bool) {
        match side {
            SelectionSide::Current => {
                self.current_taken.fill(taken);
                self.empty_taken[0] = self.current_taken.is_empty() && taken;
            }
            SelectionSide::Incoming => {
                self.incoming_taken.fill(taken);
                self.empty_taken[1] = self.incoming_taken.is_empty() && taken;
            }
        }
    }

    /// Set one line on one side. Returns `false` when the index is out of range.
    pub fn set_line(&mut self, side: SelectionSide, line_index: usize, taken: bool) -> bool {
        let lines = match side {
            SelectionSide::Current => &mut self.current_taken,
            SelectionSide::Incoming => &mut self.incoming_taken,
        };
        let Some(slot) = lines.get_mut(line_index) else {
            return false;
        };
        *slot = taken;
        true
    }
}

impl LineOrigin {
    /// Stable single-char tag for the autosave JSON / tests.
    pub fn tag(self) -> char {
        match self {
            LineOrigin::Current => 'c',
            LineOrigin::Incoming => 'i',
            LineOrigin::Manual => 'm',
            LineOrigin::Context => 'x',
        }
    }

    pub fn from_tag(c: char) -> LineOrigin {
        match c {
            'c' => LineOrigin::Current,
            'i' => LineOrigin::Incoming,
            'x' => LineOrigin::Context,
            _ => LineOrigin::Manual,
        }
    }
}

/// One Result line: its text (without the trailing newline) and provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLine {
    /// The line text (no trailing `\n`).
    pub text: String,
    /// Where the line came from.
    pub origin: LineOrigin,
}

/// Marker detection for resolution text without source authority. Unlike the
/// independent staged-content checklist, this accepts custom-width delimiters:
/// a complete run of at least seven `<`, `=` or `>`, followed by space or EOL.
pub fn resolution_text_has_conflict_marker(text: &str) -> bool {
    text.split('\n').any(|line| {
        let bytes = line.as_bytes();
        let Some(&marker) = bytes.first() else {
            return false;
        };
        if !matches!(marker, b'<' | b'=' | b'>') {
            return false;
        }
        let width = bytes.iter().take_while(|&&byte| byte == marker).count();
        width >= 7 && (width == bytes.len() || bytes.get(width) == Some(&b' '))
    })
}

/// Whether a Result contains residual markers, excluding literal source/context
/// text. Manual text has no source authority and uses the plain-text detector.
pub fn resolved_lines_have_conflict_marker(lines: &[ResolvedLine]) -> bool {
    lines.iter().any(|line| {
        line.origin == LineOrigin::Manual && resolution_text_has_conflict_marker(&line.text)
    })
}

// ────────────────────────────────────────────────────────────
// Hunk-level model (W32-CONFLICT-EDITOR, T-CONFLICT-020..025)
// ────────────────────────────────────────────────────────────

/// A per-hunk choice in the hunk-level Conflict Editor (ADR-0064 buttons).
///
/// This is the hunk analogue of [`ResolutionChoice`] with two extra states the
/// file-level API does not need: a free-form [`HunkChoice::Manual`] edit of a
/// single hunk and an explicit [`HunkChoice::Unresolved`] reset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HunkChoice {
    /// Keep the current branch side only ("Accept current").
    AcceptCurrent,
    /// Take the incoming side only ("Accept incoming").
    AcceptIncoming,
    /// Keep both, current side first ("Accept both: current then incoming").
    BothCurrentFirst,
    /// Keep both, incoming side first ("Accept both: incoming then current").
    BothIncomingFirst,
    /// Hand-edited replacement text for this hunk ("Edit result").
    Manual(String),
    /// Not yet decided — the hunk is still unresolved ("Reset this hunk").
    Unresolved,
}

impl HunkChoice {
    /// Whether this whole-hunk choice adopts a side, including an empty one.
    pub fn takes_side(&self, side: SelectionSide) -> bool {
        matches!(
            (self, side),
            (Self::AcceptCurrent, SelectionSide::Current)
                | (Self::AcceptIncoming, SelectionSide::Incoming)
                | (Self::BothCurrentFirst, _)
                | (Self::BothIncomingFirst, _)
        )
    }
}

/// One ordered region of a conflicted file's zdiff3 materialization.
///
/// A file splits into an ordered list of regions: [`Region::Passthrough`] for
/// the non-conflict context lines git produced verbatim, and [`Region::Hunk`]
/// for each conflict block with its Current / Incoming / Base line groups and a
/// per-hunk [`HunkChoice`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Region {
    /// Non-conflict context lines (shared by both sides), kept verbatim.
    Passthrough(Vec<String>),
    /// One conflict block.
    Hunk(ConflictHunk),
}

/// One conflict hunk: the three side line groups plus the user's choice.
///
/// Line groups hold the text **without** trailing newlines; the file assembler
/// re-joins them on `\n`.  `base` is the zdiff3 common-ancestor context (may be
/// empty when git emitted standard markers instead of zdiff3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictHunk {
    /// Current branch (A) side lines.
    pub current: Vec<String>,
    /// Incoming (B) side lines.
    pub incoming: Vec<String>,
    /// Base / common-ancestor lines (zdiff3); empty under standard markers.
    pub base: Vec<String>,
    /// The per-hunk resolution choice.
    pub choice: HunkChoice,
    /// Per-line selection state. `None` means assemble from `choice` for
    /// backwards-compatible hunk-level behavior.
    pub line_select: Option<LineSelection>,
    /// Delimiter width of the materialization, retained for unresolved assembly.
    pub marker_width: usize,
}

impl ConflictHunk {
    /// Whether this hunk has been resolved (anything but [`HunkChoice::Unresolved`]).
    pub fn is_resolved(&self) -> bool {
        if self.line_select.is_some() {
            return true;
        }
        !matches!(self.choice, HunkChoice::Unresolved)
    }

    /// Tri-state for one side of this hunk, deriving from hunk choice when no
    /// line-level state exists.
    pub fn side_state(&self, side: SelectionSide) -> TriState {
        if let Some(selection) = &self.line_select {
            return selection.side_state(side);
        }
        if self.choice.takes_side(side) {
            TriState::All
        } else {
            TriState::None
        }
    }

    /// Ensure this hunk has line-level state, seeding it from the current
    /// hunk-level choice so a line edit preserves the existing chunk selection.
    pub fn ensure_line_selection(&mut self) -> &mut LineSelection {
        let current_len = self.current.len();
        let incoming_len = self.incoming.len();
        let choice = self.choice.clone();
        self.line_select
            .get_or_insert_with(|| LineSelection::from_choice(current_len, incoming_len, &choice))
    }

    /// Drop back to the undecided state when a *deselect* left nothing taken on
    /// either side. Clearing the last taken line is not "take neither" — the
    /// UI has no other way to un-choose, so an empty selection is the state the
    /// user started from (user report: clicking Incoming twice made the hunk
    /// take neither side). Only ever called on a deselect, so taking a side
    /// that happens to have zero lines (a deletion) still counts as resolved.
    pub fn undecide_if_empty(&mut self) {
        let empty = self.line_select.as_ref().is_some_and(|selection| {
            selection.side_state(SelectionSide::Current) == TriState::None
                && selection.side_state(SelectionSide::Incoming) == TriState::None
        });
        if empty {
            self.line_select = None;
            self.choice = HunkChoice::Unresolved;
        }
    }

    /// The resolved lines this hunk contributes to the file Result, tagged with
    /// provenance.  An [`HunkChoice::Unresolved`] hunk re-emits the conflict
    /// markers (so an unresolved Result is recognizably unfinished and the
    /// marker gate still trips), tagged [`LineOrigin::Manual`].
    pub fn resolved_lines(&self) -> Vec<ResolvedLine> {
        let mut lines = Vec::new();
        self.visit_resolved_lines(&mut |text, origin| {
            lines.push(ResolvedLine {
                text: text.to_owned(),
                origin,
            });
            true
        });
        lines
    }
}

/// The hunk decomposition of one conflicted file: an ordered region list.
///
/// Built from the zdiff3 (or standard-marker fallback) materialization via
/// [`HunkModel::from_marker_text`].  The file Result is assembled from all
/// regions in order ([`HunkModel::assemble`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkModel {
    /// The ordered regions (passthrough + conflict hunks).
    pub regions: Vec<Region>,
}

impl HunkModel {
    /// Parse standard seven-character conflict markers.
    pub fn from_marker_text(text: &str) -> HunkModel {
        Self::from_marker_text_with_width(text, 7)
    }

    /// Parse materialized conflicts using exactly the supplied delimiter width.
    ///
    /// Openers are recognized only outside a hunk, base markers only in Current,
    /// separators only in Current/Base, and closers only in Incoming. Other
    /// marker-looking lines remain source text. Labeled markers require a space
    /// before their label; separators must occupy the whole line.
    ///
    /// A truncated hunk is retained as unresolved with all collected side text.
    /// A zero width recognizes no delimiters.
    pub fn from_marker_text_with_width(text: &str, marker_width: usize) -> HunkModel {
        let mut regions: Vec<Region> = Vec::new();
        let mut passthrough: Vec<String> = Vec::new();

        // Parser state while inside a hunk.
        #[derive(PartialEq)]
        enum Phase {
            None,
            Current,
            Base,
            Incoming,
        }
        let mut phase = Phase::None;
        let mut cur: Vec<String> = Vec::new();
        let mut base: Vec<String> = Vec::new();
        let mut inc: Vec<String> = Vec::new();

        for line in text.split_terminator('\n') {
            if phase == Phase::None && is_marker(line, b'<', marker_width) {
                // Flush any accumulated passthrough before opening a hunk.
                if !passthrough.is_empty() {
                    regions.push(Region::Passthrough(std::mem::take(&mut passthrough)));
                }
                phase = Phase::Current;
            } else if phase == Phase::Current && is_marker(line, b'|', marker_width) {
                phase = Phase::Base;
            } else if (phase == Phase::Current || phase == Phase::Base)
                && is_marker(line, b'=', marker_width)
            {
                phase = Phase::Incoming;
            } else if phase == Phase::Incoming && is_marker(line, b'>', marker_width) {
                regions.push(Region::Hunk(ConflictHunk {
                    current: std::mem::take(&mut cur),
                    incoming: std::mem::take(&mut inc),
                    base: std::mem::take(&mut base),
                    choice: HunkChoice::Unresolved,
                    line_select: None,
                    marker_width,
                }));
                phase = Phase::None;
            } else {
                match phase {
                    Phase::None => passthrough.push(line.to_string()),
                    Phase::Current => cur.push(line.to_string()),
                    Phase::Base => base.push(line.to_string()),
                    Phase::Incoming => inc.push(line.to_string()),
                }
            }
        }

        // A malformed / truncated hunk (no closing marker) is recovered as a
        // best-effort unresolved hunk so no content is lost.
        if phase != Phase::None {
            regions.push(Region::Hunk(ConflictHunk {
                current: cur,
                incoming: inc,
                base,
                choice: HunkChoice::Unresolved,
                line_select: None,
                marker_width,
            }));
        } else if !passthrough.is_empty() {
            regions.push(Region::Passthrough(passthrough));
        }

        HunkModel { regions }
    }

    /// The conflict hunks in order (skipping passthrough regions).
    pub fn hunks(&self) -> Vec<&ConflictHunk> {
        self.regions
            .iter()
            .filter_map(|r| match r {
                Region::Hunk(h) => Some(h),
                Region::Passthrough(_) => None,
            })
            .collect()
    }

    /// Number of conflict hunks.
    pub fn hunk_count(&self) -> usize {
        self.regions
            .iter()
            .filter(|r| matches!(r, Region::Hunk(_)))
            .count()
    }

    /// Number of resolved conflict hunks.
    pub fn resolved_hunk_count(&self) -> usize {
        self.hunks().iter().filter(|h| h.is_resolved()).count()
    }

    /// Whether every conflict hunk is resolved.
    pub fn all_resolved(&self) -> bool {
        self.hunks().iter().all(|h| h.is_resolved())
    }

    /// Set the choice of the `n`-th conflict hunk (0-based among hunks).  No-op
    /// if the index is out of range.  Returns `true` on success.
    pub fn set_choice(&mut self, hunk_index: usize, choice: HunkChoice) -> bool {
        let mut i = 0usize;
        for r in &mut self.regions {
            if let Region::Hunk(h) = r {
                if i == hunk_index {
                    h.choice = choice;
                    h.line_select = None;
                    return true;
                }
                i += 1;
            }
        }
        false
    }

    fn with_hunk_mut(&mut self, hunk_index: usize, mut f: impl FnMut(&mut ConflictHunk)) -> bool {
        let mut i = 0usize;
        for r in &mut self.regions {
            if let Region::Hunk(h) = r {
                if i == hunk_index {
                    f(h);
                    return true;
                }
                i += 1;
            }
        }
        false
    }

    /// Tri-state for the `n`-th hunk side.
    pub fn hunk_side_state(&self, hunk_index: usize, side: SelectionSide) -> Option<TriState> {
        self.hunks().get(hunk_index).map(|h| h.side_state(side))
    }

    /// Tri-state for a whole file side, aggregating all conflict chunks.
    pub fn file_side_state(&self, side: SelectionSide) -> TriState {
        let states: Vec<TriState> = self.hunks().iter().map(|h| h.side_state(side)).collect();
        TriState::from_children(&states)
    }

    /// Set all lines on a whole file side.
    pub fn set_file_side(&mut self, side: SelectionSide, taken: bool) {
        for r in &mut self.regions {
            if let Region::Hunk(h) = r {
                h.ensure_line_selection().set_side(side, taken);
                if !taken {
                    h.undecide_if_empty();
                }
            }
        }
    }

    /// Set all lines on one hunk side.
    pub fn set_hunk_side(&mut self, hunk_index: usize, side: SelectionSide, taken: bool) -> bool {
        self.with_hunk_mut(hunk_index, |h| {
            h.ensure_line_selection().set_side(side, taken);
            if !taken {
                h.undecide_if_empty();
            }
        })
    }

    /// Set one line checkbox on one hunk side.
    pub fn set_hunk_line(
        &mut self,
        hunk_index: usize,
        side: SelectionSide,
        line_index: usize,
        taken: bool,
    ) -> bool {
        let mut changed = false;
        self.with_hunk_mut(hunk_index, |h| {
            let len = match side {
                SelectionSide::Current => h.current.len(),
                SelectionSide::Incoming => h.incoming.len(),
            };
            if line_index >= len {
                return;
            }
            changed = h.ensure_line_selection().set_line(side, line_index, taken);
            if changed && !taken {
                h.undecide_if_empty();
            }
        }) && changed
    }

    /// Reorder already adopted lines without making a resolution choice.
    /// Returns `false` for unchanged or inapplicable orders (including unresolved,
    /// manual, empty and one-sided choices), so callers need not record an edit.
    /// Positional orders require a same-position tie; block orders apply whenever
    /// both sides have taken lines, even without a same-position tie.
    pub fn set_hunk_line_order(&mut self, hunk_index: usize, order: LineOrder) -> bool {
        let mut changed = false;
        self.with_hunk_mut(hunk_index, |h| {
            let current_first = match order {
                LineOrder::CurrentFirst | LineOrder::CurrentBlockFirst => true,
                LineOrder::IncomingFirst | LineOrder::IncomingBlockFirst => false,
            };
            if let Some(selection) = &mut h.line_select {
                let applicable = match order {
                    LineOrder::CurrentFirst | LineOrder::IncomingFirst => selection
                        .current_taken
                        .iter()
                        .zip(&selection.incoming_taken)
                        .any(|(current, incoming)| *current && *incoming),
                    LineOrder::CurrentBlockFirst | LineOrder::IncomingBlockFirst => {
                        selection.current_taken.contains(&true)
                            && selection.incoming_taken.contains(&true)
                    }
                };
                if !applicable || selection.order == order {
                    return;
                }
                selection.order = order;
                changed = true;
            } else if h.current.is_empty() || h.incoming.is_empty() {
                return;
            }
            match h.choice {
                HunkChoice::BothCurrentFirst | HunkChoice::BothIncomingFirst => {
                    let choice = if current_first {
                        HunkChoice::BothCurrentFirst
                    } else {
                        HunkChoice::BothIncomingFirst
                    };
                    changed |= h.choice != choice;
                    h.choice = choice;
                }
                HunkChoice::AcceptCurrent
                | HunkChoice::AcceptIncoming
                | HunkChoice::Manual(_)
                | HunkChoice::Unresolved => {}
            }
        }) && changed
    }

    /// Assemble the file Result lines from all regions, in order, tracking
    /// per-line provenance.  Passthrough lines are [`LineOrigin::Context`].
    pub fn assemble(&self) -> Vec<ResolvedLine> {
        let mut lines = Vec::new();
        self.visit_resolved_lines(&mut |text, origin| {
            lines.push(ResolvedLine {
                text: text.to_owned(),
                origin,
            });
            true
        });
        lines
    }

    /// The assembled Result text (lines joined with `\n`, single trailing
    /// newline), like [`ResolutionBuffer::resolved_text`].
    pub fn assembled_text(&self) -> String {
        lines_to_text(&self.assemble())
    }
}

/// Match the complete marker run, not a prefix of a wider source literal.
fn is_marker(line: &str, marker: u8, width: usize) -> bool {
    width != 0
        && line.as_bytes().get(..width).is_some_and(|run| {
            run.iter().all(|byte| *byte == marker)
                && (line.len() == width
                    || (marker != b'=' && line.as_bytes().get(width) == Some(&b' ')))
        })
}

/// Split `text` into lines (on `\n`), each tagged with `origin`.  A trailing
/// newline does not produce a spurious empty final line.
pub fn text_to_lines(text: &str, origin: LineOrigin) -> Vec<ResolvedLine> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = text.split('\n').collect();
    // A text ending in '\n' yields a trailing "" — drop it so round-trips are
    // stable (lines_to_text re-appends a single trailing newline).
    if let Some(last) = lines.last() {
        if last.is_empty() {
            lines.pop();
        }
    }
    lines
        .into_iter()
        .map(|l| ResolvedLine {
            text: l.to_string(),
            origin,
        })
        .collect()
}

/// Join resolution lines back into text with `\n` separators and a single
/// trailing newline (git-style line-oriented files).
pub fn lines_to_text(lines: &[ResolvedLine]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for line in lines {
        out.push_str(&line.text);
        out.push('\n');
    }
    out
}

// ────────────────────────────────────────────────────────────
// Unit tests (pure hunk model / line conversion)
// ────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "resolution/order_tests.rs"]
mod order_tests;

#[cfg(test)]
#[path = "resolution/hardening_tests.rs"]
mod hardening_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-hunk zdiff3 materialization with passthrough context around and
    /// between the conflicts.
    const TWO_HUNK_ZDIFF3: &str = "\
keep top
<<<<<<< Current
a-current
||||||| Base
a-base
=======
a-incoming
>>>>>>> Incoming
keep middle
<<<<<<< Current
b-current
=======
b-incoming
>>>>>>> Incoming
keep bottom
";

    /// Clicking a side on and off again must return the hunk to UNDECIDED, not
    /// leave it "resolved" with neither side taken (user report).
    #[test]
    fn deselecting_the_last_side_returns_the_hunk_to_undecided() {
        let mut model = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);

        assert!(model.set_hunk_side(0, SelectionSide::Incoming, true));
        assert_eq!(model.resolved_hunk_count(), 1);
        assert_eq!(
            model
                .assemble()
                .iter()
                .filter(|l| l.text == "a-incoming")
                .count(),
            1
        );

        assert!(model.set_hunk_side(0, SelectionSide::Incoming, false));
        assert_eq!(
            model.resolved_hunk_count(),
            0,
            "un-choosing the only taken side must not count as resolved",
        );
        // An undecided hunk re-emits its conflict markers.
        assert!(model.assembled_text().contains("<<<<<<<"));
    }

    /// The same for the per-line control: clearing the last taken line undecides
    /// the hunk instead of resolving it to nothing.
    #[test]
    fn clearing_the_last_taken_line_returns_the_hunk_to_undecided() {
        let mut model = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        assert!(model.set_hunk_line(1, SelectionSide::Current, 0, true));
        assert_eq!(model.resolved_hunk_count(), 1);
        assert!(model.set_hunk_line(1, SelectionSide::Current, 0, false));
        assert_eq!(model.resolved_hunk_count(), 0);
    }

    #[test]
    fn text_to_lines_trailing_newline_stable() {
        let lines = text_to_lines("a\nb\n", LineOrigin::Manual);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines_to_text(&lines), "a\nb\n");

        let no_trailing = text_to_lines("a\nb", LineOrigin::Manual);
        assert_eq!(no_trailing.len(), 2);
        assert_eq!(lines_to_text(&no_trailing), "a\nb\n");
    }

    #[test]
    fn hunk_split_multiple_regions_in_one_file() {
        let m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        assert_eq!(m.regions.len(), 5);
        assert_eq!(m.hunk_count(), 2);
        assert!(matches!(m.regions[0], Region::Passthrough(_)));
        assert!(matches!(m.regions[1], Region::Hunk(_)));
        assert!(matches!(m.regions[2], Region::Passthrough(_)));
        assert!(matches!(m.regions[3], Region::Hunk(_)));
        assert!(matches!(m.regions[4], Region::Passthrough(_)));

        let hunks = m.hunks();
        assert_eq!(hunks[0].current, vec!["a-current".to_string()]);
        assert_eq!(hunks[0].base, vec!["a-base".to_string()]);
        assert_eq!(hunks[0].incoming, vec!["a-incoming".to_string()]);
        assert!(hunks[1].base.is_empty());
        assert_eq!(hunks[1].current, vec!["b-current".to_string()]);
    }

    #[test]
    fn hunk_accept_variants_assemble_with_provenance() {
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        assert!(m.set_choice(0, HunkChoice::AcceptCurrent));
        assert!(m.set_choice(1, HunkChoice::BothIncomingFirst));
        assert!(m.all_resolved());

        let lines = m.assemble();
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "keep top",
                "a-current",
                "keep middle",
                "b-incoming",
                "b-current",
                "keep bottom",
            ]
        );
        let origins: Vec<LineOrigin> = lines.iter().map(|l| l.origin).collect();
        assert_eq!(
            origins,
            vec![
                LineOrigin::Context,
                LineOrigin::Current,
                LineOrigin::Context,
                LineOrigin::Incoming,
                LineOrigin::Current,
                LineOrigin::Context,
            ]
        );
    }

    #[test]
    fn hunk_accept_incoming_and_both_current_first() {
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        m.set_choice(0, HunkChoice::AcceptIncoming);
        m.set_choice(1, HunkChoice::BothCurrentFirst);
        let texts: Vec<String> = m.assemble().into_iter().map(|l| l.text).collect();
        assert_eq!(
            texts,
            vec![
                "keep top".to_string(),
                "a-incoming".to_string(),
                "keep middle".to_string(),
                "b-current".to_string(),
                "b-incoming".to_string(),
                "keep bottom".to_string(),
            ]
        );
    }

    #[test]
    fn line_selection_includes_and_excludes_individual_lines() {
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        assert!(m.set_hunk_line(0, SelectionSide::Current, 0, true));
        assert!(m.set_hunk_line(0, SelectionSide::Incoming, 0, false));
        assert!(m.set_hunk_side(1, SelectionSide::Incoming, true));

        let texts: Vec<String> = m.assemble().into_iter().map(|l| l.text).collect();
        assert_eq!(
            texts,
            vec![
                "keep top".to_string(),
                "a-current".to_string(),
                "keep middle".to_string(),
                "b-incoming".to_string(),
                "keep bottom".to_string(),
            ]
        );
        assert!(m.all_resolved());
    }

    #[test]
    fn line_selection_seeds_from_existing_hunk_choice() {
        // Every fill arm of `LineSelection::from_choice`.
        let both = |c| LineSelection::from_choice(2, 2, &c);
        assert_eq!(
            both(HunkChoice::AcceptCurrent),
            LineSelection {
                current_taken: vec![true, true],
                incoming_taken: vec![false, false],
                empty_taken: [false; 2],
                order: LineOrder::CurrentFirst,
            }
        );
        assert_eq!(
            both(HunkChoice::AcceptIncoming),
            LineSelection {
                current_taken: vec![false, false],
                incoming_taken: vec![true, true],
                empty_taken: [false; 2],
                order: LineOrder::CurrentFirst,
            }
        );
        assert_eq!(
            both(HunkChoice::BothCurrentFirst),
            LineSelection {
                current_taken: vec![true, true],
                incoming_taken: vec![true, true],
                empty_taken: [false; 2],
                order: LineOrder::CurrentBlockFirst,
            }
        );
        assert_eq!(
            both(HunkChoice::BothIncomingFirst),
            LineSelection {
                current_taken: vec![true, true],
                incoming_taken: vec![true, true],
                empty_taken: [false; 2],
                order: LineOrder::IncomingBlockFirst,
            }
        );
        for empty in [HunkChoice::Unresolved, HunkChoice::Manual(String::new())] {
            assert_eq!(
                both(empty),
                LineSelection::empty(2, 2, LineOrder::CurrentFirst)
            );
        }
    }

    #[test]
    fn line_edit_preserves_the_previous_hunk_choice() {
        // Hunk-level AcceptCurrent, then tick one Incoming line: the Current
        // line stays in, because ensure_line_selection seeds from the choice.
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        m.set_choice(0, HunkChoice::AcceptCurrent);
        m.set_choice(1, HunkChoice::AcceptCurrent);
        assert!(m.set_hunk_line(0, SelectionSide::Incoming, 0, true));

        let texts: Vec<String> = m.assemble().into_iter().map(|l| l.text).collect();
        assert_eq!(
            texts,
            vec![
                "keep top".to_string(),
                "a-current".to_string(),
                "a-incoming".to_string(),
                "keep middle".to_string(),
                "b-current".to_string(),
                "keep bottom".to_string(),
            ]
        );
    }

    #[test]
    fn line_selection_order_is_explicit() {
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        assert!(m.set_hunk_side(0, SelectionSide::Current, true));
        assert!(m.set_hunk_side(0, SelectionSide::Incoming, true));
        assert!(m.set_hunk_line_order(0, LineOrder::IncomingFirst));

        let texts: Vec<String> = m.assemble().into_iter().map(|l| l.text).collect();
        assert_eq!(texts[1], "a-incoming");
        assert_eq!(texts[2], "a-current");
    }

    #[test]
    fn line_selection_interleaves_by_position() {
        // Regression (user report): in a multi-line hunk, check all of Incoming,
        // uncheck one Incoming line, and check the Current line at the same
        // position — the Current line must land AT that position, not grouped at
        // the top/bottom. (Previously lines were grouped by side.)
        let markers = "<<<<<<< Current\nc1\nc2\nc3\n=======\ni1\ni2\ni3\n>>>>>>> Incoming\n";
        let mut m = HunkModel::from_marker_text(markers);
        assert!(m.set_hunk_side(0, SelectionSide::Incoming, true)); // all incoming
        assert!(m.set_hunk_line(0, SelectionSide::Incoming, 1, false)); // drop i2
        assert!(m.set_hunk_line(0, SelectionSide::Current, 1, true)); // take c2 instead

        let texts: Vec<String> = m.assemble().into_iter().map(|l| l.text).collect();
        assert_eq!(
            texts,
            vec!["i1".to_string(), "c2".to_string(), "i3".to_string()],
            "the swapped-in current line must keep its position, not move to an end"
        );
    }

    #[test]
    fn assemble_falls_back_to_hunk_choice_without_line_selection() {
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        assert!(m.set_choice(0, HunkChoice::AcceptIncoming));
        let hunk = m.hunks()[0];
        assert!(hunk.line_select.is_none());
        let texts: Vec<String> = m.assemble().into_iter().map(|l| l.text).collect();
        assert_eq!(texts[1], "a-incoming");
    }

    #[test]
    fn tri_state_helpers_and_parent_child_propagation() {
        assert_eq!(TriState::from_bools(&[false, false]), TriState::None);
        assert_eq!(TriState::from_bools(&[true, true]), TriState::All);
        assert_eq!(TriState::from_bools(&[true, false]), TriState::Partial);
        assert_eq!(
            TriState::from_children(&[TriState::All, TriState::None]),
            TriState::Partial
        );

        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        assert_eq!(m.file_side_state(SelectionSide::Current), TriState::None);
        m.set_file_side(SelectionSide::Current, true);
        assert_eq!(m.file_side_state(SelectionSide::Current), TriState::All);
        assert_eq!(
            m.hunk_side_state(0, SelectionSide::Current),
            Some(TriState::All)
        );
        assert!(m.set_hunk_line(0, SelectionSide::Current, 0, false));
        assert_eq!(m.file_side_state(SelectionSide::Current), TriState::Partial);
        assert_eq!(
            m.hunk_side_state(0, SelectionSide::Current),
            Some(TriState::None)
        );
    }

    #[test]
    fn hunk_manual_edit_provenance() {
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        m.set_choice(0, HunkChoice::Manual("merged a\n".to_string()));
        m.set_choice(1, HunkChoice::AcceptCurrent);
        let lines = m.assemble();
        let manual = lines.iter().find(|l| l.text == "merged a").unwrap();
        assert_eq!(manual.origin, LineOrigin::Manual);
    }

    #[test]
    fn hunk_reset_re_emits_markers_and_keeps_gate_closed() {
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        m.set_choice(0, HunkChoice::AcceptCurrent);
        m.set_choice(1, HunkChoice::AcceptIncoming);
        assert!(m.all_resolved());

        assert!(m.set_choice(1, HunkChoice::Unresolved));
        assert!(!m.all_resolved());
        assert_eq!(m.resolved_hunk_count(), 1);

        let text = m.assembled_text();
        assert!(crate::checklist::text_has_conflict_marker(&text));
    }

    #[test]
    fn all_resolved_file_assembled_result_is_marker_free() {
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        m.set_choice(0, HunkChoice::AcceptCurrent);
        m.set_choice(1, HunkChoice::AcceptIncoming);
        let text = m.assembled_text();
        assert!(
            !crate::checklist::text_has_conflict_marker(&text),
            "fully resolved Result must contain no conflict markers: {:?}",
            text
        );
    }
}
