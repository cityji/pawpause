//! Small, focused `canvas::Program` chart widgets for the Statistics page.
//! Each is generic over `Message` (none is ever emitted — hover state lives
//! entirely inside the canvas's own `Program::State`), so none of this
//! requires new `Message` variants on `App`.
//!
//! Colors (including text color) are resolved by the caller from
//! `cosmic::theme::active()` and passed in as plain fields — the canvas
//! `Program` trait's `Theme` type param defaults to vanilla `iced::Theme`,
//! not `cosmic::Theme`, so cosmic's `.cosmic()` palette accessors aren't
//! reachable from inside `draw()`.

use chrono::{Datelike, NaiveDate};
use cosmic::iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use cosmic::iced::{mouse, Color, Point, Radians, Rectangle, Size};

const PADDING: f32 = 10.0;

/// Ease-out cubic. Fast start, gentle settle — the standard "it arrived"
/// curve for progress reveals, and the reason the ring doesn't feel linear
/// and mechanical.
fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn label_text(content: String, position: Point, size: f32, color: Color) -> Text {
    Text {
        content,
        position,
        size: size.into(),
        color,
        ..Text::default()
    }
}

fn muted(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

/// Like `stats::format_hhmm`, but shows whole seconds below one minute
/// ("32s") instead of a misleading "00:00" — matters most on a bar chart,
/// where a sub-minute value can still be the only (100%-width) bar shown.
fn format_short(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        pawpause_applet::stats::format_hhmm(seconds)
    }
}

/// Sorted-descending category bars, e.g. `stats::week_breakdown()`'s output
/// fed straight in. Caps rendered bars at `max_bars` so a long project list
/// can't blow out a card-sized layout.
///
/// `reference` is the value a full-width bar represents. Normalizing against
/// the largest *bar* (the old behavior) meant a single category always
/// rendered 100% full — 34 minutes of work drawn as a maxed-out bar, which is
/// what made this chart read as broken. Pass a meaningful ceiling (a goal, a
/// personal best); `None` falls back to the in-chart max, which is only
/// honest when there are several bars to compare against each other.
pub struct BarChart {
    pub bars: Vec<(String, u64)>,
    pub color: Color,
    pub text_color: Color,
    pub max_bars: usize,
    pub reference: Option<u64>,
}

impl<Message> canvas::Program<Message, cosmic::Theme> for BarChart {
    type State = Option<usize>;

    fn update(
        &self,
        state: &mut Self::State,
        _event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let rows = self.bars.len().min(self.max_bars);
        let row_height = if rows == 0 { 0.0 } else { (bounds.height - PADDING * 2.0) / rows as f32 };
        let hovered = cursor.position_in(bounds).and_then(|p| {
            if row_height <= 0.0 {
                return None;
            }
            let index = ((p.y - PADDING) / row_height).floor() as isize;
            (index >= 0 && (index as usize) < rows).then_some(index as usize)
        });
        (hovered != *state).then(|| {
            *state = hovered;
            canvas::Action::request_redraw()
        })
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if self.bars.is_empty() {
            frame.fill_text(label_text(
                "No data yet".to_string(),
                Point::new(PADDING, PADDING),
                12.0,
                muted(self.text_color, 0.6),
            ));
            return vec![frame.into_geometry()];
        }

        let rows = self.bars.len().min(self.max_bars);
        let row_height = (bounds.height - PADDING * 2.0) / rows as f32;
        let in_chart_max = self.bars.iter().take(rows).map(|(_, v)| *v).max().unwrap_or(1);
        // Never let the reference fall below the largest bar, or that bar
        // would overflow its track.
        let max_value = self.reference.unwrap_or(in_chart_max).max(in_chart_max).max(1);
        let label_col = 90.0_f32.min(bounds.width * 0.3);
        let value_col = 60.0_f32.min(bounds.width * 0.2);
        let bar_area = (bounds.width - PADDING * 2.0 - label_col - value_col).max(1.0);

        for (i, (label, value)) in self.bars.iter().take(rows).enumerate() {
            let y = PADDING + i as f32 * row_height;
            let bar_h = (row_height - 6.0).max(2.0);
            let bar_y = y + (row_height - bar_h) / 2.0;

            if *state == Some(i) {
                frame.fill_rectangle(
                    Point::new(0.0, y),
                    Size::new(bounds.width, row_height),
                    muted(self.color, 0.08),
                );
            }

            let truncated: String = label.chars().take(14).collect();
            frame.fill_text(label_text(
                truncated,
                Point::new(PADDING, y + row_height / 2.0 - 6.0),
                12.0,
                self.text_color,
            ));

            // Empty track first, so a short bar reads as "progress along a
            // scale" instead of a lonely stub floating in whitespace.
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(PADDING + label_col, bar_y),
                    Size::new(bar_area, bar_h),
                    (bar_h / 2.0).into(),
                ),
                muted(self.text_color, 0.07),
            );

