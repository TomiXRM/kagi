//! Interactive, fixture-only A″ layout study. No repositories or Git operations.
//! cargo run --example compact_layout_proto [-- --dark]
use std::{borrow::Cow, cell::Cell, rc::Rc};

use gpui::{prelude::*, *};
use gpui_component::{
    button::{Button, ButtonVariants},
    input::{Input, InputState},
    popover::{Popover, PopoverState},
    ElementExt, Icon, Root, Sizable, StyledExt,
};
use kagi::ui::{
    assets::KagiAssets,
    keyboard_nav::{self, Activation, TabFocus, TabList},
    settings_view::{section_header, setting_row},
};
use kagi_ui_core::theme::{self, scaled_px, theme, UI_FONT};
#[path = "compact_layout_proto/chrome.rs"]
mod chrome;
#[path = "compact_layout_proto/fixture.rs"]
mod fixture;
use fixture::{branches, SectionPicker};

const COMPACT_WIDTH: f32 = 1000.;
const SECTIONS: [(&str, &str); 7] = [
    ("Local branches", "git-branch"),
    ("Remote", "cloud"),
    ("Worktrees", "tree-pine"),
    ("Tags", "tag"),
    ("Stashes", "inbox"),
    ("PRs", "git-pull-request"),
    ("Issues", "circle-dot"),
];
const FILES: [&str; 2] = ["src/ui/render_wip.rs", "src/ui/compact.rs"];
const MESSAGES: [&str; 6] = [
    "Keep the commit draft across layout changes",
    "Use system accent for selection",
    "Refresh repository status after staging",
    "Add keyboard navigation to bottom tabs",
    "Polish sidebar section headings",
    "Preserve graph selection on refresh",
];

struct Prototype {
    focus: FocusHandle,
    summary: Entity<InputState>,
    body: Entity<InputState>,
    pickers: Vec<Entity<SectionPicker>>,
    tabs: TabFocus,
    active_tab: usize,
    popup: bool,
    wip_hover: bool,
    wip_bounds: Rc<Cell<Bounds<Pixels>>>,
    graph_scroll: ScrollHandle,
    selected_commit: Option<usize>,
    selected_ref: SharedString,
    staged: [bool; 2],
    tree: [bool; 2],
    status: SharedString,
}

