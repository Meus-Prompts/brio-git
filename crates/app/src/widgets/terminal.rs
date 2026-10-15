//! Draws a [`TermSnapshot`] and turns mouse input into terminal messages.

use iced::mouse;
use iced::widget::canvas::{self, Cache, Geometry, Path, Stroke, Text};
use iced::{Color, Font, Pixels, Point, Rectangle, Renderer, Size, Theme};
use lazygit_session::TermSnapshot;
use lazygit_session::pty::{DEFAULT_BG, DEFAULT_FG};

use crate::workspace::WsMessage as Message;
use crate::style;

pub const FONT_SIZE: f32 = 13.0;
/// Monospace cell size; the advance of common monospace fonts is ~0.6em.
pub const CELL_W: f32 = 7.8;
pub const CELL_H: f32 = 17.0;
const PAD: f32 = 6.0;

pub struct TerminalView<'a> {
    pub snapshot: Option<&'a TermSnapshot>,
    pub cache: &'a Cache,
    pub focused: bool,
    /// Current size of the terminal (cols, rows).
    pub size: (u16, u16),
    pub placeholder: &'a str,
}

/// Remembers the last size we asked for, so resizes are requested once.
#[derive(Default)]
pub struct ViewState {
    requested: Option<(u16, u16)>,
}

pub fn grid_size(bounds: Size) -> (u16, u16) {
    let cols = ((bounds.width - 2.0 * PAD) / CELL_W).floor().max(2.0) as u16;
    let rows = ((bounds.height - 2.0 * PAD) / CELL_H).floor().max(2.0) as u16;
    (cols, rows)
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

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(_)) => {
                let inside = cursor.is_over(bounds);
                if inside != self.focused {
                    let action = canvas::Action::publish(Message::TermFocus(inside));
                    return Some(if inside { action.and_capture() } else { action });
                }
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 3.0,
                    mouse::ScrollDelta::Pixels { y, .. } => y / CELL_H,
                };
                if lines.abs() >= 1.0 {
                    return Some(canvas::Action::publish(Message::TermScroll(lines as i32)).and_capture());
                }
            }
            _ => {}
        }
        None
    }

    fn mouse_interaction(&self, _state: &ViewState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if cursor.is_over(bounds) { mouse::Interaction::Text } else { mouse::Interaction::default() }
    }

    fn draw(
        &self,
        _state: &ViewState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
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
                    position: Point::new(size.width - 200.0, PAD),
                    color: style::p().warn,
                    size: Pixels(12.0),
                    ..Text::default()
                });
            }
        });

        let mut layers = vec![geometry];
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
