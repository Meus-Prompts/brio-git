//! Panel layout: which panels are visible and how they are arranged.
//!
//! The arrangement is a tree of splits ([`Tree`]) that the user reshapes by
//! dragging panels around and moving dividers; it is read back from the
//! `pane_grid` after each change and saved to disk. Layouts saved before the
//! tree existed only had a few fractions; [`Layout::legacy`] turns those into
//! the original arrangement (branches and reflog on the left, graph, details,
//! AI session below), which is also the default.

use iced::widget::pane_grid::{self, Axis, Configuration, Node};
use serde::{Deserialize, Serialize};

use crate::icons;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PaneKind {
    Sidebar,
    Reflog,
    Graph,
    Details,
    Session,
}

impl PaneKind {
    pub const ALL: [PaneKind; 5] =
        [PaneKind::Sidebar, PaneKind::Reflog, PaneKind::Graph, PaneKind::Details, PaneKind::Session];

    pub fn title(self) -> &'static str {
        match self {
            PaneKind::Sidebar => "Branches",
            PaneKind::Reflog => "Reflog",
            PaneKind::Graph => "Grafo",
            PaneKind::Details => "Alterações",
            PaneKind::Session => "Sessão de IA",
        }
    }

    pub fn icon(self) -> char {
        match self {
            PaneKind::Sidebar => icons::GIT_BRANCH,
            PaneKind::Reflog => icons::HISTORY,
            PaneKind::Graph => icons::GIT_GRAPH,
            PaneKind::Details => icons::FILE_DIFF,
            PaneKind::Session => icons::SPARKLES,
        }
    }
}

/// Direction of a split, as saved. `Rows` stacks its halves top/bottom
/// (a horizontal divider); `Columns` puts them side by side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitAxis {
    Rows,
    Columns,
}

impl SplitAxis {
    fn to_iced(self) -> Axis {
        match self {
            SplitAxis::Rows => Axis::Horizontal,
            SplitAxis::Columns => Axis::Vertical,
        }
    }

    fn from_iced(axis: Axis) -> Self {
        match axis {
            Axis::Horizontal => SplitAxis::Rows,
            Axis::Vertical => SplitAxis::Columns,
        }
    }
}

/// The arrangement of the visible panels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Tree {
    /// `a` takes `ratio` of the room (top or left), `b` the rest.
    Split { axis: SplitAxis, ratio: f32, a: Box<Tree>, b: Box<Tree> },
    Pane(PaneKind),
}

impl Tree {
    fn split(axis: SplitAxis, ratio: f32, a: Tree, b: Tree) -> Tree {
        Tree::Split { axis, ratio: ratio.clamp(MIN_FRACTION, 1.0 - MIN_FRACTION), a: Box::new(a), b: Box::new(b) }
    }

    fn from_state(state: &pane_grid::State<PaneKind>) -> Option<Tree> {
        fn walk(node: &Node, state: &pane_grid::State<PaneKind>) -> Option<Tree> {
            match node {
                Node::Split { axis, ratio, a, b, .. } => match (walk(a, state), walk(b, state)) {
                    (Some(a), Some(b)) => Some(Tree::split(SplitAxis::from_iced(*axis), *ratio, a, b)),
                    (one, other) => one.or(other),
                },
                Node::Pane(pane) => state.get(*pane).copied().map(Tree::Pane),
            }
        }
        walk(state.layout(), state)
    }

    fn configuration(&self) -> Configuration<PaneKind> {
        match self {
            Tree::Split { axis, ratio, a, b } => Configuration::Split {
                axis: axis.to_iced(),
                ratio: *ratio,
                a: Box::new(a.configuration()),
                b: Box::new(b.configuration()),
            },
            Tree::Pane(kind) => Configuration::Pane(*kind),
        }
    }

    pub fn panes(&self) -> Vec<PaneKind> {
        match self {
            Tree::Split { a, b, .. } => {
                let mut panes = a.panes();
                panes.extend(b.panes());
                panes
            }
            Tree::Pane(kind) => vec![*kind],
        }
    }

