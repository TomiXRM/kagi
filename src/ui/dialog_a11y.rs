//! Accessibility for confirmation cards (#354 slice 2).
//!
//! A plan card is a dialog: `Role::Dialog`, or `Role::AlertDialog` when the
//! operation is destructive or needs a second confirm (ADR-0023). Its name is
//! the card title; its description says whether the next Confirm runs the
//! operation. It is marked modal and advertises two custom actions, Confirm
//! and Cancel, routed through [`dialog_action`] to the same handlers as the
//! buttons. (accesskit 0.24 has no Dismiss action; gpui_component's Button
//! already exposes `Role::Button` named by its visible label — including the
//! armed wording — but it cannot carry `on_a11y_action`, so the explicit
//! actions live on the dialog.) Warnings are `Role::Note`, blockers
//! `Role::Alert`, each named by its text.
//!
//! The AccessKit tree only exists while an assistive technology is
//! connected, so Tier A reads what the renderer *set*, recorded here the same
//! way the toolbar bridge does (#797, #840).

use std::rc::Rc;

use gpui::{
    AccessibleAction, App, Div, Role, SharedString, Stateful, StatefulInteractiveElement, Window,
};
use kagi_ui_core::i18n::Msg;

/// Custom action id for Confirm.
pub(crate) const CONFIRM_ACTION: i32 = 1;
/// Custom action id for Cancel.
pub(crate) const CANCEL_ACTION: i32 = 2;

/// Where a card is in its confirm sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmStage {
    /// One confirm runs it.
    Single,
    /// Two-stage, first confirm still pending.
    Unarmed,
    /// Two-stage, the next confirm runs it.
    Armed,
}

impl ConfirmStage {
    /// Stage of a two-stage card from its `confirm_armed` flag.
    pub fn two_stage(armed: bool) -> Self {
        if armed {
            ConfirmStage::Armed
        } else {
            ConfirmStage::Unarmed
        }
    }
}

/// What the renderer sets on the dialog node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogA11y {
    pub role: Role,
    pub label: String,
    pub description: Option<String>,
    /// `(id, description)` custom actions, Confirm first when present.
    pub actions: Vec<(i32, String)>,
}

impl DialogA11y {
    /// Add the plan's recovery explanation after the confirm-stage instruction.
    /// Even a plan with no shell commands retains its accessible recovery advice.
    pub(crate) fn with_recovery(mut self, recovery: &str) -> Self {
        if !recovery.is_empty() {
            match &mut self.description {
                Some(description) => {
                    description.push('\n');
                    description.push_str(recovery);
                }
                None => self.description = Some(recovery.to_owned()),
            }
        }
        self
    }
}

/// The dialog's role, name, description and actions. `confirm_label` is
/// `None` when blockers hide the confirm button — the action is then not
/// offered either.
pub fn dialog_a11y(
    title: &str,
    confirm_label: Option<&str>,
    destructive: bool,
    stage: ConfirmStage,
) -> DialogA11y {
    let role = if destructive || stage != ConfirmStage::Single {
        Role::AlertDialog
    } else {
        Role::Dialog
    };
    let description = match (stage, confirm_label) {
        (_, None) => Some(Msg::A11yDialogBlocked.t().to_string()),
        (ConfirmStage::Unarmed, Some(_)) => Some(Msg::A11yDialogTwoStage.t().to_string()),
        (ConfirmStage::Armed, Some(_)) => Some(Msg::A11yDialogArmed.t().to_string()),
        (ConfirmStage::Single, Some(_)) => None,
    };
    let mut actions = Vec::with_capacity(2);
    if let Some(label) = confirm_label {
        actions.push((CONFIRM_ACTION, label.to_string()));
    }
    actions.push((CANCEL_ACTION, Msg::PlanCancel.t().to_string()));
    DialogA11y {
        role,
        label: title.to_string(),
        description,
        actions,
    }
}

/// Which handler an AX custom action request maps to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogAction {
    Confirm,
    Cancel,
}