impl Prototype {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let parent = cx.weak_entity();
        let pickers = (0..SECTIONS.len())
            .map(|section| cx.new(|cx| SectionPicker::new(section, parent.clone(), window, cx)))
            .collect();
        Self {
            focus: cx.focus_handle(),
            summary: cx.new(|cx| InputState::new(window, cx).placeholder("Summary")),
            body: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .rows(5)
                    .placeholder("Description (optional)")
            }),
            pickers,
            tabs: TabFocus::default(),
            active_tab: 0,
            popup: false,
            wip_hover: false,
            wip_bounds: Rc::new(Cell::new(Bounds::default())),
            graph_scroll: ScrollHandle::new(),
            selected_commit: None,
            selected_ref: "dev".into(),
            staged: [true, true],
            tree: [false, false],
            status: "Fixture · no Git operations".into(),
        }
    }

    fn open_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.popup = true;
        self.summary.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn composer(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .h_full()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .bg(rgb(theme().modal))
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().font_semibold().child("Commit message"))
                    .child(
                        Button::new("close-composer")
                            .ghost()
                            .label("×")
                            .tooltip("Close · draft kept")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.popup = false;
                                this.focus.focus(window, cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(div().flex_shrink_0().child(Input::new(&self.summary)))
            .child(
                div()
                    .flex_1()
                    .min_h(scaled_px(45.))
                    .child(Input::new(&self.body).h_full()),
            )
            .child(
                div().flex_shrink_0().child(setting_row(
                    "Staged changes".into(),
                    format!(
                        "{} files · shared draft",
                        self.staged.iter().filter(|s| **s).count()
                    )
                    .into(),
                    div()
                        .text_color(rgb(theme().text_sub))
                        .child("+24 −8")
                        .into_any_element(),
                )),
            )
            .child(
                div().flex_shrink_0().flex().justify_end().child(
                    Button::new("commit-fixture")
                        .primary()
                        .label("Commit")
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.summary.read(cx).value().trim().is_empty() {
                                this.status = "Enter a Summary to simulate a Commit".into();
                            } else {
                                this.popup = false;
                                this.status =
                                    "Commit simulated · draft retained · no repository changed"
                                        .into();
                                this.focus.focus(window, cx);
                            }
                            cx.notify();
                        })),
                ),
            )
            .into_any_element()
    }

    fn rail(&self, _cx: &mut Context<Self>) -> AnyElement {
        let mut rail = div()
            .w(scaled_px(44.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .py_2()
            .gap_2()
            .bg(rgb(theme().sidebar))
            .border_r_1()
            .border_color(rgb(theme().surface));
        for (i, (label, icon)) in SECTIONS.iter().enumerate() {
            if i == 5 {
                rail = rail.child(div().flex_1());
            }
            let picker = self.pickers[i].clone();
            rail = rail.child(
                Popover::new(("rail-popover", i))
                    .anchor(Anchor::TopLeft)
                    .trigger(
                        Button::new(("rail-icon", i))
                            .ghost()
                            .icon(Icon::default().path(format!("icons/{icon}.svg")))
                            .tooltip(*label)
                            .w(scaled_px(36.))
                            .h(scaled_px(36.)),
                    )
                    .content(move |_, _, cx| {
                        let popover = cx.weak_entity();
                        picker.update(cx, |picker, _| picker.popover = Some(popover));
                        picker.clone()
                    }),
            );
        }
        rail.into_any_element()
    }

    fn wide_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let filter = self.pickers[0].read(cx).filter.clone();
        let query = filter.read(cx).value().to_lowercase();
        div()
            .w(scaled_px(230.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(rgb(theme().sidebar))
            .border_r_1()
            .border_color(rgb(theme().surface))
            .child(div().p_3().child(Input::new(&filter).small()))
            .child(
                div()
                    .id("wide-sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(div().px_3().child(section_header(
                        "proto-wide-local",
                        "LOCAL BRANCHES · 121".into(),
                    )))
                    .children(
                        branches()
                            .into_iter()
                            .filter(|name| name.to_lowercase().contains(&query))
                            .enumerate()
                            .map(|(i, name)| {
                                let selected = self.selected_ref == name;
                                div()
                                    .id(("wide-branch", i))
                                    .px_3()
                                    .h(scaled_px(23.))
                                    .flex()
                                    .items_center()
                                    .text_xs()
                                    .cursor_pointer()
                                    .when(selected, |el| el.bg(rgb(theme().selected)))
                                    .hover(|el| el.bg(rgb(theme().surface)))
                                    .child(name.clone())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.selected_ref = name.clone();
                                        cx.notify();
                                    }))
                            }),
                    ),
            )
            .into_any_element()
    }

    fn graph(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(scaled_px(34.))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .border_b_1()
                    .border_color(rgb(theme().surface))
                    .child("Graph")
                    .child(
                        div()
                            .ml_3()
                            .text_color(rgb(theme().color_branch))
                            .child(self.selected_ref.clone()),
                    ),
            )
            .child(
                div()
                    .id("graph-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.graph_scroll)
                    .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                        if this.popup {
                            this.popup = false;
                            cx.notify();
                        }
                    }))
                    .child(self.wip_row(compact, cx))
                    .children((0..30).map(|i| self.commit_row(i, cx))),
            )
            .into_any_element()
    }

    fn wip_row(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let bounds = self.wip_bounds.clone();
        let staged = self.staged.iter().filter(|s| **s).count();
        div()
            .id("wip-row")
            .flex()
            .items_center()
            .gap_3()
            .h(scaled_px(36.))
            .px_3()
            .bg(rgb(theme().surface))
            .cursor_pointer()
            .on_prepaint(move |b, _, _| bounds.set(b))
            .on_hover(cx.listener(|this, hovered, _, cx| {
                this.wip_hover = *hovered;
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.selected_commit = None;
                if compact {
                    this.open_composer(window, cx);
                } else {
                    cx.notify();
                }
            }))
            .child(icon("circle-dot", theme().color_warning))
            .child(div().font_semibold().text_xs().child("Working changes"))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .child(format!(
                        "{} unstaged · {staged} staged",
                        FILES.len() - staged
                    )),
            )
            .child(div().flex_1())
            .when(compact && self.wip_hover, |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().ui_accent()))
                        .child("Commit…  ⌘↵"),
                )
            })
            .into_any_element()
    }

    fn commit_row(&self, i: usize, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id(("commit-row", i))
            .flex()
            .items_center()
            .gap_3()
            .h(scaled_px(29.))
            .flex_shrink_0()
            .px_3()
            .text_xs()
            .cursor_pointer()
            .when(self.selected_commit == Some(i), |el| {
                el.bg(rgb(theme().selected))
            })
            .hover(|el| el.bg(rgb(theme().surface)))
            .child(
                div()
                    .w(scaled_px(38.))
                    .h_full()
                    .relative()
                    .child(
                        div()
                            .absolute()
                            .left(scaled_px(9.))
                            .h_full()
                            .w(px(2.))
                            .bg(theme().lane_color(i % 3)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(scaled_px(5.))
                            .top(scaled_px(10.))
                            .size(scaled_px(10.))
                            .rounded_full()
                            .border_2()
                            .border_color(theme().lane_color(i % 3))
                            .bg(rgb(theme().bg_base)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(MESSAGES[i % MESSAGES.len()]),
            )
            .child(
                div()
                    .text_color(rgb(theme().text_sub))
                    .w(scaled_px(76.))
                    .child("TomiXRM"),
            )
            .child(
                div()
                    .text_color(rgb(theme().text_sub))
                    .w(scaled_px(40.))
                    .child(format!("{}h", i + 1)),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected_commit = Some(i);
                cx.notify();
            }))
            .into_any_element()
    }

    fn file_list(&self, staged: bool, cx: &mut Context<Self>) -> AnyElement {
        let column = usize::from(staged);
        let count = self.staged.iter().filter(|s| **s == staged).count();
        let mut list = div()
            .id(("files-scroll", column))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        for (i, path) in FILES
            .iter()
            .enumerate()
            .filter(|(i, _)| self.staged[*i] == staged)
        {
            let selected = self.selected_commit == Some(100 + i);
            list =
                list.child(
                    div()
                        .id(("file-row", i))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .h(scaled_px(29.))
                        .text_xs()
                        .cursor_pointer()
                        .when(selected, |el| el.bg(rgb(theme().selected)))
                        .hover(|el| el.bg(rgb(theme().surface)))
                        .child(div().text_color(rgb(theme().color_branch)).child("M"))
                        .child(div().flex_1().min_w_0().overflow_hidden().child(
                            if self.tree[column] {
                                path.replace("src/ui/", "▾ src / ui /  ")
                            } else {
                                path.to_string()
                            },
                        ))
                        .child(div().text_color(rgb(theme().change_added)).child("+12"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.selected_commit = Some(100 + i);
                            cx.notify();
                        })),
                );
        }
        if count == 0 {
            list = list.child(
                div()
                    .p_4()
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .child("No changes"),
            );
        }
        list.into_any_element()
    }

    fn files_column(&self, staged: bool, vertical: bool, cx: &mut Context<Self>) -> AnyElement {
        let column = usize::from(staged);
        let count = self.staged.iter().filter(|s| **s == staged).count();
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .when(vertical, |el| el.h(scaled_px(180.)).flex_shrink_0())
            .child(
                div()
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(theme().surface))
                    .child(div().text_xs().font_semibold().flex_1().child(format!(
                        "{} ({count})",
                        if staged { "Staged" } else { "Unstaged" }
                    )))
                    .child(
                        Button::new(("list-tree", column))
                            .ghost()
                            .small()
                            .label(if self.tree[column] { "Tree" } else { "List" })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.tree[column] = !this.tree[column];
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(("stage-all", column))
                            .ghost()
                            .small()
                            .label(if staged { "Unstage all" } else { "Stage all" })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.staged.fill(!staged);
                                cx.notify();
                            })),
                    ),
            )
            .child(self.file_list(staged, cx))
            .into_any_element()
    }

    fn bottom_tabs(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let labels = ["Changes", "Operation Log", "Terminal", "Activity"];
        let weak = cx.weak_entity();
        let tabs = TabList::new(
            &self.tabs,
            keyboard_nav::slot_keys(4),
            if compact {
                vec![0, 1, 2, 3]
            } else {
                vec![1, 2, 3]
            },
            Some(if compact {
                self.active_tab
            } else {
                self.active_tab.max(1)
            }),
            Activation::Automatic,
            Some(self.focus.clone()),
            move |slot, _, _, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.active_tab = slot;
                    cx.notify();
                });
            },
            cx,
        );
        let mut strip = tabs.list(div().id("bottom-tabs").flex().gap_2().px_3().py_2());
        for (i, label) in labels
            .iter()
            .enumerate()
            .filter(|(i, _)| compact || *i != 0)
        {
            strip = strip.child(
                tabs.cell(
                    i,
                    label,
                    div()
                        .id(("bottom-tab", i))
                        .text_xs()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .when(
                            i == if compact {
                                self.active_tab
                            } else {
                                self.active_tab.max(1)
                            },
                            |el| el.bg(rgb(theme().surface)),
                        )
                        .child(*label),
                ),
            );
        }
        strip.into_any_element()
    }

    fn bottom(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        div()
            .h(scaled_px(if compact { 260. } else { 140. }))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(rgb(theme().surface))
            .bg(rgb(theme().panel))
            .child(self.bottom_tabs(compact, cx))
            .child(if compact && self.active_tab == 0 {
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.files_column(false, false, cx))
                    .child(div().w(px(1.)).h_full().bg(rgb(theme().surface)))
                    .child(self.files_column(true, false, cx))
                    .into_any_element()
            } else {
                div()
                    .p_3()
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .child(match self.active_tab.max(1) {
                        1 => "✓ Repository refreshed     ✓ 2 files staged",
                        2 => "$ git status\nOn branch dev\nChanges to be committed: 2 files",
                        _ => "Activity  ▂▃▆▅▂▇▃▅▇▂▃▆▅▂▇▃▅▇",
                    })
                    .into_any_element()
            })
            .into_any_element()
    }

    fn popup(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let b = self.wip_bounds.get();
        let viewport = window.viewport_size();
        let height = scaled_px(420.).min(viewport.height - px(16.));
        let width = scaled_px(480.).min(viewport.width - px(24.));
        let below = b.bottom() + px(6.);
        let y = if below + height <= viewport.height - px(8.) {
            below
        } else {
            (b.top() - height - px(6.)).max(px(8.))
        };
        deferred(
            anchored()
                .position(point(b.left() + scaled_px(12.), y))
                .anchor(Anchor::TopLeft)
                .snap_to_window_with_margin(px(8.))
                .child(
                    div()
                        .id("pinned-composer")
                        .w(width)
                        .h(height)
                        .occlude()
                        .tab_group()
                        .bg(rgb(theme().modal))
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(theme().surface))
                        .shadow_lg()
                        .overflow_hidden()
                        .child(self.composer(cx)),
                ),
        )
        .with_priority(2)
        .into_any_element()
    }
}

