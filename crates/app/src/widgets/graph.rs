//! The commit graph: hash, lanes, author avatars, ref badges, summary, date
//! and author, drawn on a cached canvas ([`GraphBase`]). A second canvas stacked on
//! top ([`GraphView`]) takes the mouse and draws tooltips and the expanded ref
//! list. They must be separate canvases: iced draws all the text of a canvas
//! after all its shapes, so a popup in the same canvas would let the text
//! under it show through its background.
//!
//! With local changes, an "uncommitted changes" row sits above the first commit.
//!
//! The minimal look ([`style::minimal`]) draws shorter rows, narrower lanes and
//! a dot per commit instead of the avatar.

use std::cell::RefCell;
use std::collections::HashMap;
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

const MAX_LANES: usize = 12;
const TEXT_SIZE: f32 = 13.0;
/// Rough average glyph width at TEXT_SIZE, used to size tooltips and truncate text.
const CHAR_W: f32 = 7.2;
/// The hash is the first column; the lanes start after it.
const HASH_PAD: f32 = 10.0;
const HASH_W: f32 = 72.0;
const DATE_W: f32 = 96.0;
const AUTHOR_W: f32 = 130.0;
/// Room taken by one local/remote icon inside a badge.
const ICON_W: f32 = 15.0;
/// Room taken by the tag icon, left of a tag's name.
const TAG_ICON_W: f32 = 14.0;
/// Room taken by the pin icon of the pinned branch, left of its name.
const PIN_W: f32 = 13.0;
const POPUP_PAD: f32 = 6.0;
const POPUP_GAP: f32 = 5.0;

fn row_h() -> f32 {
    if style::minimal() { 26.0 } else { 30.0 }
}

fn lane_w() -> f32 {
    if style::minimal() { 16.0 } else { 26.0 }
}

/// Radius of a commit node: the avatar, or the dot in the minimal look.
fn node_r() -> f32 {
    if style::minimal() { 4.0 } else { 10.0 }
}

fn badge_h() -> f32 {
    row_h() - if style::minimal() { 8.0 } else { 10.0 }
}

/// Top of the text in a row starting at `y0`.
fn text_y(y0: f32) -> f32 {
    y0 + (row_h() - 16.0) / 2.0
}

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
    /// One of its refs is pinned to the first lane.
    pinned: bool,
}

impl Group {
    /// Local/remote icons drawn at the end of the badge.
    fn icons(&self) -> usize {
        self.local.is_some() as usize + self.remote.is_some() as usize
    }

    /// Offset of the name: the pin and tag icons come first, in both looks.
    fn name_x(&self) -> f32 {
        (if self.pinned { PIN_W } else { 0.0 }) + if self.tag.is_some() { TAG_ICON_W } else { 0.0 }
    }

    /// Draws the pin and tag icons at the start of `rect`, in `color`.
    fn lead_icons(&self, frame: &mut Frame, rect: Rectangle, color: Color) {
        let mut x = rect.x + 5.0;
        if self.pinned {
            frame.fill_text(Text {
                content: icons::PIN.to_string(),
                position: Point::new(x, rect.center_y()),
                color,
                size: Pixels(11.0),
                font: icons::FONT,
                align_y: Vertical::Center,
                ..Text::default()
            });
            x += PIN_W;
        }
        if self.tag.is_some() {
            tag_icon(frame, Point::new(x, rect.center_y()), color);
        }
    }

    fn label(&self) -> String {
        if self.current { format!("✔ {}", self.name) } else { self.name.clone() }
    }

    /// Font and size of the name, which differ between the two looks.
    fn font(&self) -> (Font, f32) {
        let weight = match (self.current, style::minimal()) {
            (false, _) => font::Weight::Normal,
            (true, true) => font::Weight::Semibold,
            (true, false) => font::Weight::Bold,
        };
        (Font { weight, ..Font::default() }, if style::minimal() { 12.0 } else { TEXT_SIZE })
    }

    /// Left padding before the lead icons and the name.
    fn pad(&self) -> f32 {
        if self.current && !style::minimal() { 7.0 } else { 6.0 }
    }

