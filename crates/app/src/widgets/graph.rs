//! The commit graph: lanes, author avatars, ref badges, summary, hash and
//! date, drawn on a cached canvas. Hover tooltips are drawn on top, uncached.

use iced::advanced::image::Image;
use iced::alignment::Vertical;
use iced::mouse;
use iced::widget::canvas::{self, Cache, Frame, Geometry, Path, Stroke, Text};
use iced::widget::text::Alignment as TextAlign;
use iced::{Color, Font, Pixels, Point, Rectangle, Renderer, Size, Theme, font};
use lazygit_core::graph::Segment;
use lazygit_core::{Commit, RefKind};

use crate::avatars::{self, Avatars};
use crate::data::RepoData;
use crate::style;
use crate::util;
use crate::workspace::WsMessage as Message;

pub const ROW_H: f32 = 30.0;
const LANE_W: f32 = 26.0;
const MAX_LANES: usize = 12;
const AVATAR_R: f32 = 10.0;
const TEXT_SIZE: f32 = 13.0;
/// Rough average glyph width at TEXT_SIZE, used to size badges.
const CHAR_W: f32 = 7.2;
const HASH_W: f32 = 72.0;
const DATE_W: f32 = 96.0;

pub struct GraphView<'a> {
    pub data: &'a RepoData,
    pub avatars: &'a Avatars,
    pub selected: Option<usize>,
    pub cache: &'a Cache,
    pub now: i64,
}

/// Row under the mouse and which part of it.
#[derive(Default)]
pub struct Hover {
    row: Option<usize>,
    on_avatar: bool,
    on_hash: bool,
    cursor: Point,
}

fn lane_x(lane: usize) -> f32 {
    14.0 + lane as f32 * LANE_W
}

impl GraphView<'_> {
    pub fn height(&self) -> f32 {
        self.data.commits.len().max(1) as f32 * ROW_H
    }

    fn graph_width(&self) -> f32 {
        let lanes = self.data.rows.iter().map(|r| r.width).max().unwrap_or(1).min(MAX_LANES);
        lanes as f32 * LANE_W + 14.0
    }

    /// Whether the date column fits; the hash column is kept longer.
    fn show_date(&self, width: f32) -> bool {
        width - self.graph_width() > 520.0
    }

    fn hash_x(&self, width: f32) -> f32 {
        if self.show_date(width) { width - DATE_W - HASH_W - 8.0 } else { width - HASH_W - 4.0 }
    }

    fn hit(&self, p: Point, width: f32) -> Hover {
        let row = (p.y / ROW_H) as usize;
        if row >= self.data.commits.len() {
            return Hover::default();
        }
        let center = Point::new(lane_x(self.data.rows[row].lane), row as f32 * ROW_H + ROW_H / 2.0);
        let hash_x = self.hash_x(width);
        Hover {
            row: Some(row),
            on_avatar: center.distance(p) <= AVATAR_R + 2.0,
            on_hash: hash_x - self.graph_width() > 160.0 && p.x >= hash_x && p.x <= hash_x + HASH_W,
            cursor: p,
        }
    }
}

impl canvas::Program<Message> for GraphView<'_> {
    type State = Hover;

    fn update(
        &self,
        state: &mut Hover,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let hover = cursor.position_in(bounds).map(|p| self.hit(p, bounds.width)).unwrap_or_default();
                let changed = hover.row != state.row || hover.on_avatar != state.on_avatar || hover.on_avatar;
                *state = hover;
                changed.then(canvas::Action::request_redraw)
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                let hit = self.hit(p, bounds.width);
                let commit = &self.data.commits[hit.row?];
                let msg = if hit.on_hash {
                    Message::CopyHash(commit.id.clone())
                } else {
                    Message::SelectCommit(commit.id.clone())
                };
                Some(canvas::Action::publish(msg).and_capture())
            }
            _ => None,
        }
    }

    fn mouse_interaction(&self, state: &Hover, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if !cursor.is_over(bounds) {
            mouse::Interaction::default()
        } else if state.on_hash {
            mouse::Interaction::Copy
        } else {
            mouse::Interaction::Pointer
        }
    }

    fn draw(
        &self,
        state: &Hover,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut layers = vec![self.cache.draw(renderer, bounds.size(), |frame| self.paint(frame, bounds.width))];
        if let Some(row) = state.row
            && state.on_avatar
        {
            let mut frame = Frame::new(renderer, bounds.size());
            tooltip(&mut frame, &self.data.commits[row], state.cursor, bounds.width);
            layers.push(frame.into_geometry());
        }
        layers
    }
}