impl Render for Prototype {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(px(theme::rem_size_px()));
        keyboard_nav::forget_roving_without_focus(window);
        let compact = window.viewport_size().width < px(COMPACT_WIDTH);
        if !compact {
            for picker in &self.pickers {
                let popover = picker.read(cx).popover.clone();
                if let Some(popover) = popover {
                    let _ = popover.update(cx, |p, cx| p.dismiss(window, cx));
                }
            }
        }
        div()
            .id("prototype")
            .size_full()
            .flex()
            .flex_col()
            .font_family(UI_FONT)
            .text_sm()
            .text_color(rgb(theme().text_main))
            .bg(rgb(theme().bg_base))
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && this.popup {
                    this.popup = false;
                    this.focus.focus(window, cx);
                    cx.stop_propagation();
                    cx.notify();
                } else if event.keystroke.key == "enter" && event.keystroke.modifiers.platform {
                    this.open_composer(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(self.header(compact))
            .child(self.workspace(compact, cx))
            .child(
                div()
                    .h(scaled_px(24.))
                    .px_3()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .border_t_1()
                    .border_color(rgb(theme().surface))
                    .child(self.status.clone()),
            )
            .when(compact && self.popup, |el| el.child(self.popup(window, cx)))
    }
}

fn icon(name: &str, color: u32) -> impl IntoElement {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(scaled_px(16.))
        .text_color(rgb(color))
}

fn main() {
    fixture::run();
}
