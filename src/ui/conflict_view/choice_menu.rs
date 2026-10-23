//! Gutter pull-down: shared menu overlay and focus lifecycle, in-memory choices only.
use super::*;
use kagi_git::resolution::HunkChoice;

#[derive(Clone)]
enum ChoiceAction {
    Choose(HunkChoice),
    Reset,
}

pub fn render_choice_menu(
    entity: &Entity<ConflictView>,
    window_active: bool,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> Option<gpui::AnyElement> {
    let view = entity.read(cx);
    let keys = view.choice_menu_keys.clone();
    let fallback = view.navigation_focus.clone();
    let open = view.choice_menu.is_some();
    keys.sync(open, false, fallback.as_ref(), window, cx);
    let view = entity.read(cx);
    let (hunk, position) = view.choice_menu?;
    let path = view.editing.clone()?;
    let mode = view.mode.as_ref()?;
    let enabled = !view.writer_busy && !flow::manual_override(mode, &path);
    let reset_enabled = !view.writer_busy;
    let armed = view.reset_all_target.as_deref() == Some(path.as_path());
    let item = |action, label: &str, enabled| {
        MenuItem::command(
            action,
            SharedString::from(label.to_owned()),
            if enabled {
                ItemState::Enabled
            } else {
                ItemState::Disabled(
                    if view.writer_busy {
                        Msg::OpInProgress.t()
                    } else {
                        Msg::ConflictManualResult.t()
                    }
                    .into(),
                )
            },
            false,
        )
    };
    let groups = vec![
        MenuGroup {
            title: None,
            items: vec![
                item(
                    ChoiceAction::Choose(HunkChoice::AcceptCurrent),
                    Msg::ConflictUseLeft.t(),
                    enabled,
                ),
                item(
                    ChoiceAction::Choose(HunkChoice::AcceptIncoming),
                    Msg::ConflictUseRight.t(),
                    enabled,
                ),
            ],
        },
        MenuGroup {
            title: None,
            items: vec![
                item(
                    ChoiceAction::Choose(HunkChoice::BothCurrentFirst),
                    Msg::ConflictUseBothLeftFirst.t(),
                    enabled,
                ),
                item(
                    ChoiceAction::Choose(HunkChoice::BothIncomingFirst),
                    Msg::ConflictUseBothRightFirst.t(),
                    enabled,
                ),
            ],
        },
        MenuGroup {
            title: None,
            items: vec![item(
                ChoiceAction::Reset,
                if armed {
                    Msg::EditorResetAllConfirm.t()
                } else {
                    Msg::EditorReset.t()
                },
                reset_enabled,
            )],
        },
    ];
    let dismiss = entity.clone();
    let select = entity.clone();
    Some(menu_overlay::render_menu_overlay(
        "conflict-choice-menu",
        "conflict-choice-menu-item",
        196.,
        "",
        position,
        SharedString::default(),
        groups,
        move |_, _, cx| {
            dismiss.update(cx, |view, cx| {
                view.choice_menu = None;
                view.reset_all_target = None;
                cx.notify();
            });
        },
        move |_, action, _, cx| {
            select.update(cx, |view, cx| {
                view.choice_menu = None;
                if view.editing.as_deref() != Some(path.as_path()) {
                    return;
                }
                match action {
                    ChoiceAction::Choose(choice) => {
                        view.conflict_choose_hunk(&path, hunk, choice, cx)
                    }
                    ChoiceAction::Reset => view.conflict_editor_reset_all_request(&path),
                }
                cx.notify();
            });
        },
        Some(&keys),
        window_active,
        window,
        cx,
    ))
}
