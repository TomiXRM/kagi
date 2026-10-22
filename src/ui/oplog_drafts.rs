//! Explicit read-only preservation inspection inside the existing Operation Log.
//! The owner and loaded data are view metadata, never resolution authority.

use super::{OpLogPanel, OpLogPanelEvent};
use crate::app::{Attachment, SessionId};
use crate::ui::i18n::{self, Lang, Msg};
use crate::ui::{button_style, e2e, oplog_render, render_helpers, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, rgb, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled,
};
use gpui_component::button::Button;
use gpui_component::{Disableable as _, Sizable as _};
use kagi_ui_core::i18n::oplog_panel::OplogPanelMsg as P;
use std::cell::RefCell;
use std::path::Path;

#[cfg(feature = "gui-e2e")]
thread_local! {
    static LOAD_HOLD: RefCell<Option<gpui::Task<()>>> = const { RefCell::new(None) };
}

/// Delay admission of a real read, not its backend result (Tier A only).
#[cfg(feature = "gui-e2e")]
pub(in crate::ui) fn take_load_hold() -> Option<gpui::Task<()>> {
    LOAD_HOLD.with(|slot| slot.borrow_mut().take())
}

pub(super) struct DraftOwner {
    attachment: Attachment,
    label: SharedString,
}

pub(super) enum DraftInspection {
    Loading,
    Loaded(Option<DraftSnapshot>),
    Unavailable(SharedString),
}

/// Frozen inspection data, with presentation cached only for the current language.
/// Changing language neither reloads the repository nor grants draft authority.
pub(super) struct DraftSnapshot {
    buffer: kagi_git::ResolutionBuffer,
    localized: RefCell<Option<(Lang, SharedString)>>,
}

impl DraftSnapshot {
    fn text(&self) -> SharedString {
        let language = i18n::lang();
        let mut localized = self.localized.borrow_mut();
        if let Some((cached_language, text)) = localized.as_ref() {
            if *cached_language == language {
                return text.clone();
            }
        }
        let text = SharedString::from(snapshot(&self.buffer));
        *localized = Some((language, text.clone()));
        text
    }
}

fn same_owner(left: &Attachment, right: &Attachment) -> bool {
    left.session == right.session
        && left.visit == right.visit
        && left.path == right.path
        && left.worktree == right.worktree
}

impl OpLogPanel {
    /// Hold the next real inspection result before its owner/generation checks.
    #[cfg(feature = "gui-e2e")]
    pub fn hold_next_preserved_conflict_draft_for_e2e(hold: gpui::Task<()>) {
        LOAD_HOLD.with(|slot| assert!(slot.borrow_mut().replace(hold).is_none()));
    }

    /// Compare borrowed attachment metadata before cloning only on an owner
    /// change in the parent render-sync pass.
    pub(crate) fn preserved_draft_owner_matches(
        &self,
        session: Option<SessionId>,
        visit: Option<u64>,
        path: Option<&Path>,
        worktree: Option<&crate::app::WorktreeId>,
    ) -> bool {
        match (&self.draft_owner, session) {
            (None, None) => true,
            (Some(owner), Some(session)) => {
                owner.attachment.session == session
                    && Some(owner.attachment.visit) == visit
                    && Some(owner.attachment.path.as_path()) == path
                    && owner.attachment.worktree.as_ref() == worktree
            }
            _ => false,
        }
    }

    pub(crate) fn set_preserved_draft_owner(
        &mut self,
        owner: Option<Attachment>,
        cx: &mut Context<Self>,
    ) {
        self.close_preserved_conflict_drafts(cx);
        self.draft_owner = owner.map(|attachment| DraftOwner {
            label: kagi_domain::text_safety::sanitize_control_bytes(
                &attachment.path.to_string_lossy(),
            )
            .into(),
            attachment,
        });
        cx.notify();
    }

    pub(crate) fn begin_preserved_conflict_drafts(
        &mut self,
        owner: &Attachment,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        self.draft_owner
            .as_ref()
            .filter(|current| same_owner(&current.attachment, owner))?;
        self.draft_generation = self.draft_generation.wrapping_add(1);
        self.draft_inspection = Some(DraftInspection::Loading);
        self.draft_scroll = Default::default();
        cx.notify();
        Some(self.draft_generation)
    }