    fn width(&self) -> f32 {
        let (font, size) = self.font();
        let icons = self.icons() as f32 * ICON_W;
        let end = if icons > 0.0 { 4.0 + icons + 4.0 } else { 6.0 };
        self.pad() + self.name_x() + text_width(&self.label(), font, size) + end
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
        let (row_h, badge_h) = (row_h(), badge_h());
        let h = 2.0 * POPUP_PAD + self.groups.len() as f32 * (badge_h + POPUP_GAP) - POPUP_GAP;
        let mut y = row as f32 * row_h + 5.0 - POPUP_PAD;
        if y + h > height {
            y = (row as f32 * row_h + row_h - 5.0 + POPUP_PAD - h).max(0.0);
        }
        let x = self.x - POPUP_PAD;
        let items = (0..self.groups.len())
            .map(|i| {
                let top = y + POPUP_PAD + i as f32 * (badge_h + POPUP_GAP);
                Rectangle::new(Point::new(x + POPUP_PAD, top), Size::new(self.groups[i].width(), badge_h))
            })
            .collect::<Vec<_>>();
        (Rectangle::new(Point::new(x, y), Size::new(w + 2.0 * POPUP_PAD, h)), items)
    }
}

/// Top of commit `row` on screen, below the uncommitted-changes row if any.
pub fn row_top(data: &RepoData, row: usize) -> f32 {
    (row + wip_rows(data)) as f32 * row_h()
}

/// Rows drawn above the first commit: 1 when the working tree has changes.
fn wip_rows(data: &RepoData) -> usize {
    (!data.changes.is_empty()) as usize
}

fn lane_x(lane: usize) -> f32 {
    HASH_PAD + HASH_W + 14.0 + lane as f32 * lane_w()
}