    /// The tree without `kind`; its sibling takes the freed room.
    fn without(self, kind: PaneKind) -> Option<Tree> {
        match self {
            Tree::Pane(k) if k == kind => None,
            Tree::Pane(k) => Some(Tree::Pane(k)),
            Tree::Split { axis, ratio, a, b } => match (a.without(kind), b.without(kind)) {
                (Some(a), Some(b)) => Some(Tree::split(axis, ratio, a, b)),
                (one, other) => one.or(other),
            },
        }
    }

    /// Splits the largest panel in two along its longer side and puts `kind`
    /// in the new half. Sizes are measured on a `width` x `height` area.
    fn with(self, kind: PaneKind, width: f32, height: f32) -> Tree {
        let target = self.largest(width, height).map(|(k, _)| k);
        self.insert_at(kind, target, width, height)
    }

    /// The panel with the most room, and that room.
    fn largest(&self, width: f32, height: f32) -> Option<(PaneKind, f32)> {
        match self {
            Tree::Pane(k) => Some((*k, width * height)),
            Tree::Split { axis, ratio, a, b } => {
                let ((aw, ah), (bw, bh)) = halves(*axis, *ratio, width, height);
                let (a, b) = (a.largest(aw, ah), b.largest(bw, bh));
                match (a, b) {
                    (Some(a), Some(b)) => Some(if b.1 > a.1 { b } else { a }),
                    (one, other) => one.or(other),
                }
            }
        }
    }

    fn insert_at(self, kind: PaneKind, target: Option<PaneKind>, width: f32, height: f32) -> Tree {
        match self {
            Tree::Pane(k) if Some(k) == target => {
                let axis = if width >= height { SplitAxis::Columns } else { SplitAxis::Rows };
                Tree::split(axis, 0.5, Tree::Pane(k), Tree::Pane(kind))
            }
            Tree::Pane(k) => Tree::Pane(k),
            Tree::Split { axis, ratio, a, b } => {
                let ((aw, ah), (bw, bh)) = halves(axis, ratio, width, height);
                Tree::Split {
                    axis,
                    ratio,
                    a: Box::new(a.insert_at(kind, target, aw, ah)),
                    b: Box::new(b.insert_at(kind, target, bw, bh)),
                }
            }
        }
    }
}

/// Sizes of the two halves of a split.
fn halves(axis: SplitAxis, ratio: f32, width: f32, height: f32) -> ((f32, f32), (f32, f32)) {
    match axis {
        SplitAxis::Rows => ((width, height * ratio), (width, height * (1.0 - ratio))),
        SplitAxis::Columns => ((width * ratio, height), (width * (1.0 - ratio), height)),
    }
}

/// Typical window proportions, used to tell a panel's longer side.
const ASPECT: (f32, f32) = (16.0, 10.0);
const MIN_FRACTION: f32 = 0.05;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// Arrangement of the visible panels; `None` until the user changes it
    /// (or when every panel is closed), meaning the arrangement from the
    /// fractions below.
    #[serde(default)]
    pub tree: Option<Tree>,
    /// Width of the left column (branches and reflog), as a fraction of the window width.
    pub sidebar: f32,
    /// Height of the reflog, as a fraction of the left column when it shares it with the branches.
    pub reflog: f32,
    /// Width of the details panel.
    pub details: f32,
    /// Height of the AI session panel, as a fraction of the window height.
    pub session: f32,
    pub hidden: Vec<PaneKind>,
}

impl Default for Layout {
    fn default() -> Self {
        Self { tree: None, sidebar: 0.16, reflog: 0.4, details: 0.32, session: 0.36, hidden: Vec::new() }
    }
}

impl Layout {
    pub fn is_visible(&self, kind: PaneKind) -> bool {
        !self.hidden.contains(&kind)
    }

    pub fn set_visible(&mut self, kind: PaneKind, visible: bool) {
        let tree = self.current();
        self.hidden.retain(|k| *k != kind);
        if !visible {
            self.hidden.push(kind);
        }
        self.tree = match tree {
            Some(t) if visible && t.panes().contains(&kind) => Some(t),
            Some(t) if visible => Some(t.with(kind, ASPECT.0, ASPECT.1)),
            Some(t) => t.without(kind),
            None if visible => Some(Tree::Pane(kind)),
            None => None,
        };
    }

