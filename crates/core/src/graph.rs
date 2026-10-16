//! Lane layout for the commit graph.
//!
//! [`layout`] turns a list of commits (newest first) into one [`GraphRow`] per
//! commit. Each row says in which lane the commit dot sits and which line
//! segments cross the row, so the renderer only has to draw straight lines.

use std::collections::{HashMap, HashSet};

use crate::model::Commit;

/// A line segment inside one row. Lanes are column indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    /// From the top edge of the row (at `from`) into the commit dot (at `to`).
    IntoNode { from: usize, to: usize, color: usize },
    /// From the commit dot (at `from`) to the bottom edge of the row (at `to`).
    OutOfNode { from: usize, to: usize, color: usize },
    /// A line crossing the row without touching the dot. It enters at the top
    /// at `from` and leaves at the bottom at `to` (equal unless the lane shifts).
    Through { from: usize, to: usize, color: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    /// Lane of the commit dot.
    pub lane: usize,
    /// Color index of the commit dot.
    pub color: usize,
    pub segments: Vec<Segment>,
    /// Number of lanes this row needs to be drawn.
    pub width: usize,
}

#[derive(Clone)]
struct Lane {
    /// The commit this lane is waiting for.
    expects: String,
    color: usize,
}

/// Computes the graph rows for `commits`, which must be ordered so that every
/// commit comes before its parents (newest first).
pub fn layout(commits: &[Commit]) -> Vec<GraphRow> {
    layout_pinned(commits, None)
}

/// Like [`layout`], but keeps the first-parent line of `pinned` (a commit id,
/// usually a branch tip) in the leftmost lane.
pub fn layout_pinned(commits: &[Commit], pinned: Option<&str>) -> Vec<GraphRow> {
    let chain = first_parent_chain(commits, pinned);
    let reserve = !chain.is_empty();
    let mut lanes: Vec<Option<Lane>> = Vec::new();
    let mut next_color = 0usize;
    let mut rows = Vec::with_capacity(commits.len());

    for commit in commits {
        let on_chain = chain.contains(commit.id.as_str());
        let width_before = lanes.len();
        let mut segments = Vec::new();

        let matching: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter(|(_, l)| l.as_ref().is_some_and(|l| l.expects == commit.id))
            .map(|(i, _)| i)
            .collect();

        let (lane, color) = match matching.first() {
            // The pinned line always sits in lane 0.
            _ if on_chain && !matching.contains(&0) => {
                if lanes.is_empty() {
                    lanes.push(None);
                }
                let color = next_color;
                next_color += 1;
                (0, color)
            }
            Some(&i) => (i, lanes[i].as_ref().map_or(0, |l| l.color)),
            None => {
                let color = next_color;
                next_color += 1;
                (free_slot(&mut lanes, reserve), color)
            }
        };

        if !matching.contains(&lane) {
            // Release the slot reserved above; the parents decide who takes it.
            lanes[lane] = None;
        }

        for (i, slot) in lanes.iter().enumerate() {
            if let Some(l) = slot {
                if matching.contains(&i) {
                    segments.push(Segment::IntoNode { from: i, to: lane, color: l.color });
                } else {
                    segments.push(Segment::Through { from: i, to: i, color: l.color });
                }
            }
        }
        for &i in &matching {
            lanes[i] = None;
        }

        for (n, parent) in commit.parents.iter().enumerate() {
            let waiting = lanes
                .iter()
                .position(|l| l.as_ref().is_some_and(|l| &l.expects == parent));
            match waiting {
                // A lane further right waits for our first parent: pull it into
                // our lane so the main line keeps its column.
                Some(j) if n == 0 && j > lane => {
                    for s in &mut segments {
                        if let Segment::Through { from, to, .. } = s
                            && *from == j
                        {
                            *to = lane;
                        }
                    }
                    lanes[j] = None;
                    lanes[lane] = Some(Lane { expects: parent.clone(), color });
                    segments.push(Segment::OutOfNode { from: lane, to: lane, color });
                }
                // Another branch already waits for this parent: merge into it.
                Some(j) => {
                    let c = if n == 0 { color } else { lanes[j].as_ref().map_or(color, |l| l.color) };
                    segments.push(Segment::OutOfNode { from: lane, to: j, color: c });
                }
                None => {
                    let (target, c) = if n == 0 {
                        (lane, color)
                    } else {
                        let c = next_color;
                        next_color += 1;
                        (free_slot(&mut lanes, reserve), c)
                    };
                    lanes[target] = Some(Lane { expects: parent.clone(), color: c });
                    segments.push(Segment::OutOfNode { from: lane, to: target, color: c });
                }
            }
        }

        while matches!(lanes.last(), Some(None)) {
            lanes.pop();
        }

        let width = width_before.max(lanes.len()).max(lane + 1);
        rows.push(GraphRow { lane, color, segments, width });
    }

    rows
}