            let ratio = *value as f32 / max_value as f32;
            let bar_w = (bar_area * ratio).max(2.0);
            let bar_path = Path::rounded_rectangle(
                Point::new(PADDING + label_col, bar_y),
                Size::new(bar_w, bar_h),
                (bar_h / 2.0).into(),
            );
            frame.fill(&bar_path, self.color);

            frame.fill_text(label_text(
                format_short(*value),
                Point::new(PADDING + label_col + bar_area + 4.0, y + row_height / 2.0 - 6.0),
                11.0,
                muted(self.text_color, 0.8),
            ));
        }

        vec![frame.into_geometry()]
    }
}

/// The hero metric: today's focused time drawn as a progress ring against the
/// daily goal, with the headline figure inside it.
///
/// `anim` is 0.0..=1.0 elapsed progress of the reveal, supplied by the caller
/// from a timer subscription (canvas has no clock of its own). The sweep eases
/// out to its true value; once complete the widget is static, so an idle
/// Statistics page settles rather than animating forever.
///
/// When the goal is met the ring completes and gains a soft outer glow — the
/// one celebratory flourish on the page, held to a single element per the
/// "animate 1-2 things, not everything" rule.
pub struct GoalRing {
    pub today_seconds: u64,
    /// What a full sweep represents. Callers substitute a fallback (personal
    /// best, or a default) when the user's goal is switched off, so the ring
    /// is never a meaningless empty circle.
    pub goal_seconds: u64,
    /// Whether filling the ring is a genuine achievement. False when
    /// `goal_seconds` is a stand-in, so the celebratory glow is reserved for
    /// a goal the user actually set.
    pub celebrate: bool,
    pub headline: String,
    pub caption: String,
    pub color: Color,
    pub accent_done: Color,
    pub text_color: Color,
    pub anim: f32,
}

impl<Message> canvas::Program<Message, cosmic::Theme> for GoalRing {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let radius = (bounds.width.min(bounds.height) / 2.0 - PADDING).max(8.0);
        let thickness = (radius * 0.16).clamp(5.0, 14.0);

        let target = if self.goal_seconds == 0 {
            0.0
        } else {
            (self.today_seconds as f32 / self.goal_seconds as f32).clamp(0.0, 1.0)
        };
        let progress = target * ease_out(self.anim);
        let done = self.celebrate && self.goal_seconds > 0 && self.today_seconds >= self.goal_seconds;
        let ring_color = if done { self.accent_done } else { self.color };

        // Glow first, so the ring sits on top of it.
        if done {
            frame.stroke(
                &Path::circle(center, radius),
                Stroke::default()
                    .with_color(muted(ring_color, 0.18 * ease_out(self.anim)))
                    .with_width(thickness * 2.4),
            );
        }

        frame.stroke(
            &Path::circle(center, radius),
            Stroke::default().with_color(muted(self.text_color, 0.10)).with_width(thickness),
        );

        if progress > 0.0 {
            let start = -std::f32::consts::FRAC_PI_2;
            let arc = Path::new(|b| {
                b.arc(canvas::path::Arc {
                    center,
                    radius,
                    start_angle: Radians(start),
                    end_angle: Radians(start + progress * std::f32::consts::TAU),
                });
            });
            frame.stroke(
                &arc,
                Stroke::default()
                    .with_color(ring_color)
                    .with_width(thickness)
                    .with_line_cap(canvas::LineCap::Round),
            );
        }

        let headline_size = (radius * 0.42).clamp(16.0, 30.0);
        frame.fill_text(Text {
            content: self.headline.clone(),
            position: Point::new(center.x, center.y - headline_size * 0.62),
            size: headline_size.into(),
            color: self.text_color,
            align_x: cosmic::iced::alignment::Horizontal::Center.into(),
            ..Text::default()
        });
        frame.fill_text(Text {
            content: self.caption.clone(),
            position: Point::new(center.x, center.y + headline_size * 0.28),
            size: 11.0.into(),
            color: muted(self.text_color, 0.65),
            align_x: cosmic::iced::alignment::Horizontal::Center.into(),
            ..Text::default()
        });

        vec![frame.into_geometry()]
    }
}

/// Seven bars, Monday-Sunday, showing which weekdays actually carry the work.
/// Bars grow in on reveal with a small per-column stagger, and today's column
/// is drawn in full color while the rest are muted — so the chart answers
/// "when am I productive, and where does today sit" at a glance.
pub struct WeekdayProfile {
    pub minutes: [u64; 7],
    pub today_index: usize,
    pub color: Color,
    pub text_color: Color,
    pub anim: f32,
}

