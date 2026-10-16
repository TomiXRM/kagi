//! Activity chart/legend stroke encoding, including coincident and single-point series.
use super::theme::{self, theme};
use gpui::{canvas, point, px, Bounds, Canvas, PathBuilder, Pixels, Point, Window};
use kagi_domain::activity::ActivityBucket;

/// Padded linear x scale and its midpoint inspection cells.
pub(super) struct PlotScale {
    pub pad: Pixels,
    count: usize,
}
impl PlotScale {
    pub fn new(count: usize) -> Self {
        Self {
            pad: theme::scaled_px(10.),
            count,
        }
    }
    pub fn x(&self, width: Pixels, index: usize) -> Pixels {
        if self.count <= 1 {
            width / 2.
        } else {
            self.pad + (width - self.pad * 2.) * (index as f32 / (self.count - 1) as f32)
        }
    }
    /// End cells own padding plus half an interval; interior cells one interval.
    pub fn cell(&self, index: usize) -> (Pixels, f32) {
        if self.count <= 1 {
            (px(0.), 1.)
        } else if index == 0 || index + 1 == self.count {
            (self.pad, 0.5)
        } else {
            (px(0.), 1.)
        }
    }
}

pub(super) fn activity_chart(buckets: Vec<ActivityBucket>) -> Canvas<()> {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, (), window, _| {
            if buckets.is_empty() {
                return;
            }
            let scale = PlotScale::new(buckets.len());
            let pad = scale.pad;
            let left = bounds.left() + pad;
            let right = bounds.right() - pad;
            let top = bounds.top() + pad;
            let bottom = bounds.bottom() - pad;
            let max = buckets.iter().map(|b| b.commits).max().unwrap_or(1).max(1) as f32;
            let position = |i: usize, value: u32| {
                let x = bounds.left() + scale.x(bounds.size.width, i);
                point(x, bottom - (bottom - top) * (value as f32 / max))
            };
            let mut base = PathBuilder::stroke(theme::scaled_px(1.));
            base.move_to(point(left, bottom));
            base.line_to(point(right, bottom));
            if let Ok(path) = base.build() {
                window.paint_path(path, gpui::rgb(theme().text_muted));
            }
            for merges in [false, true] {
                let points = buckets.iter().enumerate().map(|(i, b)| {
                    let point = position(i, if merges { b.merges } else { b.commits });
                    #[cfg(feature = "gui-e2e")]
                    super::e2e::activity::record_point(i, merges, point);
                    point
                });
                paint_series(points, merges, window, "chart");
            }
        },
    )
}

/// One path for the ink and, only for merges, one path knocking out dash gaps.
/// Wider panel-coloured gaps remove underlying commits/baseline even at equal values.
struct SeriesStroke {
    ink: PathBuilder,
    knockout: Option<PathBuilder>,
    phase: f32,
    dash: f32,
    period: f32,
    #[cfg(feature = "gui-e2e")]
    solid_segments: usize,
    #[cfg(feature = "gui-e2e")]
    dash_segments: usize,
    #[cfg(feature = "gui-e2e")]
    gap_segments: usize,
}
impl SeriesStroke {
    fn new(merges: bool) -> Self {
        Self {
            ink: PathBuilder::stroke(theme::scaled_px(if merges { 1.5 } else { 1.8 })),
            knockout: merges.then(|| PathBuilder::stroke(theme::scaled_px(3.8))),
            phase: 0.,
            dash: if merges { theme::scaled(5.) } else { 0. },
            period: if merges { theme::scaled(9.) } else { 0. },
            #[cfg(feature = "gui-e2e")]
            solid_segments: 0,
            #[cfg(feature = "gui-e2e")]
            dash_segments: 0,
            #[cfg(feature = "gui-e2e")]
            gap_segments: 0,
        }
    }
    fn segment(&mut self, from: Point<Pixels>, to: Point<Pixels>) {
        let Some(gaps) = &mut self.knockout else {
            self.ink.line_to(to);
            #[cfg(feature = "gui-e2e")]
            {
                self.solid_segments += 1;
            }
            return;
        };
        let dx = f32::from(to.x - from.x);
        let dy = f32::from(to.y - from.y);
        let length = dx.hypot(dy);
        let dash = self.dash;
        let period = self.period;
        let mut offset = 0.;
        // Ignore float rounding's subpixel remainder, not an extra near-zero dash/gap.
        while offset + 0.001 < length {
            let draw = self.phase < dash;
            let span = if draw {
                dash - self.phase
            } else {
                period - self.phase
            };
            let end = (offset + span.max(0.01)).min(length);
            let at = |distance: f32| {
                from + point(px(dx * distance / length), px(dy * distance / length))
            };
            if draw {
                self.ink.move_to(at(offset));
                self.ink.line_to(at(end));
                #[cfg(feature = "gui-e2e")]
                {
                    self.dash_segments += 1;
                }
            } else {
                gaps.move_to(at(offset));
                gaps.line_to(at(end));
                #[cfg(feature = "gui-e2e")]
                {
                    self.gap_segments += 1;
                }
            }
            self.phase = (self.phase + end - offset) % period;
            offset = end;
        }
    }
    fn paint(mut self, merges: bool, _single_point: bool, window: &mut Window, _probe: &str) {
        #[cfg(feature = "gui-e2e")]
        let mut knockout_segments = 0;
        if let Some(gaps) = self.knockout.take() {
            if let Ok(path) = gaps.build() {
                window.paint_path(path, gpui::rgb(theme().panel));
                #[cfg(feature = "gui-e2e")]
                {
                    knockout_segments = self.gap_segments;
                }
            }
        }
        if let Ok(path) = self.ink.build() {
            window.paint_path(
                path,
                gpui::rgb(if merges {
                    theme().color_head
                } else {
                    theme().color_success
                }),
            );
            #[cfg(feature = "gui-e2e")]
            super::e2e::activity::stroke(
                &format!("{_probe}-{}", if merges { "merges" } else { "commits" }),
                super::e2e::activity::Stroke {
                    solid_segments: self.solid_segments,
                    dash_segments: self.dash_segments,
                    gap_segments: self.gap_segments,
                    knockout_segments,
                    single_point: _single_point,
                },
            );
        }
    }
}

/// Shared by chart and legend. A single merge bucket has two real dashes and
/// a gap, below the commit mark so both encodings remain individually visible.
pub(super) fn paint_series(
    points: impl IntoIterator<Item = Point<Pixels>>,
    merges: bool,
    window: &mut Window,
    probe: &str,
) {
    let mut points = points.into_iter().peekable();
    let Some(mut from) = points.next() else {
        return;
    };
    let single_point = points.peek().is_none();
    let mut stroke = SeriesStroke::new(merges);
    if single_point {
        let center = from + point(px(0.), if merges { theme::scaled_px(3.) } else { px(0.) });
        let half = theme::scaled_px(if merges { 7. } else { 2. });
        let start = center - point(half, px(0.));
        stroke.ink.move_to(start);
        stroke.segment(start, center + point(half, px(0.)));
    } else {
        stroke.ink.move_to(from);
        for to in points {
            stroke.segment(from, to);
            from = to;
        }
    }
    stroke.paint(merges, single_point, window, probe);
}
