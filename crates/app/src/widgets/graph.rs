//! The commit graph: lanes, author avatars, ref badges, summary, hash and
//! date, drawn on a cached canvas ([`GraphBase`]). A second canvas stacked on
//! top ([`GraphView`]) takes the mouse and draws tooltips and the expanded ref
//! list. They must be separate canvases: iced draws all the text of a canvas
//! after all its shapes, so a popup in the same canvas would let the text
//! under it show through its background.
//!
//! With local changes, an "uncommitted changes" row sits above the first commit.

use std::time::{Duration, Instant};

use iced::advanced::image::Image;
use iced::alignment::Vertical;
use iced::mouse;
use iced::widget::canvas::{self, Cache, Frame, Geometry, LineDash, Path, Stroke, Text};
use iced::widget::text::Alignment as TextAlign;
use iced::{Color, Font, Pixels, Point, Rectangle, Renderer, Size, Theme, font};
use brio_core::graph::Segment;
use brio_core::{Commit, RefKind};

use crate::avatars::{self, Avatars};
use crate::icons;
use crate::menu::Target;
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
const BADGE_H: f32 = ROW_H - 10.0;
/// Room taken by one local/remote icon inside a badge.
const ICON_W: f32 = 15.0;
const POPUP_PAD: f32 = 6.0;
const POPUP_GAP: f32 = 5.0;

#[derive(Clone, Copy)]
pub struct GraphView<'a> {
    pub data: &'a RepoData,
    pub avatars: &'a Avatars,
    pub selected: Option<usize>,
    /// The uncommitted-changes row is selected.
    pub worktree_selected: bool,
    pub cache: &'a Cache,
    pub now: i64,
}

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Row under the mouse and which part of it.
#[derive(Default)]
pub struct Hover {
    /// Commit index (not screen row) under the mouse.
    row: Option<usize>,
    /// Mouse over the uncommitted-changes row.
    wip: bool,
    on_avatar: bool,
    on_hash: bool,
    /// Index into the row's ref groups of the badge under the mouse.
    badge: Option<usize>,
    /// Row whose ref groups are shown expanded (mouse over its `+N` badge).
    expanded: Option<usize>,
    cursor: Point,
    last_click: Option<(usize, Instant)>,
}

/// Refs sharing a name: a local branch and its remote counterparts, or a tag.
struct Group {
    name: String,
    local: Option<usize>,
    remote: Option<usize>,
    tag: Option<usize>,
    current: bool,
}

impl Group {
    fn icons(&self) -> usize {
        self.local.is_some() as usize + self.remote.is_some() as usize
    }

    fn width(&self) -> f32 {
        let chars = self.name.chars().count() + if self.current { 2 } else { 0 };
        let icons = self.icons() as f32 * ICON_W;
        chars as f32 * CHAR_W + if self.current { 18.0 } else { 12.0 } + if icons > 0.0 { icons + 2.0 } else { 0.0 }
    }
}

/// Ref groups of a row as drawn: the first group and a `+N` badge for the rest.
struct Badges {
    groups: Vec<Group>,
    x: f32,
    /// Width of the first group's badge.
    first_w: f32,
    /// Width of the `+N` badge, 0 when there is a single group.
    more_w: f32,
}

impl Badges {
    fn end(&self) -> f32 {
        if self.groups.is_empty() {
            self.x
        } else if self.more_w > 0.0 {
            self.x + self.first_w + 4.0 + self.more_w + 6.0
        } else {
            self.x + self.first_w + 6.0
        }
    }

    fn contains_x(&self, x: f32) -> bool {
        !self.groups.is_empty() && x >= self.x && x <= self.end() - 6.0
    }

