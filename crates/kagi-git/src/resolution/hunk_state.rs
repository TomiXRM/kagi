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
                });
                saved.current.clone_from(&selection.current_taken);
                saved.incoming.clone_from(&selection.incoming_taken);
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
            hunk.line_select = decision.selection.as_ref().map(|selection| LineSelection {
                current_taken: selection.current.clone(),
                incoming_taken: selection.incoming.clone(),
                order: match selection.order {
                    Order::CurrentFirst => LineOrder::CurrentFirst,
                    Order::IncomingFirst => LineOrder::IncomingFirst,
                    Order::CurrentBlockFirst => LineOrder::CurrentBlockFirst,
                    Order::IncomingBlockFirst => LineOrder::IncomingBlockFirst,
                },
            });
        }
        true
    }
}
