//! Persist editing decisions, never the materialized source hunk data.
use super::*;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct HunkState {
    fingerprint: String,
    decisions: Vec<Decision>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Decision {
    choice: Choice,
    selection: Option<Selection>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Choice {
    Current,
    Incoming,
    BothCurrentFirst,
    BothIncomingFirst,
    Manual(String),
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Selection {
    current: Vec<bool>,
    incoming: Vec<bool>,
    order: Order,
    #[serde(default)]
    empty_taken: Option<[bool; 2]>,
}

// Mirror LineOrder's names; these spellings also define the persisted codec.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Order {
    CurrentFirst,
    IncomingFirst,
    CurrentBlockFirst,
    IncomingBlockFirst,
}

impl HunkState {
    pub(super) fn new(markers: &str, model: &HunkModel) -> Self {
        let mut state = Self {
            fingerprint: hex::encode(Sha256::digest(markers.as_bytes())),
            decisions: Vec::new(),
        };
        state.update(model);
        state
    }

    pub(super) fn update(&mut self, model: &HunkModel) {
        self.decisions.resize_with(model.hunk_count(), || Decision {
            choice: Choice::Unresolved,
            selection: None,
        });
        for (decision, hunk) in self
            .decisions
            .iter_mut()
            .zip(model.regions.iter().filter_map(|region| match region {
                Region::Hunk(hunk) => Some(hunk),
                _ => None,
            }))
        {
            if let (Choice::Manual(saved), HunkChoice::Manual(text)) =
                (&mut decision.choice, &hunk.choice)
            {
                saved.clone_from(text);
            } else {
                decision.choice = match &hunk.choice {
                    HunkChoice::AcceptCurrent => Choice::Current,
                    HunkChoice::AcceptIncoming => Choice::Incoming,
                    HunkChoice::BothCurrentFirst => Choice::BothCurrentFirst,
                    HunkChoice::BothIncomingFirst => Choice::BothIncomingFirst,
                    HunkChoice::Manual(text) => Choice::Manual(text.clone()),
                    HunkChoice::Unresolved => Choice::Unresolved,
                };
            }
            if let Some(selection) = &hunk.line_select {
                let saved = decision.selection.get_or_insert_with(|| Selection {
                    current: Vec::new(),
                    incoming: Vec::new(),
                    order: Order::CurrentFirst,
                    empty_taken: None,
                });
                selection
                    .taken(SelectionSide::Current)
                    .clone_into(&mut saved.current);
                selection
                    .taken(SelectionSide::Incoming)
                    .clone_into(&mut saved.incoming);
                saved.empty_taken = Some(selection.empty_taken());
                saved.order = match selection.order {
                    LineOrder::CurrentFirst => Order::CurrentFirst,
                    LineOrder::IncomingFirst => Order::IncomingFirst,
                    LineOrder::CurrentBlockFirst => Order::CurrentBlockFirst,
                    LineOrder::IncomingBlockFirst => Order::IncomingBlockFirst,
                };
            } else {
                decision.selection = None;
            }
        }
    }

    /// Three-way merge only independent decisions. A collision remains two
    /// recoverable drafts rather than guessing between acknowledged choices.
    pub(super) fn merge(&self, disk: &Self, base: Option<&Self>) -> Option<Self> {
        if self.fingerprint != disk.fingerprint
            || self.decisions.len() != disk.decisions.len()
            || base.is_some_and(|base| {
                base.fingerprint != self.fingerprint || base.decisions.len() != self.decisions.len()
            })
        {
            return None;
        }
        let unresolved = Decision {
            choice: Choice::Unresolved,
            selection: None,
        };
        let mut merged = self.clone();
        for (index, (ours, theirs)) in self.decisions.iter().zip(&disk.decisions).enumerate() {
            let before = base.map_or(&unresolved, |base| &base.decisions[index]);
            if ours == before {
                merged.decisions[index] = theirs.clone();
            } else if theirs != before && ours != theirs {
                return None;
            }
        }
        Some(merged)
    }

    pub(super) fn apply(&self, model: &mut HunkModel) -> bool {
        if self.decisions.len() != model.hunk_count() {
            return false;
        }
        self.restore_decisions(model)
    }

    pub(super) fn restore(&self, markers: &str, model: &mut HunkModel) -> bool {
        if self.fingerprint != hex::encode(Sha256::digest(markers.as_bytes()))
            || !self.restore_decisions(model)
        {
            return false;
        }
        true
    }

    fn restore_decisions(&self, model: &mut HunkModel) -> bool {
        if self.decisions.len() != model.hunk_count()
            || model
                .regions
                .iter()
                .filter_map(|region| match region {
                    Region::Hunk(hunk) => Some(hunk),
                    _ => None,
                })
                .zip(&self.decisions)
                .any(|(hunk, decision)| {
                    decision.selection.as_ref().is_some_and(|selection| {
                        selection.current.len() != hunk.current.len()
                            || selection.incoming.len() != hunk.incoming.len()
                    })
                })
        {
            return false;
        }
        for (hunk, decision) in model
            .regions
            .iter_mut()
            .filter_map(|region| match region {
                Region::Hunk(hunk) => Some(hunk),
                _ => None,
            })
            .zip(&self.decisions)
        {
            hunk.choice = match &decision.choice {
                Choice::Current => HunkChoice::AcceptCurrent,
                Choice::Incoming => HunkChoice::AcceptIncoming,
                Choice::BothCurrentFirst => HunkChoice::BothCurrentFirst,
                Choice::BothIncomingFirst => HunkChoice::BothIncomingFirst,
                Choice::Manual(text) => HunkChoice::Manual(text.clone()),
                Choice::Unresolved => HunkChoice::Unresolved,
            };
            hunk.line_select = if let Some(selection) = decision.selection.as_ref() {
                let order = match selection.order {
                    Order::CurrentFirst => LineOrder::CurrentFirst,
                    Order::IncomingFirst => LineOrder::IncomingFirst,
                    Order::CurrentBlockFirst => LineOrder::CurrentBlockFirst,
                    Order::IncomingBlockFirst => LineOrder::IncomingBlockFirst,
                };
                let empty_taken = selection.empty_taken.unwrap_or_else(|| {
                    [
                        hunk.current.is_empty() && hunk.choice.takes_side(SelectionSide::Current),
                        hunk.incoming.is_empty() && hunk.choice.takes_side(SelectionSide::Incoming),
                    ]
                });
                let Ok(restored) = LineSelection::new(
                    hunk.current.len(),
                    hunk.incoming.len(),
                    selection.current.clone(),
                    selection.incoming.clone(),
                    order,
                    empty_taken,
                ) else {
                    return false;
                };
                Some(restored)
            } else {
                None
            };
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_rejects_malformed_masks_without_partial_adoption() {
        let markers = "<<<<<<< Current\nc\n=======\ni\n>>>>>>> Incoming\n";
        let mut chosen = HunkModel::from_marker_text(markers);
        chosen.set_hunk_side(0, SelectionSide::Current, true);
        let mut state = HunkState::new(markers, &chosen);
        state.decisions[0]
            .selection
            .as_mut()
            .unwrap()
            .incoming
            .push(true);
        let mut unresolved = HunkModel::from_marker_text(markers);
        let before = unresolved.clone();
        assert!(!state.restore(markers, &mut unresolved));
        assert_eq!(unresolved, before);
    }

    #[test]
    fn empty_side_adoption_survives_codec_round_trip() {
        let markers = "<<<<<<< Current\n=======\ni\n>>>>>>> Incoming\n";
        let mut model = HunkModel::from_marker_text(markers);
        assert!(model.set_hunk_side(0, SelectionSide::Current, true));
        let state = HunkState::new(markers, &model);
        let encoded = serde_json::to_string(&state).unwrap();
        let decoded: HunkState = serde_json::from_str(&encoded).unwrap();
        let mut restored = HunkModel::from_marker_text(markers);
        assert!(decoded.restore(markers, &mut restored));
        assert_eq!(model, restored);
        assert_eq!(
            restored.hunk_side_state(0, SelectionSide::Current),
            Some(TriState::All)
        );

        // Pre-amendment drafts have no explicit empty-side metadata.
        model.set_choice(0, HunkChoice::AcceptCurrent);
        model.regions.iter_mut().for_each(|region| {
            if let Region::Hunk(hunk) = region {
                hunk.ensure_line_selection();
            }
        });
        let mut legacy = HunkState::new(markers, &model);
        legacy.decisions[0].selection.as_mut().unwrap().empty_taken = None;
        assert!(legacy.restore(markers, &mut restored));
        assert_eq!(
            restored.hunk_side_state(0, SelectionSide::Current),
            Some(TriState::All)
        );
    }
}