impl<Message> canvas::Program<Message, cosmic::Theme> for WeekdayProfile {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        const DAYS: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];
        let mut frame = Frame::new(renderer, bounds.size());

        let max = self.minutes.iter().copied().max().unwrap_or(0);
        if max == 0 {
            frame.fill_text(label_text(
                "No focus logged yet this month".to_string(),
                Point::new(PADDING, PADDING),
                12.0,
                muted(self.text_color, 0.6),
            ));
            return vec![frame.into_geometry()];
        }

        let label_h = 16.0;
        let plot_h = (bounds.height - PADDING * 2.0 - label_h).max(4.0);
        let slot = (bounds.width - PADDING * 2.0) / 7.0;
        let bar_w = (slot * 0.52).clamp(6.0, 26.0);

        for (i, minutes) in self.minutes.iter().enumerate() {
            // Stagger: each column starts a beat after the previous one.
            let local = ((self.anim * 1.6) - i as f32 * 0.06).clamp(0.0, 1.0);
            let ratio = *minutes as f32 / max as f32;
            let h = (plot_h * ratio * ease_out(local)).max(if *minutes > 0 { 2.0 } else { 0.0 });
            let x = PADDING + slot * i as f32 + (slot - bar_w) / 2.0;
            let y = PADDING + plot_h - h;
            let is_today = i == self.today_index;

            // Track behind every column keeps the baseline legible on
            // low-data days, where most columns are zero.
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(x, PADDING),
                    Size::new(bar_w, plot_h),
                    (bar_w / 3.0).into(),
                ),
                muted(self.text_color, 0.05),
            );

            if h > 0.0 {
                frame.fill(
                    &Path::rounded_rectangle(Point::new(x, y), Size::new(bar_w, h), (bar_w / 3.0).into()),
                    if is_today { self.color } else { muted(self.color, 0.45) },
                );
            }

            frame.fill_text(Text {
                content: DAYS[i].to_string(),
                position: Point::new(x + bar_w / 2.0, PADDING + plot_h + 3.0),
                size: 11.0.into(),
                color: muted(self.text_color, if is_today { 0.95 } else { 0.55 }),
                align_x: cosmic::iced::alignment::Horizontal::Center.into(),
                ..Text::default()
            });
        }

        vec![frame.into_geometry()]
    }
}

/// A continuous daily series, e.g. `stats::daily_breakdown()`'s output.
/// `fill: true` renders as a filled area chart under the line.
pub struct TrendChart {
    pub points: Vec<(NaiveDate, u64)>,
    pub color: Color,
    pub text_color: Color,
    pub fill: bool,
}

impl<Message> canvas::Program<Message, cosmic::Theme> for TrendChart {
    type State = Option<usize>;

    fn update(
        &self,
        state: &mut Self::State,
        _event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let n = self.points.len();
        let hovered = cursor.position_in(bounds).and_then(|p| {
            if n < 2 {
                return None;
            }
            let step = (bounds.width - PADDING * 2.0) / (n - 1) as f32;
            if step <= 0.0 {
                return None;
            }
            let index = ((p.x - PADDING) / step).round() as isize;
            (index >= 0 && (index as usize) < n).then_some(index as usize)
        });
        (hovered != *state).then(|| {
            *state = hovered;
            canvas::Action::request_redraw()
        })
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if self.points.len() < 2 {
            frame.fill_text(label_text(
                "Not enough data yet".to_string(),
                Point::new(PADDING, PADDING),
                12.0,
                muted(self.text_color, 0.6),
            ));
            return vec![frame.into_geometry()];
        }

        let max_value = self.points.iter().map(|(_, v)| *v).max().unwrap_or(1).max(1) as f32;
        let plot_w = bounds.width - PADDING * 2.0;
        let plot_h = bounds.height - PADDING * 2.0 - 14.0; // leave room for a hover caption
        let n = self.points.len();
        let step = plot_w / (n - 1) as f32;

        let point_at = |i: usize| -> Point {
            let (_, v) = self.points[i];
            let x = PADDING + step * i as f32;
            let y = PADDING + plot_h * (1.0 - v as f32 / max_value);
            Point::new(x, y)
        };

        let line = Path::new(|b| {
            b.move_to(point_at(0));
            for i in 1..n {
                b.line_to(point_at(i));
            }
        });

        if self.fill {
            let area = Path::new(|b| {
                b.move_to(Point::new(PADDING, PADDING + plot_h));
                for i in 0..n {
                    b.line_to(point_at(i));
                }
                b.line_to(Point::new(PADDING + plot_w, PADDING + plot_h));
                b.close();
            });
            frame.fill(&area, muted(self.color, 0.18));
        }

        frame.stroke(&line, canvas::Stroke::default().with_color(self.color).with_width(2.0));

        if let Some(i) = *state {
            let p = point_at(i);
            frame.fill(&Path::circle(p, 3.0), self.color);
            let (date, minutes) = self.points[i];
            frame.fill_text(label_text(
                format!("{} · {}", date.format("%b %-d"), format_short(minutes)),
                Point::new(PADDING, bounds.height - 12.0),
                11.0,
                self.text_color,
            ));
        }

        vec![frame.into_geometry()]
    }
}