    pub(crate) fn finish_preserved_conflict_drafts(
        &mut self,
        owner: &Attachment,
        generation: u64,
        result: Result<Option<kagi_git::ResolutionBuffer>, String>,
        cx: &mut Context<Self>,
    ) {
        if self.draft_generation != generation
            || self.draft_inspection.is_none()
            || !self
                .draft_owner
                .as_ref()
                .is_some_and(|current| same_owner(&current.attachment, owner))
        {
            return;
        }
        self.draft_inspection = Some(match result {
            Ok(buffer) => DraftInspection::Loaded(buffer.map(|buffer| DraftSnapshot {
                buffer,
                localized: RefCell::new(None),
            })),
            Err(error) => DraftInspection::Unavailable(error.into()),
        });
        cx.notify();
    }

    pub(super) fn close_preserved_conflict_drafts(&mut self, cx: &mut Context<Self>) {
        self.draft_generation = self.draft_generation.wrapping_add(1);
        if self.draft_inspection.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn preserved_conflict_draft_open(&self) -> bool {
        self.draft_inspection.is_some()
    }

    #[cfg(feature = "gui-e2e")]
    pub fn preserved_conflict_draft_unavailable(&self) -> bool {
        matches!(
            self.draft_inspection.as_ref(),
            Some(DraftInspection::Unavailable(_))
        )
    }

    pub fn preserved_conflict_draft_text(&self) -> Option<SharedString> {
        match self.draft_inspection.as_ref()? {
            DraftInspection::Loaded(Some(snapshot)) => Some(snapshot.text()),
            _ => None,
        }
    }

    /// The root checks the actual current Attachment before asking for this
    /// explicit draft-only clipboard payload. Receipt Copy never calls it.
    pub(crate) fn preserved_conflict_draft_copy(
        &self,
        owner: &Attachment,
        generation: u64,
    ) -> Option<SharedString> {
        if self.draft_generation != generation
            || !self
                .draft_owner
                .as_ref()
                .is_some_and(|current| same_owner(&current.attachment, owner))
        {
            return None;
        }
        self.preserved_conflict_draft_text()
    }

    fn request_preserved_conflict_drafts(&self, cx: &mut Context<Self>) {
        if let Some(owner) = &self.draft_owner {
            cx.emit(OpLogPanelEvent::InspectPreservedDrafts {
                owner: owner.attachment.clone(),
            });
        }
    }

    fn request_preserved_conflict_draft_copy(&self, cx: &mut Context<Self>) {
        if self.preserved_conflict_draft_text().is_some() {
            if let Some(owner) = &self.draft_owner {
                cx.emit(OpLogPanelEvent::CopyPreservedDrafts {
                    owner: owner.attachment.clone(),
                    generation: self.draft_generation,
                });
            }
        }
    }
}

pub(in crate::ui) fn load(
    owner: &Attachment,
) -> Result<Option<kagi_git::ResolutionBuffer>, String> {
    let backend = kagi_git::Backend::open(&owner.path).map_err(|error| error.to_string())?;
    if owner.worktree.is_none()
        || backend.write_worktree_id().ok().as_ref() != owner.worktree.as_ref()
    {
        return Err("worktree identity changed; reopen the repository".into());
    }
    let buffer = backend
        .resolution_buffer_from_repo_with_autosave()
        .map_err(|error| error.to_string())?;
    let has_drafts = buffer.recovery_drafts().next().is_some();
    Ok(has_drafts.then_some(buffer))
}

fn snapshot(buffer: &kagi_git::ResolutionBuffer) -> String {
    use std::fmt::Write as _;
    let drafts = buffer.recovery_drafts();
    let mut text = Msg::OplogPanel(P::PreservedDraftSnapshot).t().to_string();
    for draft in drafts {
        text.push_str("\n\n");
        let _ = writeln!(
            text,
            "{}: {}",
            Msg::OplogPanel(P::DetailFile).t(),
            draft.path.display()
        );
        let identity = draft
            .identity
            .map(|identity| identity.as_str())
            .unwrap_or_else(|| Msg::OplogPanel(P::PreservedDraftUnbound).t());
        let _ = writeln!(
            text,
            "{}: {identity}",
            Msg::OplogPanel(P::PreservedDraftIdentity).t()
        );
        if let Some(lines) = draft.result {
            for line in lines {
                text.push_str(&line.text);
                text.push('\n');
            }
        }
        if let Some(raw) = draft.raw_result {
            let _ = writeln!(text, "OID: {} / mode {:o}", raw.oid, raw.mode);
        }
    }
    text
}

pub(in crate::ui) fn render_header(
    panel: &OpLogPanel,
    cx: &mut Context<OpLogPanel>,
) -> gpui::AnyElement {
    let inspecting = panel.preserved_conflict_draft_open();
    let available = panel
        .draft_owner
        .as_ref()
        .is_some_and(|owner| owner.attachment.worktree.is_some());
    let (id, label) = if inspecting {
        (
            "oplog-preserved-drafts-back",
            Msg::OplogPanel(P::PreservedDraftBack).t(),
        )
    } else {
        (
            "oplog-preserved-drafts-open",
            Msg::OplogPanel(P::PreservedDraftOpen).t(),
        )
    };
    let button = Button::new(id)
        .label(label)
        .small()
        .disabled(!available)
        .on_click(cx.listener(move |panel, _, _, cx| {
            if !available {
                return;
            }
            if inspecting {
                panel.close_preserved_conflict_drafts(cx);
            } else {
                panel.request_preserved_conflict_drafts(cx);
            }
        }));
    div()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .border_b_1()
        .border_color(rgb(theme::theme().surface))
        .child(e2e::measure_control(
            id,
            button_style::named_button_with_description(
                format!("{id}-help"),
                button,
                label.into(),
                None,
                (!available).then(|| {
                    Msg::OplogPanel(P::PreservedDraftOwnerUnavailable)
                        .t()
                        .into()
                }),
                !available,
            ),
        ))
        .when_some(panel.draft_owner.as_ref(), |header, owner| {
            header.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_xs()
                    .text_color(rgb(theme::theme().text_muted))
                    .child(owner.label.clone()),
            )
        })
        .into_any_element()
}