impl GraphView<'_> {
    fn paint(&self, frame: &mut Frame, width: f32) {
        let p = style::p();
        let data = self.data;
        if data.commits.is_empty() {
            frame.fill_text(text("Nenhum commit ainda.", Point::new(12.0, 8.0), p.muted));
            return;
        }
        let text_x = self.graph_width();
        let hash_x = self.hash_x(width);
        let date_x = width - DATE_W;
        let show_hash = hash_x - text_x > 160.0;
        let show_date = show_hash && self.show_date(width);
        let head_id = data.head.id.as_deref();

        for (i, (commit, row)) in data.commits.iter().zip(&data.rows).enumerate() {
            let y0 = i as f32 * ROW_H;
            let mid = y0 + ROW_H / 2.0;
            let y1 = y0 + ROW_H;
            let is_head = Some(commit.id.as_str()) == head_id;

            if self.selected == Some(i) {
                frame.fill_rectangle(Point::new(0.0, y0), Size::new(width, ROW_H), p.selected);
            } else if is_head {
                frame.fill_rectangle(Point::new(0.0, y0), Size::new(width, ROW_H), p.accent.scale_alpha(0.10));
            }

            for seg in &row.segments {
                let (from, to, a_y, b_y, color) = match *seg {
                    Segment::IntoNode { from, to, color } => (from, to, y0, mid, color),
                    Segment::OutOfNode { from, to, color } => (from, to, mid, y1, color),
                    Segment::Through { from, to, color } => (from, to, y0, y1, color),
                };
                let a = Point::new(lane_x(from), a_y);
                let b = Point::new(lane_x(to), b_y);
                let path = if from == to {
                    Path::line(a, b)
                } else {
                    Path::new(|pb| {
                        let c = (a_y + b_y) / 2.0;
                        pb.move_to(a);
                        pb.bezier_curve_to(Point::new(a.x, c), Point::new(b.x, c), b);
                    })
                };
                frame.stroke(&path, Stroke::default().with_color(p.lane(color)).with_width(2.0));
            }

            let center = Point::new(lane_x(row.lane), mid);
            let lane_color = p.lane(row.color);
            self.avatar(frame, commit, center, lane_color, is_head);

            // Ref badges; the checked-out branch is filled and bold.
            let mut x = text_x;
            if let Some(labels) = data.labels.get(&commit.id) {
                for &r in labels {
                    let r = &data.refs[r];
                    let current = r.kind == RefKind::LocalBranch && data.head.branch.as_deref() == Some(r.name.as_str());
                    let label = if current { format!("✔ {}", r.name) } else { r.name.clone() };
                    let badge = match r.kind {
                        RefKind::LocalBranch => lane_color,
                        RefKind::RemoteBranch => p.muted,
                        RefKind::Tag => p.warn,
                    };
                    let w = label.chars().count() as f32 * CHAR_W + if current { 18.0 } else { 12.0 };
                    let rect = Path::rounded_rectangle(Point::new(x, y0 + 5.0), Size::new(w, ROW_H - 10.0), 4.0.into());
                    if current {
                        frame.fill(&rect, p.accent);
                        frame.fill_text(Text {
                            font: Font { weight: font::Weight::Bold, ..Font::default() },
                            ..text(&label, Point::new(x + 7.0, y0 + 7.0), p.bg)
                        });
                    } else {
                        frame.fill(&rect, badge.scale_alpha(0.22));
                        frame.stroke(&rect, Stroke::default().with_color(badge).with_width(1.0));
                        frame.fill_text(text(&label, Point::new(x + 6.0, y0 + 7.0), p.text));
                    }
                    x += w + 6.0;
                }
            }

            let limit = if show_hash { hash_x } else { width };
            let summary_w = (limit - x - 12.0).max(40.0);
            frame.fill_text(Text {
                max_width: summary_w,
                font: if is_head { Font { weight: font::Weight::Semibold, ..Font::default() } } else { Font::default() },
                ..text(&truncate(&commit.summary, summary_w), Point::new(x, y0 + 7.0), p.text)
            });
            if show_hash {
                frame.fill_text(Text {
                    font: Font::MONOSPACE,
                    ..text(&commit.id[..7.min(commit.id.len())], Point::new(hash_x, y0 + 7.0), p.accent)
                });
            }
            if show_date {
                frame.fill_text(text(&util::relative_time(commit.time, self.now), Point::new(date_x, y0 + 7.0), p.muted));
            }
        }
    }

    fn avatar(&self, frame: &mut Frame, commit: &Commit, center: Point, ring: Color, is_head: bool) {
        let p = style::p();
        if is_head {
            frame.fill(&Path::circle(center, AVATAR_R + 4.5), p.accent.scale_alpha(0.35));
        }
        frame.fill(&Path::circle(center, AVATAR_R + 2.0), ring);
        let top_left = Point::new(center.x - AVATAR_R, center.y - AVATAR_R);
        let size = Size::new(AVATAR_R * 2.0, AVATAR_R * 2.0);
        match self.avatars.get(&commit.email) {
            Some(handle) => {
                let mut image = Image::new(handle.clone());
                image.border_radius = AVATAR_R.into();
                frame.draw_image(Rectangle::new(top_left, size), image);
            }
            None => {
                frame.fill(&Path::circle(center, AVATAR_R), avatars::color(&commit.email));
                frame.fill_text(Text {
                    content: avatars::initials(&commit.author),
                    position: center,
                    color: Color::WHITE,
                    size: Pixels(9.0),
                    font: Font { weight: font::Weight::Bold, ..Font::default() },
                    align_x: TextAlign::Center,
                    align_y: Vertical::Center,
                    ..Text::default()
                });
            }
        }
        if commit.parents.len() > 1 {
            // Merge commits get a small marker.
            frame.fill(&Path::circle(Point::new(center.x + AVATAR_R - 1.0, center.y + AVATAR_R - 1.0), 3.5), ring);
        }
    }
}