    /// The expanded list drawn over screen row `row`: its frame and one rect per group.
    /// It opens downwards, or upwards when it would pass `height`.
    fn popup(&self, row: usize, height: f32) -> (Rectangle, Vec<Rectangle>) {
        let w = self.groups.iter().map(Group::width).fold(0.0, f32::max);
        let h = 2.0 * POPUP_PAD + self.groups.len() as f32 * (BADGE_H + POPUP_GAP) - POPUP_GAP;
        let mut y = row as f32 * ROW_H + 5.0 - POPUP_PAD;
        if y + h > height {
            y = (row as f32 * ROW_H + ROW_H - 5.0 + POPUP_PAD - h).max(0.0);
        }
        let x = self.x - POPUP_PAD;
        let items = (0..self.groups.len())
            .map(|i| {
                let top = y + POPUP_PAD + i as f32 * (BADGE_H + POPUP_GAP);
                Rectangle::new(Point::new(x + POPUP_PAD, top), Size::new(self.groups[i].width(), BADGE_H))
            })
            .collect::<Vec<_>>();
        (Rectangle::new(Point::new(x, y), Size::new(w + 2.0 * POPUP_PAD, h)), items)
    }
}

/// Top of commit `row` on screen, below the uncommitted-changes row if any.
pub fn row_top(data: &RepoData, row: usize) -> f32 {
    (row + wip_rows(data)) as f32 * ROW_H
}

/// Rows drawn above the first commit: 1 when the working tree has changes.
fn wip_rows(data: &RepoData) -> usize {
    (!data.changes.is_empty()) as usize
}

fn lane_x(lane: usize) -> f32 {
    14.0 + lane as f32 * LANE_W
}

