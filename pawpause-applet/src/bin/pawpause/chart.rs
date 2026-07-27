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

use chrono::NaiveDate;
use cosmic::iced::widget::canvas::{self, Frame, Geometry, Path, Text};
use cosmic::iced::{mouse, Color, Point, Rectangle, Size};

const PADDING: f32 = 10.0;

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
pub struct BarChart {
    pub bars: Vec<(String, u64)>,
    pub color: Color,
    pub text_color: Color,
    pub max_bars: usize,
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
        let max_value = self.bars.iter().take(rows).map(|(_, v)| *v).max().unwrap_or(1).max(1);
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

            let ratio = *value as f32 / max_value as f32;
            let bar_w = (bar_area * ratio).max(2.0);
            let bar_path = Path::rectangle(Point::new(PADDING + label_col, bar_y), Size::new(bar_w, bar_h));
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
        let hovered = cursor.position_in(bounds).and_then(|p| {
            if col_w <= 0.0 {
                return None;
            }
            let col = ((p.x - PADDING) / col_w).floor() as isize;
            let row = ((p.y - PADDING) / (cell + gap)).floor() as isize;
            if col < 0 || row < 0 || row > 6 {
                return None;
            }
            let index = col as usize * 7 + row as usize;
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

        for (index, (_, value)) in self.days.iter().take(rendered).enumerate() {
            let col = index / 7;
            let row = index % 7;
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
}