impl GraphView<'_> {
    pub fn height(&self) -> f32 {
        (self.data.commits.len() + wip_rows(self.data)).max(1) as f32 * row_h()
    }

    fn top(&self, row: usize) -> f32 {
        row_top(self.data, row)
    }

    fn graph_width(&self) -> f32 {
        let lanes = self.data.rows.iter().map(|r| r.width).max().unwrap_or(1).min(MAX_LANES);
        HASH_PAD + HASH_W + lanes as f32 * lane_w() + 14.0
    }

    /// Left edges of the date and author columns, each when it fits: the
    /// author goes first when the panel gets narrow.
    fn columns(&self, width: f32) -> (Option<f32>, Option<f32>) {
        let room = width - self.graph_width();
        let author = (room > 660.0).then_some(width - AUTHOR_W);
        let date = (room > 520.0).then(|| author.unwrap_or(width) - DATE_W);
        (date, author)
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
                    groups.push(Group { name: name.to_owned(), local: None, remote: None, tag: None, current: false, pinned: false });
                    groups.last_mut().unwrap()
                }
            };
            group.pinned |= data.pinned == Some(index);
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
        let more_w = if groups.len() > 1 { text_width(&format!("+{}", groups.len() - 1), Font::default(), more_size()) + 12.0 } else { 0.0 };
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
    fn hit(&self, p: Point, expanded: Option<usize>) -> Hover {
        let offset = wip_rows(self.data);
        if let Some(row) = expanded {
            let badges = self.badges(row);
            let (frame, items) = badges.popup(row + offset, self.height());
            if frame.contains(p) {
                let badge = items.iter().position(|r| r.contains(p));
                return Hover { row: Some(row), badge, expanded: Some(row), cursor: p, ..Hover::default() };
            }
        }
        let screen = (p.y / row_h()) as usize;
        if screen < offset {
            return Hover { wip: true, cursor: p, ..Hover::default() };
        }
        let row = screen - offset;
        if row >= self.data.commits.len() {
            return Hover::default();
        }
        let center = Point::new(lane_x(self.data.rows[row].lane), self.top(row) + row_h() / 2.0);
        let badges = self.badges(row);
        let on_badges = badges.contains_x(p.x);
        let badge = (on_badges && p.x <= badges.x + badges.first_w).then_some(0);
        Hover {
            row: Some(row),
            // The dot is small: allow a few pixels around it.
            on_avatar: center.distance(p) <= node_r() + if style::minimal() { 5.0 } else { 2.0 },
            on_hash: (HASH_PAD..=HASH_PAD + HASH_W).contains(&p.x),
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
                    cursor.position_in(bounds).map(|p| self.hit(p, state.expanded)).unwrap_or_default();
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
                let hit = self.hit(p, state.expanded);
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
                let hit = self.hit(p, state.expanded);
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
        let minimal = style::minimal();
        let (row_h, badge_h) = (row_h(), badge_h());
        let (date_x, author_x) = self.columns(width);
        // Hash, date and author: quieter and smaller in the minimal look.
        let quiet = if minimal { p.faint() } else { p.muted };
        let small = |s: &str, at: Point, color: Color| Text { size: Pixels(if minimal { 12.0 } else { TEXT_SIZE }), ..text(s, at, color) };
        let head_id = data.head.id.as_deref();
        if wip_rows(data) > 0 {
            self.wip_row(frame, width);
        }

        for (i, (commit, row)) in data.commits.iter().zip(&data.rows).enumerate() {
            let y0 = self.top(i);
            let mid = y0 + row_h / 2.0;
            let y1 = y0 + row_h;
            let ty = text_y(y0);
            let is_head = Some(commit.id.as_str()) == head_id;

            if self.selected == Some(i) {
                let color = if minimal { p.accent.scale_alpha(0.12) } else { p.selected };
                frame.fill_rectangle(Point::new(0.0, y0), Size::new(width, row_h), color);
            } else if is_head && !minimal {
                frame.fill_rectangle(Point::new(0.0, y0), Size::new(width, row_h), p.accent.scale_alpha(0.10));
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
                frame.stroke(&path, Stroke::default().with_color(p.lane(color)).with_width(if minimal { 1.5 } else { 2.0 }));
            }

            let center = Point::new(lane_x(row.lane), mid);
            let lane_color = p.lane(row.color);
            self.avatar(frame, commit, center, lane_color, is_head);

            // Ref badges: the first group, then `+N` for the others.
            let badges = self.badges(i);
            let by = y0 + (row_h - badge_h) / 2.0;
            if let Some(first) = badges.groups.first() {
                let rect = Rectangle::new(Point::new(badges.x, by), Size::new(badges.first_w, badge_h));
                self.badge(frame, first, rect, lane_color, false);
            }
            if badges.more_w > 0.0 {
                let mx = badges.x + badges.first_w + 4.0;
                let rect = Path::rounded_rectangle(Point::new(mx, by), Size::new(badges.more_w, badge_h), 4.0.into());
                frame.fill(&rect, p.muted.scale_alpha(if minimal { 0.14 } else { 0.22 }));
                if !minimal {
                    frame.stroke(&rect, Stroke::default().with_color(p.muted.scale_alpha(0.6)).with_width(1.0));
                }
                frame.fill_text(Text {
                    size: Pixels(more_size()),
                    ..text(&format!("+{}", badges.groups.len() - 1), Point::new(mx + 6.0, by + 3.0), if minimal { p.muted } else { p.text })
                });
            }
            let x = badges.end();

            let limit = date_x.or(author_x).unwrap_or(width);
            let summary_w = (limit - x - 12.0).max(40.0);
            let weight = match (is_head, minimal) {
                (true, true) => font::Weight::Medium,
                (true, false) => font::Weight::Semibold,
                _ => font::Weight::Normal,
            };
            frame.fill_text(Text {
                max_width: summary_w,
                font: Font { weight, ..Font::default() },
                ..text(&truncate(&commit.summary, summary_w), Point::new(x, ty), p.text)
            });
            frame.fill_text(Text {
                font: Font::MONOSPACE,
                ..small(&commit.id[..7.min(commit.id.len())], Point::new(HASH_PAD, ty), if minimal { quiet } else { p.accent })
            });
            if let Some(date_x) = date_x {
                frame.fill_text(small(&util::relative_time(commit.time, self.now), Point::new(date_x, ty), quiet));
            }
            if let Some(author_x) = author_x {
                let name = truncate(&commit.author, AUTHOR_W - 12.0);
                frame.fill_text(Text { max_width: AUTHOR_W - 8.0, ..small(&name, Point::new(author_x, ty), quiet) });
            }
        }
    }

    /// The uncommitted-changes row: a dashed node in the lane of HEAD, joined to it by a dashed line.
    fn wip_row(&self, frame: &mut Frame, width: f32) {
        let p = style::p();
        let data = self.data;
        let minimal = style::minimal();
        let (row_h, r) = (row_h(), node_r());
        if self.worktree_selected {
            frame.fill_rectangle(Point::ORIGIN, Size::new(width, row_h), if minimal { p.accent.scale_alpha(0.12) } else { p.selected });
        }
        let head = data.head.id.as_deref().and_then(|id| data.commit_index(id));
        let (lane, color) = head.map_or((0, p.accent), |h| (data.rows[h].lane, p.lane(data.rows[h].color)));
        let center = Point::new(lane_x(lane), row_h / 2.0);
        let dashed = Stroke {
            line_dash: LineDash { segments: if minimal { &[2.0, 2.0] } else { &[3.0, 3.0] }, offset: 0 },
            ..Stroke::default().with_color(color).with_width(if minimal { 1.5 } else { 2.0 })
        };
        if let Some(h) = head {
            // Drawn before the commits, so avatars on the way cover it.
            let to = self.top(h) + row_h / 2.0 - r - 1.0;
            frame.stroke(&Path::line(Point::new(center.x, center.y + r + 1.0), Point::new(center.x, to)), dashed);
        }
        frame.stroke(&Path::circle(center, r), dashed);

        let n = data.changes.len();
        let ink = if self.worktree_selected { p.text } else { p.muted };
        let x = self.graph_width();
        let ty = text_y(0.0);
        frame.fill_text(Text { font: icons::FONT, size: Pixels(13.0), ..text(&icons::PENCIL.to_string(), Point::new(x, ty + 2.0), ink) });
        let label = format!("Alterações não commitadas · {n} arquivo{}", if n == 1 { "" } else { "s" });
        frame.fill_text(Text {
            font: Font { style: font::Style::Italic, ..Font::default() },
            ..text(&label, Point::new(x + 20.0, ty), ink)
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
        if style::minimal() {
            // No frame: a faint tint of the ref color, the current branch as an outline.
            let ink = if g.current { p.accent } else { color };
            if g.current {
                frame.stroke(&path, Stroke::default().with_color(p.accent).with_width(1.0));
            } else {
                frame.fill(&path, color.scale_alpha(if hovered { 0.26 } else { 0.14 }));
            }
            g.lead_icons(frame, rect, ink);
            let (font, size) = g.font();
            let tx = rect.x + g.pad() + g.name_x();
            frame.fill_text(Text { font, size: Pixels(size), ..text(&g.label(), Point::new(tx, rect.y + 1.5), ink) });
            self.place_icons(frame, g, rect, if g.current { p.accent } else { p.muted });
            return;
        }
        let ink = if g.current { p.bg } else { p.text };
        if g.current {
            frame.fill(&path, p.accent);
        } else {
            frame.fill(&path, color.scale_alpha(if hovered { 0.38 } else { 0.22 }));
            frame.stroke(&path, Stroke::default().with_color(color).with_width(1.0));
        }
        let (font, size) = g.font();
        g.lead_icons(frame, rect, ink);
        let tx = rect.x + g.pad() + g.name_x();
        frame.fill_text(Text { font, size: Pixels(size), ..text(&g.label(), Point::new(tx, rect.y + 2.0), ink) });

        self.place_icons(frame, g, rect, if g.current { p.bg } else { p.muted });
    }

    /// The computer (local) and cloud (remote) icons at the end of a badge.
    fn place_icons(&self, frame: &mut Frame, g: &Group, rect: Rectangle, color: Color) {
        let mut ix = rect.x + rect.width - 4.0 - g.icons() as f32 * ICON_W;
        let cy = rect.center_y();
        if g.local.is_some() {
            computer_icon(frame, Point::new(ix + 1.0, cy), color);
            ix += ICON_W;
        }
        if g.remote.is_some() {
            cloud_icon(frame, Point::new(ix, cy), color);
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

    /// Round picture (or initials) framed by a ring in the lane color; in the
    /// minimal look, a dot in the lane color with a ring around HEAD.
    fn avatar(&self, frame: &mut Frame, commit: &Commit, center: Point, ring: Color, is_head: bool) {
        let p = style::p();
        let r = node_r();
        if style::minimal() {
            frame.fill(&Path::circle(center, r), ring);
            if is_head {
                frame.stroke(&Path::circle(center, r + 3.0), Stroke::default().with_color(ring).with_width(1.5));
            }
            return;
        }
        if is_head {
            frame.fill(&Path::circle(center, r + 5.0), p.accent.scale_alpha(0.30));
        }
        let top_left = Point::new(center.x - r, center.y - r);
        let size = Size::new(r * 2.0, r * 2.0);
        match self.avatars.get(&commit.email) {
            Some(handle) => {
                // Fill behind the picture so the lane line never shows through.
                frame.fill(&Path::circle(center, r), p.panel);
                frame.draw_image(Rectangle::new(top_left, size), Image::new(handle.clone()));
            }
            None => {
                frame.fill(&Path::circle(center, r), avatars::color(&commit.email));
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
        frame.stroke(&Path::circle(center, r + 1.0), Stroke::default().with_color(ring).with_width(2.5));
        if commit.parents.len() > 1 {
            // Merge commits get a small marker.
            frame.fill(&Path::circle(Point::new(center.x + r, center.y + r), 3.5), ring);
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

/// A price tag pointing right with its hole, ~11px wide, vertically centered on `at.y`.
fn tag_icon(frame: &mut Frame, at: Point, color: Color) {
    let (x, y) = (at.x, at.y);
    let outline = Path::new(|pb| {
        pb.move_to(Point::new(x, y - 3.5));
        pb.line_to(Point::new(x + 6.5, y - 3.5));
        pb.line_to(Point::new(x + 10.5, y));
        pb.line_to(Point::new(x + 6.5, y + 3.5));
        pb.line_to(Point::new(x, y + 3.5));
        pb.close();
    });
    frame.stroke(&outline, Stroke::default().with_color(color).with_width(1.2));
    frame.fill(&Path::circle(Point::new(x + 2.6, y), 1.0), color);
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

/// Size of the `+N` badge's text.
fn more_size() -> f32 {
    if style::minimal() { 11.0 } else { 12.0 }
}

/// Width of one line of `content` as the renderer shapes it, cached since
/// badges are measured on every paint and hover.
fn text_width(content: &str, font: Font, size: f32) -> f32 {
    use iced::advanced::graphics::text::Paragraph;
    use iced::advanced::text::{self as core_text, Paragraph as _};

    thread_local! {
        static WIDTHS: RefCell<HashMap<(String, Font, u32), f32>> = RefCell::new(HashMap::new());
    }
    let key = (content.to_owned(), font, size.to_bits());
    if let Some(w) = WIDTHS.with_borrow(|m| m.get(&key).copied()) {
        return w;
    }
    let w = Paragraph::with_text(core_text::Text {
        content,
        bounds: Size::INFINITE,
        size: Pixels(size),
        line_height: core_text::LineHeight::default(),
        font,
        align_x: TextAlign::Default,
        align_y: Vertical::Top,
        shaping: core_text::Shaping::default(),
        wrapping: core_text::Wrapping::None,
    })
    .min_width();
    WIDTHS.with_borrow_mut(|m| m.insert(key, w));
    w
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
        assert_eq!(g.height(), 2.0 * row_h());
        assert_eq!(row_top(&data, 0), row_h());

        let wip = g.hit(Point::new(200.0, row_h() / 2.0), None);
        assert!(wip.wip);
        assert_eq!(wip.row, None);

        let first = g.hit(Point::new(200.0, row_h() * 1.5), None);
        assert!(!first.wip);
        assert_eq!(first.row, Some(0));
        assert_eq!(g.hit(Point::new(200.0, row_h() * 2.5), None).row, None);
    }

    #[test]
    fn clean_tree_has_no_extra_row() {
        let (avatars, cache) = (Avatars::default(), Cache::new());
        let (_d, data) = repo(false);
        let g = view(&data, &avatars, &cache);
        assert_eq!(g.height(), row_h());
        assert_eq!(row_top(&data, 0), 0.0);
        let first = g.hit(Point::new(200.0, row_h() / 2.0), None);
        assert!(!first.wip);
        assert_eq!(first.row, Some(0));
    }
}
