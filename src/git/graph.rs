//! Layout do grafo de commits em "lanes" (colunas).
//!
//! Os commits chegam em ordem topológica (filhos antes dos pais). Cada lane
//! guarda o próximo commit que ela espera encontrar. Para cada linha `r`
//! registramos as arestas do intervalo `(r-1) -> r` em `edges_up`; assim a UI
//! virtualizada desenha cada linha visível sem olhar o resto do histórico.

use git2::Oid;

pub const PALETTE_SIZE: u8 = 10;

#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub from: u16,
    pub to: u16,
    pub color: u8,
}

#[derive(Clone, Debug, Default)]
pub struct RowGraph {
    pub col: u16,
    pub color: u8,
    pub edges_up: Vec<Edge>,
}

#[derive(Clone, Debug, Default)]
pub struct GraphLayout {
    pub rows: Vec<RowGraph>,
    pub max_cols: u16,
}

#[derive(Clone, Copy)]
struct Lane {
    oid: Oid,
    color: u8,
    from: u16,
}

/// Aresta extra de merge para um pai que já tem lane própria; ela pousa na
/// posição daquela lane (ou no commit, se for a próxima linha).
struct PendingLink {
    from: u16,
    oid: Oid,
    color: u8,
}

pub fn layout(commits: &[(Oid, Vec<Oid>)]) -> GraphLayout {
    let mut lanes: Vec<Option<Lane>> = Vec::new();
    let mut pending: Vec<PendingLink> = Vec::new();
    let mut rows = Vec::with_capacity(commits.len());
    let mut next_color: u8 = 0;
    let mut max_cols: u16 = 1;

    let take_color = |next_color: &mut u8| {
        let c = *next_color;
        *next_color = (*next_color + 1) % PALETTE_SIZE;
        c
    };

    for (oid, parents) in commits {
        let matches: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter_map(|(i, l)| l.filter(|l| l.oid == *oid).map(|_| i))
            .collect();

        let col = match matches.first() {
            Some(&c) => c,
            None => free_slot(&mut lanes),
        };
        let color = match matches.first() {
            Some(&c) => lanes[c].unwrap().color,
            None => take_color(&mut next_color),
        };

        // Arestas que chegam nesta linha vindas da anterior.
        let mut edges_up = Vec::new();
        for (i, lane) in lanes.iter().enumerate() {
            if let Some(lane) = lane {
                let to = if lane.oid == *oid { col } else { i };
                edges_up.push(Edge { from: lane.from, to: to as u16, color: lane.color });
            }
        }
        for link in pending.drain(..) {
            let to = if link.oid == *oid {
                Some(col)
            } else {
                lanes.iter().position(|l| l.is_some_and(|l| l.oid == link.oid))
            };
            if let Some(to) = to {
                edges_up.push(Edge { from: link.from, to: to as u16, color: link.color });
            }
        }

        for m in &matches {
            lanes[*m] = None;
        }
        for (i, lane) in lanes.iter_mut().enumerate() {
            if let Some(lane) = lane {
                lane.from = i as u16;
            }
        }

        if let Some(first) = parents.first() {
            lanes[col] = Some(Lane { oid: *first, color, from: col as u16 });
            for parent in &parents[1..] {
                let existing = lanes
                    .iter()
                    .enumerate()
                    .find(|(i, l)| *i != col && l.is_some_and(|l| l.oid == *parent))
                    .map(|(_, l)| l.unwrap().color);
                match existing {
                    Some(c) => pending.push(PendingLink { from: col as u16, oid: *parent, color: c }),
                    None => {
                        let slot = free_slot(&mut lanes);
                        let c = take_color(&mut next_color);
                        lanes[slot] = Some(Lane { oid: *parent, color: c, from: col as u16 });
                    }
                }
            }
        }

        while matches!(lanes.last(), Some(None)) {
            lanes.pop();
        }
        max_cols = max_cols.max(lanes.len() as u16).max(col as u16 + 1);
        rows.push(RowGraph { col: col as u16, color, edges_up });
    }

    GraphLayout { rows, max_cols }
}

fn free_slot(lanes: &mut Vec<Option<Lane>>) -> usize {
    match lanes.iter().position(Option::is_none) {
        Some(i) => i,
        None => {
            lanes.push(None);
            lanes.len() - 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(n: u8) -> Oid {
        Oid::from_bytes(&[n; 20]).unwrap()
    }

    #[test]
    fn linear_history_uses_single_lane() {
        let commits = vec![(oid(3), vec![oid(2)]), (oid(2), vec![oid(1)]), (oid(1), vec![])];
        let g = layout(&commits);
        assert_eq!(g.max_cols, 1);
        assert!(g.rows.iter().all(|r| r.col == 0));
        assert_eq!(g.rows[1].edges_up.len(), 1);
    }

    #[test]
    fn merge_opens_and_closes_second_lane() {
        // 4 = merge(3, 2); 3 -> 1; 2 -> 1
        let commits = vec![
            (oid(4), vec![oid(3), oid(2)]),
            (oid(3), vec![oid(1)]),
            (oid(2), vec![oid(1)]),
            (oid(1), vec![]),
        ];
        let g = layout(&commits);
        assert_eq!(g.max_cols, 2);
        assert_eq!(g.rows[2].col, 1);
        // Na raiz, as duas lanes convergem para a coluna 0.
        let last = &g.rows[3];
        assert_eq!(last.col, 0);
        assert!(last.edges_up.iter().any(|e| e.from == 1 && e.to == 0));
    }

    #[test]
    fn merge_into_tracked_parent_reuses_lane() {
        // 5 -> 4(merge 3,2); topo de outro branch 6 -> 2
        let commits = vec![
            (oid(6), vec![oid(2)]),
            (oid(4), vec![oid(3), oid(2)]),
            (oid(3), vec![oid(2)]),
            (oid(2), vec![]),
        ];
        let g = layout(&commits);
        assert_eq!(g.max_cols, 2);
        assert!(g.rows[2].edges_up.iter().any(|e| e.from == 1 && e.to == 0));
    }
}