pub(in crate::ui) fn render_inspection(
    panel: &OpLogPanel,
    cx: &mut Context<OpLogPanel>,
) -> gpui::AnyElement {
    let Some(state) = panel.draft_inspection.as_ref() else {
        return div().into_any_element();
    };
    let text = panel.preserved_conflict_draft_text();
    let copyable = text.is_some();
    let reason = match state {
        DraftInspection::Loading => Some(P::PreservedDraftLoading),
        DraftInspection::Loaded(None) => Some(P::PreservedDraftEmpty),
        DraftInspection::Unavailable(_) => Some(P::PreservedDraftUnavailable),
        DraftInspection::Loaded(Some(_)) => None,
    };
    let label = Msg::OplogPanel(P::PreservedDraftCopy).t();
    let copy = Button::new("oplog-preserved-drafts-copy")
        .label(label)
        .small()
        .disabled(!copyable)
        .on_click(cx.listener(|panel, _, _, cx| panel.request_preserved_conflict_draft_copy(cx)));
    let mut body = div()
        .id("oplog-preserved-drafts-body")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .track_scroll(&panel.draft_scroll)
        .px_3()
        .py_2()
        .text_xs()
        .text_color(rgb(theme::theme().text_sub));
    body = match (state, text) {
        (DraftInspection::Loaded(Some(_)), Some(text)) => body.child(
            oplog_render::selectable_text("oplog-preserved-drafts-text".into(), &text)
                .selectable(text.len() <= oplog_render::SELECTABLE_DETAIL_MAX),
        ),
        (DraftInspection::Unavailable(error), _) => body
            .child(Msg::OplogPanel(P::PreservedDraftUnavailable).t())
            .child(
                div()
                    .pt_2()
                    .child(kagi_ui_core::i18n::op::user_text::diagnostic(error).into_owned()),
            ),
        _ => body.child(Msg::OplogPanel(reason.unwrap_or(P::PreservedDraftEmpty)).t()),
    };
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(div().px_3().py_1().child(e2e::measure_control(
            "oplog-preserved-drafts-copy",
            button_style::named_button_with_description(
                "oplog-preserved-drafts-copy-help",
                copy,
                label.into(),
                None,
                reason.map(|reason| Msg::OplogPanel(reason).t().into()),
                !copyable,
            ),
        )))
        .child(render_helpers::with_vertical_scrollbar(
            "oplog-preserved-drafts-scroll",
            &panel.draft_scroll,
            body,
            true,
        ))
        .into_any_element()
}
