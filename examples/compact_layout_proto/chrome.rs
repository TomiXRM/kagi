use super::*;

impl Prototype {
    pub(super) fn header(&self, compact: bool) -> AnyElement {
        div()
            .h(scaled_px(46.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .px_4()
            .gap_4()
            .bg(rgb(theme().panel))
            .border_b_1()
            .border_color(rgb(theme().surface))
            .child(icon("git-branch", theme().ui_accent()))
            .child(div().font_semibold().child("Kagi"))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .child("kagi · dev"),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .child(if compact {
                        "Compact · A″ prototype"
                    } else {
                        "Wide · A″ prototype"
                    }),
            )
            .into_any_element()
    }

    pub(super) fn workspace(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(if compact {
                self.rail(cx)
            } else {
                self.wide_sidebar(cx)
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .child(self.graph(compact, cx))
                            .when(!compact, |el| el.child(self.wide_commit_pane(cx))),
                    )
                    .child(self.bottom(compact, cx)),
            )
            .into_any_element()
    }

    fn wide_commit_pane(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w(scaled_px(340.))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(rgb(theme().surface))
            .child(self.files_column(false, true, cx))
            .child(self.files_column(true, true, cx))
            .child(
                div()
                    .id("wide-composer-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.composer(cx)),
            )
            .into_any_element()
    }
}