/// A GitHub-style contribution grid: `weeks` columns across, 7 rows down,
/// most-recent week on the right. Cell alpha scales with value/max.
pub struct HeatmapCalendar {
    pub days: Vec<(NaiveDate, u64)>,
    pub base_color: Color,
    pub text_color: Color,
    pub weeks: u32,
}

impl<Message> canvas::Program<Message, cosmic::Theme> for HeatmapCalendar {
    type State = Option<usize>;

    fn update(
        &self,
        state: &mut Self::State,
        _event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let (cell, gap, col_w) = self.geometry(bounds);
        let offset = self.row_offset();
        let hovered = cursor.position_in(bounds).and_then(|p| {
            if col_w <= 0.0 {
                return None;
            }
            let col = ((p.x - PADDING) / col_w).floor() as isize;
            let row = ((p.y - PADDING) / (cell + gap)).floor() as isize;
            if col < 0 || row < 0 || row > 6 {
                return None;
            }
            // Inverse of draw()'s slot mapping — must stay in step with it,
            // or the tooltip names a different day than the cell under the
            // cursor.
            let slot = col as usize * 7 + row as usize;
            let index = slot.checked_sub(offset)?;
            (index < self.days.len().min(self.rendered_cells())).then_some(index)
        });
        (hovered != *state).then(|| {
            *state = hovered;
            canvas::Action::request_redraw()
        })
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if self.days.is_empty() {
            return vec![frame.into_geometry()];
        }

        let max_value = self.days.iter().map(|(_, v)| *v).max().unwrap_or(1).max(1);
        let (cell, gap, col_w) = self.geometry(bounds);
        let rendered = self.rendered_cells();
        let offset = self.row_offset();

        for (index, (_, value)) in self.days.iter().take(rendered).enumerate() {
            // Offset by the first day's weekday so every row is a fixed
            // weekday (row 0 = Monday). Without this the grid's rows are an
            // arbitrary rotation and the calendar shape means nothing.
            let slot = index + offset;
            let col = slot / 7;
            let row = slot % 7;
            let x = PADDING + col as f32 * col_w;
            let y = PADDING + row as f32 * (cell + gap);
            let intensity = if *value == 0 { 0.08 } else { 0.2 + 0.8 * (*value as f32 / max_value as f32) };
            let highlight = if *state == Some(index) { 1.0 } else { intensity };
            frame.fill_rectangle(
                Point::new(x, y),
                Size::new(cell, cell),
                muted(self.base_color, highlight.min(1.0)),
            );
        }

        if let Some(i) = *state {
            if let Some((date, minutes)) = self.days.get(i) {
                frame.fill_text(label_text(
                    format!("{} · {}", date.format("%b %-d"), format_short(*minutes)),
                    Point::new(PADDING, bounds.height - 12.0),
                    11.0,
                    self.text_color,
                ));
            }
        }

        vec![frame.into_geometry()]
    }
}

impl HeatmapCalendar {
    /// (cell side length, gap between cells, column stride) for the current bounds.
    fn geometry(&self, bounds: Rectangle) -> (f32, f32, f32) {
        let gap = 3.0;
        let cell = (((bounds.height - PADDING * 2.0) / 7.0) - gap).max(4.0);
        let col_w = cell + gap;
        (cell, gap, col_w)
    }

    /// Caps how many day-cells get drawn to `weeks` columns worth, in case
    /// `days` holds more than that (the caller is expected to pass exactly
    /// `weeks * 7` entries, but this keeps rendering correct either way).
    fn rendered_cells(&self) -> usize {
        self.weeks as usize * 7
    }

    /// How many blank cells precede the first day, so that row 0 is always
    /// Monday regardless of which weekday the series happens to begin on.
    fn row_offset(&self) -> usize {
        self.days
            .first()
            .map(|(date, _)| date.weekday().num_days_from_monday() as usize)
            .unwrap_or(0)
    }
}