    /// The arrangement as shown: the saved tree kept in step with `hidden`
    /// (panels hidden elsewhere are dropped, missing visible ones added).
    pub fn current(&self) -> Option<Tree> {
        let Some(tree) = self.tree.clone() else { return self.legacy() };
        let mut tree = Some(tree);
        for kind in PaneKind::ALL {
            let present = tree.as_ref().is_some_and(|t| t.panes().contains(&kind));
            tree = match (tree, present, self.is_visible(kind)) {
                (Some(t), true, false) => t.without(kind),
                (Some(t), false, true) => Some(t.with(kind, ASPECT.0, ASPECT.1)),
                (None, _, true) => Some(Tree::Pane(kind)),
                (t, _, _) => t,
            };
        }
        tree
    }

    /// Builds the pane grid for the visible panels, or `None` if all are closed.
    pub fn build(&self) -> Option<pane_grid::State<PaneKind>> {
        self.current().map(|t| pane_grid::State::with_configuration(t.configuration()))
    }

    /// Reads the arrangement back from the grid after a drag or resize.
    pub fn capture(&mut self, state: &pane_grid::State<PaneKind>) {
        self.tree = Tree::from_state(state);
    }

    /// Back to the default arrangement with every panel open.
    pub fn reset(&mut self) {
        *self = Layout::default();
    }

    /// The original arrangement, from the saved fractions: branches over
    /// reflog on the left, then graph and details, with the AI session below.
    fn legacy(&self) -> Option<Tree> {
        let pane = |kind| self.is_visible(kind).then_some(Tree::Pane(kind));
        let left = match (pane(PaneKind::Sidebar), pane(PaneKind::Reflog)) {
            (Some(s), Some(r)) => Some(Tree::split(SplitAxis::Rows, 1.0 - self.reflog, s, r)),
            (one, other) => one.or(other),
        };
        let columns: Vec<(Tree, f32)> = [
            left.map(|t| (t, self.sidebar)),
            pane(PaneKind::Graph).map(|t| (t, (1.0 - self.sidebar - self.details).max(0.2))),
            pane(PaneKind::Details).map(|t| (t, self.details)),
        ]
        .into_iter()
        .flatten()
        .collect();
        match (row(columns), pane(PaneKind::Session)) {
            (Some(top), Some(session)) => Some(Tree::split(SplitAxis::Rows, 1.0 - self.session, top, session)),
            (one, other) => one.or(other),
        }
    }
}