fn tooltip(frame: &mut Frame, commit: &Commit, cursor: Point, width: f32) {
    let p = style::p();
    let lines = [commit.author.as_str(), commit.email.as_str()];
    let w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32 * CHAR_W + 20.0;
    let h = 44.0;
    let x = (cursor.x + 14.0).min(width - w - 4.0).max(4.0);
    let y = (cursor.y - h - 6.0).max(2.0);
    let rect = Path::rounded_rectangle(Point::new(x, y), Size::new(w, h), 6.0.into());
    frame.fill(&rect, p.panel_alt);
    frame.stroke(&rect, Stroke::default().with_color(p.border).with_width(1.0));
    frame.fill_text(Text {
        font: Font { weight: font::Weight::Bold, ..Font::default() },
        ..text(lines[0], Point::new(x + 10.0, y + 6.0), p.text)
    });
    frame.fill_text(Text { size: Pixels(12.0), ..text(lines[1], Point::new(x + 10.0, y + 24.0), p.muted) });
}

fn text(content: &str, position: Point, color: Color) -> Text {
    Text {
        content: content.to_owned(),
        position,
        color,
        size: Pixels(TEXT_SIZE),
        font: Font::default(),
        ..Text::default()
    }
}

/// Cuts text to roughly fit `width` pixels, adding an ellipsis.
fn truncate(s: &str, width: f32) -> String {
    let max = (width / CHAR_W) as usize;
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}