/// Route a `CustomAction` request. Anything else (no data, another action
/// id, another data kind) is ignored.
pub fn dialog_action(data: Option<&gpui::accesskit::ActionData>) -> Option<DialogAction> {
    match data {
        Some(gpui::accesskit::ActionData::CustomAction(CONFIRM_ACTION)) => {
            Some(DialogAction::Confirm)
        }
        Some(gpui::accesskit::ActionData::CustomAction(CANCEL_ACTION)) => {
            Some(DialogAction::Cancel)
        }
        _ => None,
    }
}

/// A handler shared between a button's click and the dialog's AX action.
pub(crate) type DialogHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Set role, name, description, modality and the Confirm/Cancel custom
/// actions on a card element (which must already carry an id).
pub(crate) fn apply_dialog(
    id: &'static str,
    el: Stateful<Div>,
    spec: DialogA11y,
    on_confirm: Option<DialogHandler>,
    on_cancel: DialogHandler,
) -> Stateful<Div> {
    record_dialog(id, &spec);
    let actions: Vec<gpui::accesskit::CustomAction> = spec
        .actions
        .iter()
        .map(|(id, description)| gpui::accesskit::CustomAction {
            id: *id,
            description: description.as_str().into(),
        })
        .collect();
    let el = el
        .role(spec.role)
        .aria_label(SharedString::from(spec.label));
    let el = match spec.description {
        Some(d) => el.aria_description(SharedString::from(d)),
        None => el,
    };
    el.a11y_synthetic_children(move |builder: &mut gpui::A11ySubtreeBuilder| {
        let node = builder.parent_node();
        node.set_modal();
        node.set_custom_actions(actions);
    })
    .on_a11y_action(
        AccessibleAction::CustomAction,
        move |data, window, cx| match dialog_action(data) {
            Some(DialogAction::Confirm) => {
                if let Some(confirm) = &on_confirm {
                    confirm(window, cx);
                }
            }
            Some(DialogAction::Cancel) => on_cancel(window, cx),
            None => {}
        },
    )
}

/// Role for a plan note row.
pub fn note_role(blocker: bool) -> Role {
    if blocker {
        Role::Alert
    } else {
        Role::Note
    }
}

/// Name a note row (which must already carry an id).
pub(crate) fn apply_note(
    id: SharedString,
    el: Stateful<Div>,
    blocker: bool,
    text: &str,
) -> Stateful<Div> {
    record_note(&id, note_role(blocker), text);
    el.role(note_role(blocker))
        .aria_label(SharedString::from(text.to_string()))
}

/// Record and expose the localized name of a plan-state group.
pub(crate) fn apply_group(
    id: &'static str,
    el: Stateful<Div>,
    label: SharedString,
) -> Stateful<Div> {
    record_note(id, Role::Group, label.as_ref());
    el.role(Role::Group).aria_label(label)
}
// ── Tier A recorder (gui-e2e only) ─────────────────────────────────────────

#[cfg(feature = "gui-e2e")]
thread_local! {
    static DIALOGS: std::cell::RefCell<std::collections::HashMap<&'static str, DialogA11y>> =
        std::cell::RefCell::new(Default::default());
    static NOTES: std::cell::RefCell<std::collections::HashMap<String, (Role, String)>> =
        std::cell::RefCell::new(Default::default());
}

