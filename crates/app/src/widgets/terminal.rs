//! Draws a [`TermSnapshot`] and turns mouse input into terminal messages.

use iced::mouse;
use iced::widget::canvas::{self, Cache, Geometry, Path, Stroke, Text};
use iced::{Color, Font, Pixels, Point, Rectangle, Renderer, Size, Theme};
use brio_session::TermSnapshot;
use brio_session::pty::{DEFAULT_BG, DEFAULT_FG};

use crate::workspace::WsMessage as Message;
use crate::style;

pub const FONT_SIZE: f32 = 13.0;
/// Monospace cell size; the advance of common monospace fonts is ~0.6em.
pub const CELL_W: f32 = 7.8;
pub const CELL_H: f32 = 17.0;
const PAD: f32 = 6.0;
/// Width reserved on the right for the scrollbar.
const SCROLLBAR_W: f32 = 10.0;
const THUMB_MIN_H: f32 = 24.0;

pub struct TerminalView<'a> {
    pub snapshot: Option<&'a TermSnapshot>,
    pub cache: &'a Cache,
    pub focused: bool,
    /// Current size of the terminal (cols, rows).
    pub size: (u16, u16),
    pub placeholder: &'a str,
}

#[derive(Default)]
pub struct ViewState {
    /// Last size we asked for, so resizes are requested once.
    requested: Option<(u16, u16)>,
    /// Fractional wheel lines not yet sent; touchpads scroll a few pixels at a time.
    wheel: f32,
    /// Whether the scrollbar thumb is being dragged.
    dragging: bool,
}

pub fn grid_size(bounds: Size) -> (u16, u16) {
    let cols = ((bounds.width - 2.0 * PAD - SCROLLBAR_W) / CELL_W).floor().max(2.0) as u16;
    let rows = ((bounds.height - 2.0 * PAD) / CELL_H).floor().max(2.0) as u16;
    (cols, rows)
}

/// Scrollbar geometry: (track, thumb) for a snapshot with history.
fn scrollbar(bounds: Size, snap: &TermSnapshot) -> Option<(Rectangle, Rectangle)> {
    if snap.history == 0 {
        return None;
    }
    let track = Rectangle::new(Point::new(bounds.width - SCROLLBAR_W, 0.0), Size::new(SCROLLBAR_W, bounds.height));
    let total = (snap.history + snap.rows) as f32;
    let thumb_h = (track.height * snap.rows as f32 / total).clamp(THUMB_MIN_H.min(track.height), track.height);
    let travel = track.height - thumb_h;
    let top = travel * (snap.history - snap.scrolled.min(snap.history)) as f32 / snap.history as f32;
    let thumb = Rectangle::new(Point::new(track.x + 2.0, top), Size::new(SCROLLBAR_W - 4.0, thumb_h));
    Some((track, thumb))
}

/// History offset that puts the thumb's center at `y` (relative to the widget).
fn offset_at(y: f32, bounds: Size, snap: &TermSnapshot) -> usize {
    let Some((track, thumb)) = scrollbar(bounds, snap) else { return 0 };
    let travel = (track.height - thumb.height).max(1.0);
    let frac = ((y - thumb.height / 2.0) / travel).clamp(0.0, 1.0);
    snap.history - (frac * snap.history as f32).round() as usize
}

fn rgb([r, g, b]: [u8; 3]) -> Color {
    Color::from_rgb8(r, g, b)
}

/// Maps the terminal's default colors to the active theme.
fn fg(c: [u8; 3]) -> Color {
    if c == DEFAULT_FG { style::p().text } else { rgb(c) }
}

fn bg(c: [u8; 3]) -> Color {
    if c == DEFAULT_BG { style::p().term_bg } else { rgb(c) }
}

