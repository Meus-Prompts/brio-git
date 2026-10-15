//! Panel layout: which panels are visible and how much room each one takes.
//!
//! The layout is kept as plain fractions of the window so it survives
//! closing/reopening panels and can be saved to disk. The `pane_grid` state is
//! rebuilt from it whenever the set of visible panels changes, and the
//! fractions are read back after the user drags a divider.

use iced::Size;
use iced::widget::pane_grid::{self, Axis, Configuration};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PaneKind {
    Sidebar,
    Graph,
    Details,
    Session,
}

impl PaneKind {
    pub const ALL: [PaneKind; 4] = [PaneKind::Sidebar, PaneKind::Graph, PaneKind::Details, PaneKind::Session];

    pub fn title(self) -> &'static str {
        match self {
            PaneKind::Sidebar => "Branches",
            PaneKind::Graph => "Grafo",
            PaneKind::Details => "Alterações",
            PaneKind::Session => "Sessão de IA",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// Width of the branch list, as a fraction of the window width.
    pub sidebar: f32,
    /// Width of the details panel.
    pub details: f32,
    /// Height of the AI session panel, as a fraction of the window height.
    pub session: f32,
    pub hidden: Vec<PaneKind>,
}

impl Default for Layout {
    fn default() -> Self {
        Self { sidebar: 0.16, details: 0.32, session: 0.36, hidden: Vec::new() }
    }
}

const MIN_FRACTION: f32 = 0.05;

impl Layout {
    pub fn is_visible(&self, kind: PaneKind) -> bool {
        !self.hidden.contains(&kind)
    }

    pub fn set_visible(&mut self, kind: PaneKind, visible: bool) {
        self.hidden.retain(|k| *k != kind);
        if !visible {
            self.hidden.push(kind);
        }
    }

    /// Builds the pane grid for the visible panels, or `None` if all are closed.
    pub fn build(&self) -> Option<pane_grid::State<PaneKind>> {
        let top: Vec<(PaneKind, f32)> = [
            (PaneKind::Sidebar, self.sidebar),
            (PaneKind::Graph, (1.0 - self.sidebar - self.details).max(0.2)),
            (PaneKind::Details, self.details),
        ]
        .into_iter()
        .filter(|(k, _)| self.is_visible(*k))
        .collect();

        let top = row(&top);
        let config = match (top, self.is_visible(PaneKind::Session)) {
            (Some(top), true) => Configuration::Split {
                axis: Axis::Horizontal,
                ratio: (1.0 - self.session).clamp(MIN_FRACTION, 1.0 - MIN_FRACTION),
                a: Box::new(top),
                b: Box::new(Configuration::Pane(PaneKind::Session)),
            },
            (Some(top), false) => top,
            (None, true) => Configuration::Pane(PaneKind::Session),
            (None, false) => return None,
        };
        Some(pane_grid::State::with_configuration(config))
    }

    /// Reads the panel sizes back from the grid after a resize.
    pub fn capture(&mut self, state: &pane_grid::State<PaneKind>) {
        // Regions are rounded to whole pixels, so measure on a large virtual size.
        const SCALE: f32 = 100_000.0;
        let regions = state.layout().pane_regions(0.0, 0.0, Size::new(SCALE, SCALE));
        for (pane, rect) in regions {
            match state.get(pane) {
                Some(PaneKind::Sidebar) => self.sidebar = rect.width / SCALE,
                Some(PaneKind::Details) => self.details = rect.width / SCALE,
                Some(PaneKind::Session) if self.has_top() => self.session = rect.height / SCALE,
                _ => {}
            }
        }
    }

    fn has_top(&self) -> bool {
        [PaneKind::Sidebar, PaneKind::Graph, PaneKind::Details].iter().any(|k| self.is_visible(*k))
    }
}

/// Panels side by side, each taking its share of the summed widths.
fn row(panes: &[(PaneKind, f32)]) -> Option<Configuration<PaneKind>> {
    let ((first, width), rest) = panes.split_first()?;
    if rest.is_empty() {
        return Some(Configuration::Pane(*first));
    }
    let total: f32 = panes.iter().map(|(_, w)| w).sum();
    Some(Configuration::Split {
        axis: Axis::Vertical,
        ratio: (width / total).clamp(MIN_FRACTION, 1.0 - MIN_FRACTION),
        a: Box::new(Configuration::Pane(*first)),
        b: Box::new(row(rest)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(state: &pane_grid::State<PaneKind>) -> Vec<PaneKind> {
        let mut k: Vec<_> = state.iter().map(|(_, k)| *k).collect();
        k.sort_by_key(|k| PaneKind::ALL.iter().position(|a| a == k));
        k
    }

    #[test]
    fn default_layout_round_trips_through_the_grid() {
        let layout = Layout::default();
        let state = layout.build().unwrap();
        assert_eq!(kinds(&state), PaneKind::ALL.to_vec());

        let mut read = Layout { sidebar: 0.0, details: 0.0, session: 0.0, hidden: Vec::new() };
        read.capture(&state);
        assert!((read.sidebar - layout.sidebar).abs() < 1e-3, "{read:?}");
        assert!((read.details - layout.details).abs() < 1e-3, "{read:?}");
        assert!((read.session - layout.session).abs() < 1e-3, "{read:?}");
    }

    #[test]
    fn hidden_panels_are_left_out_and_sizes_kept() {
        let mut layout = Layout::default();
        layout.set_visible(PaneKind::Sidebar, false);
        layout.set_visible(PaneKind::Session, false);
        let state = layout.build().unwrap();
        assert_eq!(kinds(&state), vec![PaneKind::Graph, PaneKind::Details]);

        layout.capture(&state);
        assert_eq!(layout.sidebar, Layout::default().sidebar, "hidden panel keeps its size");
        assert!((layout.details - 0.32 / (1.0 - 0.16)).abs() < 1e-3, "details grows into freed room");

        layout.set_visible(PaneKind::Sidebar, true);
        assert_eq!(kinds(&layout.build().unwrap()), vec![PaneKind::Sidebar, PaneKind::Graph, PaneKind::Details]);
    }

    #[test]
    fn all_hidden_builds_nothing() {
        let layout = Layout { hidden: PaneKind::ALL.to_vec(), ..Layout::default() };
        assert!(layout.build().is_none());
    }

    #[test]
    fn only_session_visible() {
        let mut layout = Layout::default();
        for k in [PaneKind::Sidebar, PaneKind::Graph, PaneKind::Details] {
            layout.set_visible(k, false);
        }
        let state = layout.build().unwrap();
        assert_eq!(kinds(&state), vec![PaneKind::Session]);
        layout.capture(&state);
        assert_eq!(layout.session, Layout::default().session, "full-height session does not overwrite its size");
    }
}
