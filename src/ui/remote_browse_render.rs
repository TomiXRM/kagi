//! Viewport-bounded SSH connection and directory task presentation.
use super::*;

// ──────────────────────────────────────────────────────────────
// Renderer
// ──────────────────────────────────────────────────────────────

/// One labelled field: a small muted label over a soft rounded box holding a
/// borderless input (the same look as Home's search field). The box is a
/// flex row so the input takes its whole width.
fn labeled_input(
    label: &'static str,
    state: Option<&Entity<gpui_component::input::InputState>>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1p5()
        .child(
            div()
                .text_xs()
                .text_color(rgb(current_theme().text_muted))
                .child(SharedString::from(label)),
        )
        .children(state.map(|st| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                .px_2()
                .rounded_lg()
                .bg(rgb(current_theme().bg_base))
                .border_1()
                .border_color(rgb(current_theme().surface))
                .child(Input::new(st).aria_label(label).appearance(false).flex_1())
        }))
}

pub(crate) fn render_remote_browse(
    modal: RemoteBrowseModal,
    focus_handle: Option<FocusHandle>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let busy = modal.busy;
    // Advertise exactly the default action currently available in this task.
    let dialog_confirm = match &modal.stage {
        RemoteBrowseStage::Connect if !busy => Some(Msg::RemoteConnect.t()),
        RemoteBrowseStage::Browse if modal.current_is_repo && !busy => {
            Some(Msg::RemoteOpenRepository.t())
        }
        _ => None,
    };
    let dialog_title = match &modal.stage {
        RemoteBrowseStage::Connect => Msg::RemoteConnectTitle.t().to_owned(),
        RemoteBrowseStage::Browse => modal
            .host
            .as_ref()
            .map(|host| host.label())
            .unwrap_or_default(),
    };
    let app = cx.entity();

    // Cancel/close: an `&mut App` handler (gpui-component Button form) that
    // closes the modal and restores root focus.
    let cancel = {
        let app = app.clone();
        move |_e: &ClickEvent, window: &mut Window, cx: &mut App| {
            app.update(cx, |this, cx| {
                this.cancel_remote_browse();
                if let Some(fh) = this.root_focus.clone() {
                    window.focus(&fh, cx);
                }
                cx.notify();
            });
        }
    };

    let mut card = crate::ui::modal_shell::modal_card(520.)
        .relative()
        .child(super::e2e::measure_inside("remote-browse-card"));
    let mut body = crate::ui::modal_shell::modal_scroll_body()
        .relative()
        .child(super::e2e::measure_inside("remote-browse-body"))
        .when(modal.stage == RemoteBrowseStage::Browse, |body| {
            body.max_h(theme::scaled_px(220.))
        });

    match modal.stage {
        // ── Connect form ────────────────────────────────────────
        RemoteBrowseStage::Connect => {
            let connect = {
                let app = app.clone();
                move |_e: &ClickEvent, _w: &mut Window, cx: &mut App| {
                    app.update(cx, |this, cx| {
                        this.start_remote_connect(cx);
                        cx.notify();
                    });
                }
            };

            body = body
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_main))
                                .text_lg()
                                .child(SharedString::from(Msg::RemoteConnectTitle.t())),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(current_theme().text_muted))
                                .child(SharedString::from(Msg::RemoteSshNotice.t())),
                        ),
                )
                .child(labeled_input(
                    Msg::RemoteHostLabel.t(),
                    modal.host_state.as_ref(),
                ))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_3()
                        .child(div().w(theme::scaled_px(120.)).child(labeled_input(
                            Msg::RemotePortLabel.t(),
                            modal.port_state.as_ref(),
                        )))
                        .child(div().flex_1().child(labeled_input(
                            Msg::RemoteIdentityLabel.t(),
                            modal.identity_state.as_ref(),
                        ))),
                );

            if let Some(ref err) = modal.error {
                body = body.child(diagnostic(err.clone()));
            }

            card = card.child(body).child(
                super::button_style::modal_button_row()
                    .pt_2()
                    .flex_shrink_0()
                    .child(super::e2e::measure_control(
                        "remote-connect-cancel",
                        modal_button(
                            "remote-connect-cancel",
                            Msg::PlanCancel.t(),
                            ModalButtonKind::Cancel,
                            None,
                            cancel,
                            cx,
                        ),
                    ))
                    .child(super::e2e::measure_control(
                        "remote-connect-go",
                        modal_button(
                            "remote-connect-go",
                            if busy {
                                Msg::RemoteConnecting.t()
                            } else {
                                Msg::RemoteConnect.t()
                            },
                            ModalButtonKind::Primary,
                            busy.then(|| SharedString::from(Msg::RemoteRequestBusy.t())),
                            connect,
                            cx,
                        ),
                    )),
            );
        }

        // ── Directory browser ───────────────────────────────────
        RemoteBrowseStage::Browse => {
            let host_label = modal.host.as_ref().map(|h| h.label()).unwrap_or_default();

            let change_host = {
                let app = app.clone();
                move |_e: &ClickEvent, _w: &mut Window, cx: &mut App| {
                    app.update(cx, |this, cx| {
                        if let Some(m) = this.remote_browse_mut() {
                            m.stage = RemoteBrowseStage::Connect;
                            m.error = None;
                        }
                        cx.notify();
                    });
                }
            };

            // Header: host + current path + "Change host".
            body = body.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                identity("remote-browse-host", host_label.into())
                                    .text_color(rgb(current_theme().text_main))
                                    .text_lg(),
                            )
                            .child(
                                identity("remote-browse-path", modal.cwd.clone().into())
                                    .text_xs()
                                    .text_color(rgb(current_theme().text_sub)),
                            )
                            .when(busy, |identity| {
                                identity.child(SharedString::from(Msg::RemoteRequestBusy.t()))
                            }),
                    )
                    .child(modal_button(
                        "remote-change-host",
                        Msg::RemoteChangeHost.t(),
                        ModalButtonKind::Secondary,
                        busy.then(|| SharedString::from(Msg::RemoteRequestBusy.t())),
                        change_host,
                        cx,
                    )),
            );

            // Repo card when the current directory is itself a repository.
            if modal.current_is_repo {
                let mut repo_card = div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_2()
                    .rounded_md()
                    .bg(rgb(current_theme().surface))
                    .when(current_theme().slug == "apple-dark", |card| {
                        card.border_1().border_color(rgb(
                            current_theme().elevated_border(current_theme().surface)
                        ))
                    })
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(current_theme().color_success))
                            .child(SharedString::from(format!(
                                "\u{25cf} {}",
                                Msg::RemoteGitRepository.t()
                            ))),
                    );
                if let Some(ref s) = modal.summary {
                    let branch = s
                        .branch
                        .clone()
                        .unwrap_or_else(|| Msg::RemoteDetached.t().to_string());
                    repo_card = repo_card
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_2()
                                .text_sm()
                                .child(
                                    div()
                                        .text_color(rgb(current_theme().text_main))
                                        .child(SharedString::from(branch)),
                                )
                                .child(
                                    div()
                                        .text_color(rgb(current_theme().text_sub))
                                        .child(SharedString::from(s.head_short.clone())),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(current_theme().text_muted))
                                .overflow_hidden()
                                .child(SharedString::from(s.summary.clone())),
                        );
                } else {
                    repo_card = repo_card.child(
                        div()
                            .text_xs()
                            .text_color(rgb(current_theme().text_muted))
                            .child(SharedString::from(Msg::RemoteNoCommits.t())),
                    );
                }
                let open_repo = {
                    let app = app.clone();
                    move |_e: &ClickEvent, _w: &mut Window, cx: &mut App| {
                        app.update(cx, |this, cx| {
                            this.start_remote_open_repo(cx);
                            cx.notify();
                        });
                    }
                };
                repo_card = repo_card.child(div().pt_1().child(modal_button(
                    "remote-open-repo",
                    if busy {
                        Msg::RemoteOpeningRepository.t()
                    } else {
                        Msg::RemoteOpenRepository.t()
                    },
                    ModalButtonKind::Primary,
                    busy.then(|| SharedString::from(Msg::RemoteRequestBusy.t())),
                    open_repo,
                    cx,
                )));
                body = body.child(repo_card.flex_shrink_0());
            }

            if let Some(ref err) = modal.error {
                body = body.child(diagnostic(err.clone()));
            }

            card = card
                .child(body)
                .child(render_directory_list(
                    &modal,
                    focus_handle.as_ref(),
                    window,
                    cx,
                ))
                .child(
                    super::button_style::modal_button_row()
                        .flex_shrink_0()
                        .child(super::e2e::measure_control(
                            "remote-browse-close",
                            modal_button(
                                "remote-browse-close",
                                Msg::RemoteClose.t(),
                                ModalButtonKind::Cancel,
                                None,
                                cancel,
                                cx,
                            ),
                        )),
                );
        }
    }

    // Escape cancels (mirrors the other input modals).
    let esc_cancel = cx.listener(|this, e: &KeyDownEvent, window, cx| {
        if e.keystroke.key == "escape" {
            this.cancel_remote_browse();
            if let Some(fh) = this.root_focus.clone() {
                window.focus(&fh, cx);
            }
            cx.stop_propagation();
            cx.notify();
        }
    });
    let focusable_card = super::modal_renderers::input_dialog_frame(
        "remote-browse-dialog",
        &dialog_title,
        dialog_confirm,
        div().relative().on_key_down(esc_cancel).child(card),
        focus_handle.as_ref(),
        |frame| frame,
        cx,
    );

    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        // Measured from inside: a wrapper around an absolute overlay would
        // itself be laid out in flow and move the overlay with it.
        .child(super::e2e::measure_inside("active-modal/remote-browse"))
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .occlude()
                .bg(rgb(current_theme().modal_overlay))
                .opacity(0.65),
        )
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .child(focusable_card),
        )
}