impl canvas::Program<Message> for TerminalView<'_> {
    type State = ViewState;

    fn update(
        &self,
        state: &mut ViewState,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let wanted = grid_size(bounds.size());
        if wanted != self.size && state.requested != Some(wanted) {
            state.requested = Some(wanted);
            return Some(canvas::Action::publish(Message::TermResize(wanted.0, wanted.1)));
        }

        let size = bounds.size();
        let bar = self.snapshot.and_then(|snap| scrollbar(size, snap).map(|b| (snap, b)));

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if bar.is_some_and(|(_, (track, _))| cursor.position_in(bounds).is_some_and(|p| track.contains(p))) =>
            {
                let (snap, _) = bar?;
                let y = cursor.position_in(bounds)?.y;
                state.dragging = true;
                return Some(canvas::Action::publish(Message::TermScrollTo(offset_at(y, size, snap))).and_capture());
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
                let (snap, _) = bar?;
                let y = position.y - bounds.y;
                return Some(canvas::Action::publish(Message::TermScrollTo(offset_at(y, size, snap))).and_capture());
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                return Some(canvas::Action::request_redraw().and_capture());
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(_)) => {
                let inside = cursor.is_over(bounds);
                if inside != self.focused {
                    let action = canvas::Action::publish(Message::TermFocus(inside));
                    return Some(if inside { action.and_capture() } else { action });
                }
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                state.wheel += match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 3.0,
                    mouse::ScrollDelta::Pixels { y, .. } => y / CELL_H,
                };
                let lines = state.wheel.trunc();
                if lines == 0.0 {
                    return Some(canvas::Action::capture());
                }
                state.wheel -= lines;
                return Some(canvas::Action::publish(Message::TermScroll(lines as i32)).and_capture());
            }
            _ => {}
        }
        None
    }

    fn mouse_interaction(&self, state: &ViewState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        let on_bar = self
            .snapshot
            .and_then(|snap| scrollbar(bounds.size(), snap))
            .zip(cursor.position_in(bounds))
            .is_some_and(|((track, _), p)| track.contains(p));
        if state.dragging || on_bar {
            mouse::Interaction::default()
        } else if cursor.is_over(bounds) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        state: &ViewState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            let size = bounds.size();
            frame.fill_rectangle(Point::ORIGIN, size, style::p().term_bg);

            let Some(snap) = self.snapshot else {
                frame.fill_text(Text {
                    content: self.placeholder.to_owned(),
                    position: Point::new(PAD + 6.0, PAD + 6.0),
                    color: style::p().muted,
                    size: Pixels(14.0),
                    ..Text::default()
                });
                return;
            };

            for row in 0..snap.rows {
                let y = PAD + row as f32 * CELL_H;
                // Background runs.
                let mut col = 0;
                while col < snap.cols {
                    let bg = snap.cell(row, col).bg;
                    let start = col;
                    while col < snap.cols && snap.cell(row, col).bg == bg {
                        col += 1;
                    }
                    if bg != DEFAULT_BG {
                        frame.fill_rectangle(
                            Point::new(PAD + start as f32 * CELL_W, y),
                            Size::new((col - start) as f32 * CELL_W, CELL_H),
                            self::bg(bg),
                        );
                    }
                }
                // One glyph per cell keeps columns aligned whatever the font.
                for col in 0..snap.cols {
                    let cell = snap.cell(row, col);
                    if cell.ch == ' ' {
                        continue;
                    }
                    frame.fill_text(Text {
                        content: cell.ch.to_string(),
                        position: Point::new(PAD + col as f32 * CELL_W, y),
                        color: fg(cell.fg),
                        size: Pixels(FONT_SIZE),
                        font: if cell.bold { Font { weight: iced::font::Weight::Bold, ..Font::MONOSPACE } } else { Font::MONOSPACE },
                        ..Text::default()
                    });
                }
            }

            if let Some((row, col)) = snap.cursor {
                let top_left = Point::new(PAD + col as f32 * CELL_W, PAD + row as f32 * CELL_H);
                let cell = Size::new(CELL_W, CELL_H);
                if self.focused {
                    frame.fill_rectangle(top_left, cell, style::p().accent.scale_alpha(0.55));
                } else {
                    frame.stroke(
                        &Path::rectangle(top_left, cell),
                        Stroke::default().with_color(style::p().muted).with_width(1.0),
                    );
                }
            }

            if snap.scrolled > 0 {
                frame.fill_text(Text {
                    content: format!("↑ histórico ({} linhas)", snap.scrolled),
                    position: Point::new(size.width - SCROLLBAR_W - 200.0, PAD),
                    color: style::p().warn,
                    size: Pixels(12.0),
                    ..Text::default()
                });
            }
        });

        let mut layers = vec![geometry];
        if let Some((track, thumb)) = self.snapshot.and_then(|snap| scrollbar(bounds.size(), snap)) {
            let hovered = cursor.position_in(bounds).is_some_and(|p| track.contains(p));
            let alpha = if state.dragging { 0.9 } else if hovered { 0.7 } else { 0.45 };
            let mut frame = canvas::Frame::new(renderer, bounds.size());
            frame.fill_rectangle(track.position(), track.size(), style::p().muted.scale_alpha(0.08));
            frame.fill(
                &Path::rounded_rectangle(thumb.position(), thumb.size(), (thumb.width / 2.0).into()),
                style::p().muted.scale_alpha(alpha),
            );
            layers.push(frame.into_geometry());
        }
        if self.focused {
            let mut frame = canvas::Frame::new(renderer, bounds.size());
            frame.stroke(
                &Path::rectangle(Point::new(0.5, 0.5), Size::new(bounds.width - 1.0, bounds.height - 1.0)),
                Stroke::default().with_color(style::p().accent).with_width(1.0),
            );
            layers.push(frame.into_geometry());
        }
        layers
    }
}