#[cfg(feature = "gui-e2e")]
fn record_dialog(id: &'static str, spec: &DialogA11y) {
    DIALOGS.with(|m| m.borrow_mut().insert(id, spec.clone()));
}
#[cfg(not(feature = "gui-e2e"))]
#[inline]
fn record_dialog(_id: &'static str, _spec: &DialogA11y) {}

#[cfg(feature = "gui-e2e")]
fn record_note(id: &str, role: Role, text: &str) {
    NOTES.with(|m| {
        m.borrow_mut()
            .insert(id.to_string(), (role, text.to_string()))
    });
}
#[cfg(not(feature = "gui-e2e"))]
#[inline]
fn record_note(_id: &str, _role: Role, _text: &str) {}

/// What the last drawn frame set on dialog `id`.
#[cfg(feature = "gui-e2e")]
pub fn recorded_dialog(id: &str) -> Option<DialogA11y> {
    DIALOGS.with(|m| m.borrow().get(id).cloned())
}

/// What the last drawn frame set on note row `id`.
#[cfg(feature = "gui-e2e")]
pub fn recorded_note(id: &str) -> Option<(Role, String)> {
    NOTES.with(|m| m.borrow().get(id).cloned())
}

/// Forget recorded values so the next draw proves presence.
#[cfg(feature = "gui-e2e")]
pub fn clear_recorded_a11y() {
    DIALOGS.with(|m| m.borrow_mut().clear());
    NOTES.with(|m| m.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::accesskit::ActionData;

    #[test]
    fn role_follows_destructiveness_and_stage() {
        let r = |d, s| dialog_a11y("t", Some("Go"), d, s).role;
        assert_eq!(r(false, ConfirmStage::Single), Role::Dialog);
        assert_eq!(r(true, ConfirmStage::Single), Role::AlertDialog);
        assert_eq!(r(false, ConfirmStage::Unarmed), Role::AlertDialog);
        assert_eq!(r(false, ConfirmStage::Armed), Role::AlertDialog);
    }

    #[test]
    fn description_and_actions_track_the_confirm_sequence() {
        let single = dialog_a11y("Delete", Some("Delete"), false, ConfirmStage::Single);
        assert_eq!(single.label, "Delete");
        assert_eq!(single.description, None);
        assert_eq!(single.actions[0], (CONFIRM_ACTION, "Delete".to_string()));
        assert_eq!(single.actions[1].0, CANCEL_ACTION);

        let unarmed = dialog_a11y("t", Some("Replay x"), true, ConfirmStage::Unarmed);
        let armed = dialog_a11y("t", Some("Really replay"), true, ConfirmStage::Armed);
        assert!(unarmed.description.is_some());
        assert!(armed.description.is_some());
        assert_ne!(
            unarmed.description, armed.description,
            "arming changes the description"
        );
        assert_eq!(
            armed.actions[0].1, "Really replay",
            "Confirm is named by the armed label"
        );

        let blocked = dialog_a11y("t", None, true, ConfirmStage::Unarmed);
        assert_eq!(
            blocked.actions.len(),
            1,
            "no Confirm action when blockers hide the button"
        );
        assert_eq!(blocked.actions[0].0, CANCEL_ACTION);
        assert!(blocked.description.is_some());
    }

    #[test]
    fn recovery_follows_stage_and_is_not_lost_without_commands() {
        for stage in [
            ConfirmStage::Single,
            ConfirmStage::Unarmed,
            ConfirmStage::Armed,
        ] {
            let spec = dialog_a11y("plan", Some("Go"), true, stage)
                .with_recovery("Return to previous branch.");
            let expected = match stage {
                ConfirmStage::Single => "Return to previous branch.".to_owned(),
                ConfirmStage::Unarmed => format!(
                    "{}\nReturn to previous branch.",
                    Msg::A11yDialogTwoStage.t()
                ),
                ConfirmStage::Armed => {
                    format!("{}\nReturn to previous branch.", Msg::A11yDialogArmed.t())
                }
            };
            assert_eq!(spec.description.as_deref(), Some(expected.as_str()));
            assert_eq!(spec.actions.len(), 2);
        }
    }

    #[test]
    fn custom_actions_route_to_confirm_and_cancel_only() {
        assert_eq!(
            dialog_action(Some(&ActionData::CustomAction(CONFIRM_ACTION))),
            Some(DialogAction::Confirm)
        );
        assert_eq!(
            dialog_action(Some(&ActionData::CustomAction(CANCEL_ACTION))),
            Some(DialogAction::Cancel)
        );
        assert_eq!(dialog_action(Some(&ActionData::CustomAction(99))), None);
        assert_eq!(dialog_action(Some(&ActionData::Value("x".into()))), None);
        assert_eq!(dialog_action(None), None);
    }

    #[test]
    fn notes_are_note_or_alert() {
        assert_eq!(note_role(false), Role::Note);
        assert_eq!(note_role(true), Role::Alert);
    }
}