impl GraphView<'_> {
    pub fn height(&self) -> f32 {
        (self.data.commits.len() + wip_rows(self.data)).max(1) as f32 * ROW_H
    }

    fn top(&self, row: usize) -> f32 {
        row_top(self.data, row)
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

    /// Refs of commit `row` grouped by name, the checked-out branch first.
    fn groups(&self, row: usize) -> Vec<Group> {
        let data = self.data;
        let mut groups: Vec<Group> = Vec::new();
        for &index in data.labels.get(&data.commits[row].id).into_iter().flatten() {
            let r = &data.refs[index];
            let name = match r.kind {
                RefKind::RemoteBranch => data
                    .remotes
                    .iter()
                    .find_map(|remote| r.name.strip_prefix(remote.as_str())?.strip_prefix('/'))
                    .unwrap_or(&r.name),
                _ => &r.name,
            };
            let is_tag = r.kind == RefKind::Tag;
            let pos = groups.iter().position(|g| g.name == name && g.tag.is_some() == is_tag);
            let group = match pos {
                Some(i) => &mut groups[i],
                None => {
                    groups.push(Group { name: name.to_owned(), local: None, remote: None, tag: None, current: false });
                    groups.last_mut().unwrap()
                }
            };
            match r.kind {
                RefKind::LocalBranch => {
                    group.local = Some(index);
                    group.current = data.head.branch.as_deref() == Some(r.name.as_str());
                }
                RefKind::RemoteBranch => group.remote = group.remote.or(Some(index)),
                RefKind::Tag => group.tag = Some(index),
            }
        }
        // Stable: keeps refs order (locals, remotes, tags) after the current branch.
        groups.sort_by_key(|g| !g.current);
        groups
    }

    fn badges(&self, row: usize) -> Badges {
        let groups = self.groups(row);
        let first_w = groups.first().map_or(0.0, Group::width);
        let more_w = if groups.len() > 1 { format!("+{}", groups.len() - 1).len() as f32 * CHAR_W + 12.0 } else { 0.0 };
        Badges { groups, x: self.graph_width(), first_w, more_w }
    }

    /// Ref targeted by a group: the local branch when there is one.
    fn group_target(&self, g: &Group) -> Option<Target> {
        let r = &self.data.refs[g.local.or(g.remote).or(g.tag)?];
        Some(match r.kind {
            RefKind::Tag => Target::Tag(r.name.clone()),
            kind => Target::Branch { name: r.name.clone(), remote: kind == RefKind::RemoteBranch },
        })
    }

    /// What is under `p`; `expanded` is the row whose popup is open, which
    /// stays open (and takes the hit) while the mouse is over it.
    fn hit(&self, p: Point, width: f32, expanded: Option<usize>) -> Hover {
        let offset = wip_rows(self.data);
        if let Some(row) = expanded {
            let badges = self.badges(row);
            let (frame, items) = badges.popup(row + offset, self.height());
            if frame.contains(p) {
                let badge = items.iter().position(|r| r.contains(p));
                return Hover { row: Some(row), badge, expanded: Some(row), cursor: p, ..Hover::default() };
            }
        }
        let screen = (p.y / ROW_H) as usize;
        if screen < offset {
            return Hover { wip: true, cursor: p, ..Hover::default() };
        }
        let row = screen - offset;
        if row >= self.data.commits.len() {
            return Hover::default();
        }
        let center = Point::new(lane_x(self.data.rows[row].lane), self.top(row) + ROW_H / 2.0);
        let hash_x = self.hash_x(width);
        let badges = self.badges(row);
        let on_badges = badges.contains_x(p.x);
        let badge = (on_badges && p.x <= badges.x + badges.first_w).then_some(0);
        Hover {
            row: Some(row),
            on_avatar: center.distance(p) <= AVATAR_R + 2.0,
            on_hash: hash_x - self.graph_width() > 160.0 && p.x >= hash_x && p.x <= hash_x + HASH_W,
            badge,
            expanded: (on_badges && badges.groups.len() > 1).then_some(row),
            cursor: p,
            wip: false,
            last_click: None,
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
                let mut hover =
                    cursor.position_in(bounds).map(|p| self.hit(p, bounds.width, state.expanded)).unwrap_or_default();
                hover.last_click = state.last_click;
                let changed = hover.row != state.row
                    || hover.wip != state.wip
                    || hover.on_avatar != state.on_avatar
                    || hover.on_avatar
                    || hover.expanded != state.expanded
                    || (hover.expanded.is_some() && hover.badge != state.badge);
                *state = hover;
                changed.then(canvas::Action::request_redraw)
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                let hit = self.hit(p, bounds.width, state.expanded);
                if hit.wip {
                    return Some(canvas::Action::publish(Message::ShowWorktree).and_capture());
                }
                let row = hit.row?;
                let commit = &self.data.commits[row];
                let now = Instant::now();
                let double = state.last_click.is_some_and(|(r, t)| r == row && now.duration_since(t) < DOUBLE_CLICK);
                state.last_click = if double { None } else { Some((row, now)) };
                let msg = if hit.on_hash {
                    Message::CopyHash(commit.id.clone())
                } else if double {
                    Message::CheckoutCommit(commit.id.clone())
                } else {
                    Message::SelectCommit(commit.id.clone())
                };
                Some(canvas::Action::publish(msg).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                let p = cursor.position_in(bounds)?;
                let hit = self.hit(p, bounds.width, state.expanded);
                if hit.wip {
                    return Some(canvas::Action::publish(Message::OpenMenu(Target::Worktree)).and_capture());
                }
                let row = hit.row?;
                let groups = self.groups(row);
                let target = hit
                    .badge
                    .and_then(|i| groups.get(i))
                    .and_then(|g| self.group_target(g))
                    .unwrap_or_else(|| Target::Commit(self.data.commits[row].id.clone()));
                Some(canvas::Action::publish(Message::OpenMenu(target)).and_capture())
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
        let mut layers = Vec::new();
        if let Some(row) = state.row
            && state.on_avatar
        {
            let mut frame = Frame::new(renderer, bounds.size());
            tooltip(&mut frame, &self.data.commits[row], state.cursor, bounds.width);
            layers.push(frame.into_geometry());
        }
        if let Some(row) = state.expanded {
            let mut frame = Frame::new(renderer, bounds.size());
            self.popup(&mut frame, row, state.badge);
            layers.push(frame.into_geometry());
        }
        layers
    }
}

/// The cached graph itself, under the [`GraphView`] layer.
pub struct GraphBase<'a>(pub GraphView<'a>);

impl canvas::Program<Message> for GraphBase<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        vec![self.0.cache.draw(renderer, bounds.size(), |frame| self.0.paint(frame, bounds.width))]
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
        if wip_rows(data) > 0 {
            self.wip_row(frame, width);
        }

        for (i, (commit, row)) in data.commits.iter().zip(&data.rows).enumerate() {
            let y0 = self.top(i);
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

            // Ref badges: the first group, then `+N` for the others.
            let badges = self.badges(i);
            if let Some(first) = badges.groups.first() {
                let rect = Rectangle::new(Point::new(badges.x, y0 + 5.0), Size::new(badges.first_w, BADGE_H));
                self.badge(frame, first, rect, lane_color, false);
            }
            if badges.more_w > 0.0 {
                let mx = badges.x + badges.first_w + 4.0;
                let rect = Path::rounded_rectangle(Point::new(mx, y0 + 5.0), Size::new(badges.more_w, BADGE_H), 4.0.into());
                frame.fill(&rect, p.muted.scale_alpha(0.22));
                frame.stroke(&rect, Stroke::default().with_color(p.muted.scale_alpha(0.6)).with_width(1.0));
                frame.fill_text(Text {
                    size: Pixels(12.0),
                    ..text(&format!("+{}", badges.groups.len() - 1), Point::new(mx + 6.0, y0 + 8.0), p.text)
                });
            }
            let x = badges.end();

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

    /// The uncommitted-changes row: a dashed node in the lane of HEAD, joined to it by a dashed line.
    fn wip_row(&self, frame: &mut Frame, width: f32) {
        let p = style::p();
        let data = self.data;
        if self.worktree_selected {
            frame.fill_rectangle(Point::ORIGIN, Size::new(width, ROW_H), p.selected);
        }
        let head = data.head.id.as_deref().and_then(|id| data.commit_index(id));
        let (lane, color) = head.map_or((0, p.accent), |h| (data.rows[h].lane, p.lane(data.rows[h].color)));
        let center = Point::new(lane_x(lane), ROW_H / 2.0);
        let dashed = Stroke {
            line_dash: LineDash { segments: &[3.0, 3.0], offset: 0 },
            ..Stroke::default().with_color(color).with_width(2.0)
        };
        if let Some(h) = head {
            // Drawn before the commits, so avatars on the way cover it.
            let to = self.top(h) + ROW_H / 2.0 - AVATAR_R - 1.0;
            frame.stroke(&Path::line(Point::new(center.x, center.y + AVATAR_R + 1.0), Point::new(center.x, to)), dashed);
        }
        frame.stroke(&Path::circle(center, AVATAR_R), dashed);

        let n = data.changes.len();
        let ink = if self.worktree_selected { p.text } else { p.muted };
        let x = self.graph_width();
        frame.fill_text(Text { font: icons::FONT, size: Pixels(13.0), ..text(&icons::PENCIL.to_string(), Point::new(x, 9.0), ink) });
        let label = format!("Alterações não commitadas · {n} arquivo{}", if n == 1 { "" } else { "s" });
        frame.fill_text(Text {
            font: Font { style: font::Style::Italic, ..Font::default() },
            ..text(&label, Point::new(x + 20.0, 7.0), ink)
        });
    }

    /// One ref group: name, then a computer icon (local) and a cloud (remote).
    fn badge(&self, frame: &mut Frame, g: &Group, rect: Rectangle, lane_color: Color, hovered: bool) {
        let p = style::p();
        let path = Path::rounded_rectangle(rect.position(), rect.size(), 4.0.into());
        let color = if g.tag.is_some() {
            p.warn
        } else if g.local.is_some() {
            lane_color
        } else {
            p.muted
        };
        let ink = if g.current { p.bg } else { p.text };
        if g.current {
            frame.fill(&path, p.accent);
        } else {
            frame.fill(&path, color.scale_alpha(if hovered { 0.38 } else { 0.22 }));
            frame.stroke(&path, Stroke::default().with_color(color).with_width(1.0));
        }
        let label = if g.current { format!("✔ {}", g.name) } else { g.name.clone() };
        let font = if g.current { Font { weight: font::Weight::Bold, ..Font::default() } } else { Font::default() };
        let tx = rect.x + if g.current { 7.0 } else { 6.0 };
        frame.fill_text(Text { font, ..text(&label, Point::new(tx, rect.y + 2.0), ink) });

        let mut ix = rect.x + rect.width - 4.0 - g.icons() as f32 * ICON_W;
        let cy = rect.center_y();
        let icon_color = if g.current { p.bg } else { p.muted };
        if g.local.is_some() {
            computer_icon(frame, Point::new(ix + 1.0, cy), icon_color);
            ix += ICON_W;
        }
        if g.remote.is_some() {
            cloud_icon(frame, Point::new(ix, cy), icon_color);
        }
    }

    /// Every ref group of `row`, stacked in a panel over the graph.
    fn popup(&self, frame: &mut Frame, row: usize, hovered: Option<usize>) {
        let p = style::p();
        let badges = self.badges(row);
        let (rect, items) = badges.popup(row + wip_rows(self.data), self.height());
        let shadow = Path::rounded_rectangle(rect.position() + iced::Vector::new(0.0, 3.0), rect.size(), 7.0.into());
        frame.fill(&shadow, Color::from_rgba(0.0, 0.0, 0.0, 0.35));
        let path = Path::rounded_rectangle(rect.position(), rect.size(), 6.0.into());
        frame.fill(&path, p.panel_alt);
        frame.stroke(&path, Stroke::default().with_color(p.border).with_width(1.0));
        let lane_color = p.lane(self.data.rows[row].color);
        for (i, (g, r)) in badges.groups.iter().zip(items).enumerate() {
            self.badge(frame, g, r, lane_color, hovered == Some(i));
        }
    }

    /// Round picture (or initials) framed by a ring in the lane color.
    fn avatar(&self, frame: &mut Frame, commit: &Commit, center: Point, ring: Color, is_head: bool) {
        let p = style::p();
        if is_head {
            frame.fill(&Path::circle(center, AVATAR_R + 5.0), p.accent.scale_alpha(0.30));
        }
        let top_left = Point::new(center.x - AVATAR_R, center.y - AVATAR_R);
        let size = Size::new(AVATAR_R * 2.0, AVATAR_R * 2.0);
        match self.avatars.get(&commit.email) {
            Some(handle) => {
                // Fill behind the picture so the lane line never shows through.
                frame.fill(&Path::circle(center, AVATAR_R), p.panel);
                frame.draw_image(Rectangle::new(top_left, size), Image::new(handle.clone()));
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
        frame.stroke(&Path::circle(center, AVATAR_R + 1.0), Stroke::default().with_color(ring).with_width(2.5));
        if commit.parents.len() > 1 {
            // Merge commits get a small marker.
            frame.fill(&Path::circle(Point::new(center.x + AVATAR_R, center.y + AVATAR_R), 3.5), ring);
        }
    }
}

/// A monitor on a stand, ~11px wide, vertically centered on `at.y`.
fn computer_icon(frame: &mut Frame, at: Point, color: Color) {
    let stroke = Stroke::default().with_color(color).with_width(1.2);
    let screen = Path::rounded_rectangle(Point::new(at.x, at.y - 5.0), Size::new(11.0, 7.5), 1.5.into());
    frame.stroke(&screen, stroke);
    frame.stroke(&Path::line(Point::new(at.x + 5.5, at.y + 2.5), Point::new(at.x + 5.5, at.y + 4.5)), stroke);
    frame.stroke(&Path::line(Point::new(at.x + 2.5, at.y + 4.8), Point::new(at.x + 8.5, at.y + 4.8)), stroke);
}

/// A cloud outline, ~13px wide, vertically centered on `at.y`.
fn cloud_icon(frame: &mut Frame, at: Point, color: Color) {
    // Three bumps (left, top, right) over a flat base, traced as one outline.
    let base = at.y + 4.0;
    let left = (Point::new(at.x + 3.5, at.y + 1.0), 3.0);
    let top = (Point::new(at.x + 7.0, at.y - 0.8), 3.6);
    let right = (Point::new(at.x + 10.5, at.y + 1.5), 2.5);
    let lt = upper_intersection(left, top);
    let tr = upper_intersection(top, right);
    let angle = |c: Point, q: Point| (q.y - c.y).atan2(q.x - c.x);
    let tau = std::f32::consts::TAU;
    let path = Path::new(|pb| {
        let mut arc = |(c, r): (Point, f32), a0: f32, mut a1: f32, first: bool| {
            while a1 < a0 {
                a1 += tau;
            }
            for k in 0..=10 {
                let a = a0 + (a1 - a0) * k as f32 / 10.0;
                let q = Point::new(c.x + r * a.cos(), c.y + r * a.sin());
                if first && k == 0 { pb.move_to(q) } else { pb.line_to(q) }
            }
        };
        let quarter = std::f32::consts::FRAC_PI_2;
        // Left bump: from the base (straight down) round to where it meets the top bump.
        arc(left, quarter, angle(left.0, lt), true);
        arc(top, angle(top.0, lt), angle(top.0, tr), false);
        arc(right, angle(right.0, tr), quarter, false);
        pb.line_to(Point::new(left.0.x, base));
        pb.close();
    });
    frame.stroke(&path, Stroke::default().with_color(color).with_width(1.2));
}

/// The higher (smaller y) intersection point of two circles; they must overlap.
fn upper_intersection((c1, r1): (Point, f32), (c2, r2): (Point, f32)) -> Point {
    let (dx, dy) = (c2.x - c1.x, c2.y - c1.y);
    let d = (dx * dx + dy * dy).sqrt();
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let (mx, my) = (c1.x + a * dx / d, c1.y + a * dy / d);
    let (p1, p2) = (Point::new(mx + h * dy / d, my - h * dx / d), Point::new(mx - h * dy / d, my + h * dx / d));
    if p1.y < p2.y { p1 } else { p2 }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn repo(dirty: bool) -> (tempfile::TempDir, RepoData) {
        let dir = tempfile::tempdir().unwrap();
        let git = brio_ops::Git::new(dir.path());
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.name", "Ana"],
            &["config", "user.email", "ana@x.dev"],
            &["config", "commit.gpgsign", "false"],
        ] {
            git.run(args).unwrap();
        }
        fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-qm", "first"]).unwrap();
        if dirty {
            fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        }
        let data = RepoData::load(dir.path(), None).unwrap();
        (dir, data)
    }

    fn view<'a>(data: &'a RepoData, avatars: &'a Avatars, cache: &'a Cache) -> GraphView<'a> {
        GraphView { data, avatars, selected: None, worktree_selected: false, cache, now: 0 }
    }

    #[test]
    fn local_changes_add_a_row_above_the_commits() {
        let (avatars, cache) = (Avatars::default(), Cache::new());
        let (_d, data) = repo(true);
        let g = view(&data, &avatars, &cache);
        assert_eq!(g.height(), 2.0 * ROW_H);
        assert_eq!(row_top(&data, 0), ROW_H);

        let wip = g.hit(Point::new(200.0, ROW_H / 2.0), 800.0, None);
        assert!(wip.wip);
        assert_eq!(wip.row, None);

        let first = g.hit(Point::new(200.0, ROW_H * 1.5), 800.0, None);
        assert!(!first.wip);
        assert_eq!(first.row, Some(0));
        assert_eq!(g.hit(Point::new(200.0, ROW_H * 2.5), 800.0, None).row, None);
    }

    #[test]
    fn clean_tree_has_no_extra_row() {
        let (avatars, cache) = (Avatars::default(), Cache::new());
        let (_d, data) = repo(false);
        let g = view(&data, &avatars, &cache);
        assert_eq!(g.height(), ROW_H);
        assert_eq!(row_top(&data, 0), 0.0);
        let first = g.hit(Point::new(200.0, ROW_H / 2.0), 800.0, None);
        assert!(!first.wip);
        assert_eq!(first.row, Some(0));
    }
}