/// Dense directory browser: RowList owns the single Tab stop, reveal and
/// keyboard ring; list_a11y owns role/name/selection. Files remain meaningful
/// selectable rows but never navigate. Busy keeps the accepted directory and
/// its selection visible and makes activation inert; failures retain that
/// same owner. Only an accepted, generation-guarded read requests row focus.
fn render_directory_list(
    modal: &RemoteBrowseModal,
    fallback: Option<&FocusHandle>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let scroll = RowScroll::List(modal.row_scroll.clone());
    let rows = Rc::new(modal.row_focus.borrow_mut().rows(
        modal.row_keys.clone(),
        &scroll,
        fallback,
        window,
        cx,
    ));
    if modal.focus_accepted_read.replace(false)
        && !modal.row_focus.borrow().focus_first(&scroll, window, cx)
    {
        if let Some(fallback) = fallback {
            fallback.focus(window, cx);
        }
    }
    if let Some(key) = modal.row_focus.borrow().focused(window) {
        modal
            .selected_row
            .set(modal.row_keys.iter().position(|(k, _)| k == key));
    }
    let selected = modal.selected_row.get();
    let wrapper = rows.list(super::list_a11y::list_box(
        "remote-dir-list",
        div()
            .id("remote-dir-list")
            .w_full()
            .min_h(theme::scaled_px(40.))
            .flex()
            .flex_col(),
        Msg::RemoteDirectoryList.t(),
    ));
    let empty_notice = || {
        let text = Msg::RemoteDirectoryEmpty.t();
        super::e2e::measure_control(
            "remote-dir-empty",
            super::dialog_a11y::apply_note(
                "remote-dir-empty".into(),
                div()
                    .id("remote-dir-empty")
                    .text_sm()
                    .text_color(rgb(current_theme().text_muted))
                    .child(SharedString::from(text)),
                false,
                text,
            ),
        )
    };
    if modal.row_keys.is_empty() {
        return wrapper.child(empty_notice()).into_any_element();
    }
    let entries = modal.entries.clone();
    let cwd = modal.cwd.clone();
    let parent = remote::parent_dir(&cwd);
    let busy = modal.busy;
    let generation = modal.generation;
    let app = cx.entity();
    let row_keys = modal.row_keys.clone();
    let size = row_keys.len();
    let row_fill = if current_theme().slug == "apple-dark" {
        current_theme().selected
    } else {
        current_theme().surface
    };
    let list = gpui::list(modal.row_scroll.clone(), move |ix, window, _cx| {
        let window_active = window.is_window_active();
        let (label, text, target) = if ix == 0 && parent.is_some() {
            (
                Msg::RemoteParentDirectory.t().to_string(),
                "\u{2191}  ..".to_string(),
                parent.clone(),
            )
        } else {
            let entry = &entries[ix - usize::from(parent.is_some())];
            if entry.is_dir() {
                (
                    Msg::RemoteDirectoryRow.t().replacen("{}", &entry.name, 1),
                    format!("\u{1f4c1}  {}/", entry.name),
                    Some(remote::join_path(&cwd, &entry.name)),
                )
            } else {
                (
                    Msg::RemoteFileRow.t().replacen("{}", &entry.name, 1),
                    format!("\u{1f4c4}  {}", entry.name),
                    None,
                )
            }
        };
        let navigable = target.is_some();
        let app = app.clone();
        let click_rows = rows.clone();
        let row = rows
            .row(
                ix,
                super::list_a11y::list_option(
                    "remote-dir-list",
                    div().id(SharedString::from(row_keys[ix].0.clone())),
                    ix,
                    size,
                    label,
                    selected == Some(ix),
                ),
            )
            .w_full()
            .px(keyboard_nav::inset(8.))
            .py(keyboard_nav::inset(4.))
            .rounded_sm()
            .text_sm()
            .overflow_hidden()
            .text_color(
                if current_theme().selection_uses_on_accent(selected == Some(ix), window_active) {
                    current_theme().selection_foreground(window_active)
                } else {
                    rgb(if navigable {
                        current_theme().text_main
                    } else {
                        current_theme().text_muted
                    })
                    .into()
                },
            )
            .when(selected == Some(ix), |row| {
                row.bg(current_theme().selected_for_window(window_active))
            })
            .when(busy, |row| {
                row.aria_description(SharedString::from(Msg::RemoteRequestBusy.t()))
                    .a11y_synthetic_children(|builder: &mut gpui::A11ySubtreeBuilder| {
                        builder.parent_node().set_disabled();
                    })
            })
            .when(!busy && navigable, |row| {
                row.cursor_pointer().when(selected != Some(ix), |row| {
                    row.hover(move |style| style.bg(rgb(row_fill)))
                })
            })
            .on_click(move |_, window, cx| {
                app.update(cx, |app, cx| {
                    if app.remote_browse_generation_is(generation) {
                        click_rows.focus_row(ix, window, cx);
                        if let Some(target) = &target {
                            app.remote_browse_navigate(target.clone(), cx);
                        }
                        cx.notify();
                    }
                });
            })
            .child(SharedString::from(text));
        #[cfg(feature = "gui-e2e")]
        let row = super::e2e::measure_control(format!("remote-dir-row-{ix}"), row);
        row.into_any_element()
    })
    .w_full()
    .min_h(theme::scaled_px(40.))
    .h(theme::scaled_px((size as f32 * 32.).min(280.)));
    wrapper
        .child(list)
        .when(modal.entries.is_empty(), |wrapper| {
            wrapper.child(empty_notice())
        })
        .into_any_element()
}

/// Keep the complete transport identity; ellipsis is native paint only.
fn identity(id: &'static str, full: SharedString) -> gpui::Stateful<gpui::Div> {
    let tooltip = full.clone();
    div()
        .id(id)
        .role(gpui::Role::Label)
        .aria_label(full.clone())
        .aria_description(full.clone())
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
        })
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis_middle()
        .relative()
        .child(super::e2e::measure_inside(id))
        .child(full)
}

fn diagnostic(error: SharedString) -> gpui::Div {
    div()
        .flex_shrink_0()
        .text_sm()
        .text_color(rgb(current_theme().color_blocker))
        .child(error)
        .child(super::e2e::measure_control(
            "remote-browse-error-tail",
            div().h(theme::scaled_px(1.)),
        ))
}