/// Ids on the first-parent line starting at `tip`.
fn first_parent_chain<'a>(commits: &'a [Commit], tip: Option<&str>) -> HashSet<&'a str> {
    let by_id: HashMap<&str, &Commit> = commits.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut chain = HashSet::new();
    let mut next = tip.and_then(|t| by_id.get(t).copied());
    while let Some(c) = next {
        if !chain.insert(c.id.as_str()) {
            break;
        }
        next = c.parents.first().and_then(|p| by_id.get(p.as_str()).copied());
    }
    chain
}

/// Returns the first empty lane, appending one when all are taken. The slot is
/// reserved (set to a placeholder) so a second call does not return it again.
/// With `skip_first`, lane 0 is left for the pinned line.
fn free_slot(lanes: &mut Vec<Option<Lane>>, skip_first: bool) -> usize {
    let placeholder = Some(Lane { expects: String::new(), color: 0 });
    let start = usize::from(skip_first);
    while lanes.len() < start {
        lanes.push(None);
    }
    match lanes.iter().skip(start).position(Option::is_none).map(|i| i + start) {
        Some(i) => {
            lanes[i] = placeholder;
            i
        }
        None => {
            lanes.push(placeholder);
            lanes.len() - 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, parents: &[&str]) -> Commit {
        Commit {
            id: id.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            summary: String::new(),
            author: String::new(),
            email: String::new(),
            time: 0,
        }
    }

    fn lanes(rows: &[GraphRow]) -> Vec<usize> {
        rows.iter().map(|r| r.lane).collect()
    }

    #[test]
    fn linear_history_stays_in_one_lane() {
        let rows = layout(&[c("c", &["b"]), c("b", &["a"]), c("a", &[])]);
        assert_eq!(lanes(&rows), vec![0, 0, 0]);
        assert!(rows.iter().all(|r| r.width == 1));
        assert_eq!(rows[0].segments, vec![Segment::OutOfNode { from: 0, to: 0, color: 0 }]);
        assert_eq!(rows[2].segments, vec![Segment::IntoNode { from: 0, to: 0, color: 0 }]);
    }

    #[test]
    fn merge_commit_opens_lane_for_second_parent_and_closes_it() {
        // m merges b into the line a <- m.
        let rows = layout(&[c("m", &["a", "b"]), c("b", &["a"]), c("a", &[])]);
        assert_eq!(lanes(&rows), vec![0, 1, 0]);
        assert!(rows[0].segments.contains(&Segment::OutOfNode { from: 0, to: 1, color: 1 }));
        // b flows back into lane 0, which already waits for a.
        assert!(rows[1].segments.contains(&Segment::OutOfNode { from: 1, to: 0, color: 1 }));
        assert!(rows[1].segments.contains(&Segment::Through { from: 0, to: 0, color: 0 }));
        assert_eq!(rows[2].width, 1);
    }

    #[test]
    fn two_tips_sharing_a_parent_converge() {
        let rows = layout(&[c("x", &["a"]), c("y", &["a"]), c("a", &[])]);
        assert_eq!(lanes(&rows), vec![0, 1, 0]);
        assert!(rows[1].segments.contains(&Segment::OutOfNode { from: 1, to: 0, color: 1 }));
    }

    #[test]
    fn freed_lanes_are_reused() {
        let rows = layout(&[
            c("m2", &["m1", "f2"]),
            c("f2", &["base"]),
            c("m1", &["base", "f1"]),
            c("f1", &["base"]),
            c("base", &[]),
        ]);
        assert_eq!(lanes(&rows), vec![0, 1, 0, 1, 0]);
        assert!(rows.iter().all(|r| r.width <= 2));
        // m1's first parent was awaited by lane 1; that lane shifts into lane 0.
        assert!(rows[2].segments.contains(&Segment::Through { from: 1, to: 0, color: 1 }));
    }

    #[test]
    fn pinned_branch_takes_the_first_lane() {
        // Two branches off "base"; "x" is newer so it would normally take lane 0.
        let commits = [c("x", &["base"]), c("y2", &["y1"]), c("y1", &["base"]), c("base", &[])];
        assert_eq!(lanes(&layout(&commits)), vec![0, 1, 1, 0]);

        let rows = layout_pinned(&commits, Some("y2"));
        assert_eq!(lanes(&rows), vec![1, 0, 0, 0]);
        // x's line joins the pinned line where they share a parent.
        assert!(rows[2].segments.contains(&Segment::Through { from: 1, to: 0, color: 0 }), "{:?}", rows[2].segments);
    }

    #[test]
    fn pinning_an_unknown_commit_changes_nothing() {
        let commits = [c("b", &["a"]), c("a", &[])];
        assert_eq!(layout_pinned(&commits, Some("zzz")), layout(&commits));
    }

    #[test]
    fn commit_with_missing_parents_keeps_lane_open() {
        // History cut by a log limit: the parent never shows up.
        let rows = layout(&[c("b", &["a"])]);
        assert_eq!(rows[0].segments, vec![Segment::OutOfNode { from: 0, to: 0, color: 0 }]);
    }
}