/// Columns side by side, each taking its share of the summed widths.
fn row(mut columns: Vec<(Tree, f32)>) -> Option<Tree> {
    if columns.len() <= 1 {
        return columns.pop().map(|(t, _)| t);
    }
    let total: f32 = columns.iter().map(|(_, w)| w).sum();
    let (first, width) = columns.remove(0);
    Some(Tree::split(SplitAxis::Columns, width / total, first, row(columns)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut kinds: Vec<PaneKind>) -> Vec<PaneKind> {
        kinds.sort_by_key(|k| PaneKind::ALL.iter().position(|a| a == k));
        kinds
    }

    fn kinds(state: &pane_grid::State<PaneKind>) -> Vec<PaneKind> {
        sorted(state.iter().map(|(_, k)| *k).collect())
    }

    #[test]
    fn default_layout_is_the_classic_arrangement() {
        let layout = Layout::default();
        let state = layout.build().unwrap();
        assert_eq!(kinds(&state), PaneKind::ALL.to_vec());
        let Some(Tree::Split { axis: SplitAxis::Rows, a, b, .. }) = layout.current() else { panic!() };
        assert_eq!(*b, Tree::Pane(PaneKind::Session), "AI session below everything");
        assert_eq!(a.panes(), vec![PaneKind::Sidebar, PaneKind::Reflog, PaneKind::Graph, PaneKind::Details]);
    }

    #[test]
    fn grid_round_trips_through_the_tree() {
        let mut layout = Layout::default();
        let state = layout.build().unwrap();
        layout.capture(&state);
        let tree = layout.tree.clone().unwrap();
        assert_eq!(Some(tree.clone()), Layout::default().current());
        let again = Tree::from_state(&pane_grid::State::with_configuration(tree.configuration()));
        assert_eq!(again, Some(tree));
    }

    #[test]
    fn dragging_a_panel_is_saved() {
        let mut layout = Layout::default();
        let mut state = layout.build().unwrap();
        let find = |s: &pane_grid::State<PaneKind>, k| *s.iter().find(|(_, kind)| **kind == k).unwrap().0;
        let (session, graph) = (find(&state, PaneKind::Session), find(&state, PaneKind::Graph));
        // Session dropped on the top edge of the graph: graph column becomes graph-over-session.
        state.drop(session, pane_grid::Target::Pane(graph, pane_grid::Region::Edge(pane_grid::Edge::Top)));
        layout.capture(&state);

        let json = serde_json::to_string(&layout).unwrap();
        let loaded: Layout = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded, layout);
        let rebuilt = loaded.build().unwrap();
        assert_eq!(Tree::from_state(&rebuilt), layout.tree);
        let Some(Tree::Split { axis: SplitAxis::Columns, .. }) = &layout.tree else {
            panic!("session no longer spans the bottom: {:?}", layout.tree)
        };
    }

    #[test]
    fn old_saved_layout_migrates() {
        let json = r#"{"sidebar":0.2,"details":0.3,"session":0.25,"hidden":["Session"]}"#;
        let layout: Layout = serde_json::from_str(json).unwrap();
        assert_eq!(layout.tree, None);
        let state = layout.build().unwrap();
        assert_eq!(kinds(&state), vec![PaneKind::Sidebar, PaneKind::Reflog, PaneKind::Graph, PaneKind::Details]);
        let Some(Tree::Split { axis: SplitAxis::Columns, ratio, .. }) = layout.current() else { panic!() };
        assert!((ratio - 0.2).abs() < 1e-3, "left column keeps its saved width");
    }

    #[test]
    fn closing_and_reopening_keeps_the_rest() {
        let mut layout = Layout::default();
        layout.set_visible(PaneKind::Reflog, false);
        let without = layout.current().unwrap();
        assert_eq!(sorted(without.panes()), vec![PaneKind::Sidebar, PaneKind::Graph, PaneKind::Details, PaneKind::Session]);

        layout.set_visible(PaneKind::Reflog, true);
        let with = layout.current().unwrap();
        assert_eq!(sorted(with.panes()), PaneKind::ALL.to_vec());
        // The panels that stayed open keep their relative arrangement.
        assert_eq!(with.clone().without(PaneKind::Reflog), Some(without));
    }

    #[test]
    fn reopened_panel_splits_the_largest() {
        let tree = Tree::split(SplitAxis::Columns, 0.2, Tree::Pane(PaneKind::Sidebar), Tree::Pane(PaneKind::Graph));
        let tree = tree.with(PaneKind::Details, 16.0, 10.0);
        let Tree::Split { b, .. } = &tree else { panic!() };
        // Graph is 12.8 x 10: wider than tall, so the new panel goes beside it.
        assert_eq!(
            **b,
            Tree::split(SplitAxis::Columns, 0.5, Tree::Pane(PaneKind::Graph), Tree::Pane(PaneKind::Details))
        );
    }

    #[test]
    fn all_hidden_builds_nothing_and_one_reopens_alone() {
        let mut layout = Layout::default();
        for k in PaneKind::ALL {
            layout.set_visible(k, false);
        }
        assert!(layout.build().is_none());
        layout.set_visible(PaneKind::Graph, true);
        assert_eq!(layout.current(), Some(Tree::Pane(PaneKind::Graph)));
    }

    #[test]
    fn hidden_list_wins_over_a_stale_tree() {
        let layout =
            Layout { tree: Layout::default().current(), hidden: vec![PaneKind::Details], ..Layout::default() };
        let panes = sorted(layout.current().unwrap().panes());
        assert_eq!(panes, vec![PaneKind::Sidebar, PaneKind::Reflog, PaneKind::Graph, PaneKind::Session]);
    }

    #[test]
    fn reset_restores_the_default() {
        let mut layout = Layout::default();
        layout.set_visible(PaneKind::Graph, false);
        layout.reset();
        assert_eq!(layout, Layout::default());
    }
}
