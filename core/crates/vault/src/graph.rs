//! The graph of notes and their links, the whole vault's or around one
//! note, laid out (force-directed) so the apps only have to draw it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::Vault;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Note,
    /// A link to nothing in the vault: a note not written yet, or (`@…`)
    /// a paper in the library.
    Missing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GraphNode {
    /// The note's path, or a missing target as written.
    pub id: String,
    pub title: String,
    pub kind: NodeKind,
    /// How many links it has.
    pub degree: u32,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    /// Pairs of indexes into `nodes`, each link once.
    pub edges: Vec<(u32, u32)>,
}

impl Vault {
    /// Notes and the links between them; around `center` (within `depth`
    /// links either way) when given. Links to files (images, PDFs) are left
    /// out; links to missing notes and to papers are kept.
    pub fn graph(&self, center: Option<&str>, depth: usize) -> Graph {
        // Every link, as (from note, to note or missing target).
        let mut ids: BTreeMap<String, (String, NodeKind)> = BTreeMap::new();
        let mut links: BTreeSet<(String, String)> = BTreeSet::new();
        for note in self.notes() {
            ids.insert(note.path.clone(), (note.title.clone(), NodeKind::Note));
            for link in note.parsed.links.iter().filter(|l| !l.target.is_empty()) {
                let target = match self.resolve(&link.target, &note.path) {
                    Some(path) if self.note(&path).is_some() => path,
                    Some(_) => continue,
                    None => {
                        let key = format!("missing:{}", link.target.to_lowercase());
                        ids.entry(key.clone()).or_insert_with(|| (link.target.clone(), NodeKind::Missing));
                        key
                    }
                };
                if target != note.path {
                    let pair = if note.path < target {
                        (note.path.clone(), target)
                    } else {
                        (target, note.path.clone())
                    };
                    links.insert(pair);
                }
            }
        }
        let kept: BTreeSet<String> = match center.filter(|c| ids.contains_key(*c)) {
            Some(center) => around(center, depth, &links),
            None => ids.keys().cloned().collect(),
        };
        let index: BTreeMap<&String, u32> = kept.iter().enumerate().map(|(i, id)| (id, i as u32)).collect();
        let edges: Vec<(u32, u32)> =
            links.iter().filter_map(|(a, b)| Some((*index.get(a)?, *index.get(b)?))).collect();
        let mut degree = vec![0u32; kept.len()];
        for &(a, b) in &edges {
            degree[a as usize] += 1;
            degree[b as usize] += 1;
        }
        let positions = layout(kept.len(), &edges);
        let nodes = kept
            .iter()
            .zip(positions)
            .zip(degree)
            .map(|((id, (x, y)), degree)| {
                let (title, kind) = ids[id].clone();
                let id = if kind == NodeKind::Missing { title.clone() } else { id.clone() };
                GraphNode { id, title, kind, degree, x, y }
            })
            .collect();
        Graph { nodes, edges }
    }
}

/// The nodes within `depth` links of `center`.
fn around(center: &str, depth: usize, links: &BTreeSet<(String, String)>) -> BTreeSet<String> {
    let mut neighbours: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (a, b) in links {
        neighbours.entry(a).or_default().push(b);
        neighbours.entry(b).or_default().push(a);
    }
    let mut kept = BTreeSet::from([center.to_string()]);
    let mut queue = VecDeque::from([(center, 0)]);
    while let Some((id, distance)) = queue.pop_front() {
        if distance == depth {
            continue;
        }
        for next in neighbours.get(id).into_iter().flatten() {
            if kept.insert(next.to_string()) {
                queue.push_back((next, distance + 1));
            }
        }
    }
    kept
}

/// Fruchterman–Reingold: linked nodes pull together, all push apart, and a
/// little gravity keeps islands near. Starts from a spiral, so the same
/// graph always lands the same way.
fn layout(count: usize, edges: &[(u32, u32)]) -> Vec<(f32, f32)> {
    const SPACING: f32 = 30.0;
    let mut positions: Vec<(f32, f32)> = (0..count)
        .map(|i| {
            let radius = SPACING * (i as f32 + 0.5).sqrt();
            let angle = i as f32 * 2.399_963;
            (radius * angle.cos(), radius * angle.sin())
        })
        .collect();
    if count < 2 {
        return positions;
    }
    // Fewer rounds for big vaults: each round compares every pair.
    let rounds = (150_000_000 / (count * count)).clamp(15, 300);
    let mut temperature = SPACING * (count as f32).sqrt() / 4.0;
    let cooling = (1.0f32 / temperature).powf(1.0 / rounds as f32).min(0.99);
    let mut moves = vec![(0.0f32, 0.0f32); count];
    for _ in 0..rounds {
        moves.fill((0.0, 0.0));
        for i in 0..count {
            let (x, y) = positions[i];
            let mut pushed = (0.0, 0.0);
            for (j, &(other_x, other_y)) in positions.iter().enumerate().skip(i + 1) {
                let (dx, dy) = (x - other_x, y - other_y);
                let push = SPACING * SPACING / (dx * dx + dy * dy).max(0.01);
                pushed.0 += dx * push;
                pushed.1 += dy * push;
                moves[j].0 -= dx * push;
                moves[j].1 -= dy * push;
            }
            moves[i].0 += pushed.0;
            moves[i].1 += pushed.1;
        }
        for &(a, b) in edges {
            let (a, b) = (a as usize, b as usize);
            let (dx, dy) = (positions[a].0 - positions[b].0, positions[a].1 - positions[b].1);
            let pull = (dx * dx + dy * dy).sqrt() / SPACING;
            moves[a].0 -= dx * pull;
            moves[a].1 -= dy * pull;
            moves[b].0 += dx * pull;
            moves[b].1 += dy * pull;
        }
        for (position, step) in positions.iter_mut().zip(&moves) {
            let (mx, my) = (step.0 - position.0 * 0.05, step.1 - position.1 * 0.05);
            let length = (mx * mx + my * my).sqrt().max(0.01);
            let length_now = length.min(temperature);
            position.0 += mx / length * length_now;
            position.1 += my / length * length_now;
        }
        temperature *= cooling;
    }
    positions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_the_vault_and_around_a_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::open(dir.path()).unwrap();
        vault.write("A.md", "[[B]] [[Not yet]] ![[pic.png]] [[@kucsko2013]]").unwrap();
        vault.write("B.md", "[[C]] [[A]]").unwrap();
        vault.write("C.md", "").unwrap();
        vault.write("D.md", "alone").unwrap();
        std::fs::write(dir.path().join("pic.png"), b"png").unwrap();
        vault.refresh().unwrap();

        let all = vault.graph(None, 0);
        let titles: Vec<&str> = all.nodes.iter().map(|n| n.title.as_str()).collect();
        assert_eq!(titles, ["A", "B", "C", "D", "@kucsko2013", "Not yet"]);
        assert_eq!(all.edges.len(), 4, "A–B once, B–C, A–Not yet, A–@kucsko2013; no picture");
        assert_eq!(all.nodes[0].degree, 3);
        assert_eq!(all.nodes[4].kind, NodeKind::Missing);
        assert_eq!(all.nodes[4].id, "@kucsko2013");
        assert!(all.nodes.iter().all(|n| n.x.is_finite() && n.y.is_finite()));
        let distance =
            |a: usize, b: usize| (all.nodes[a].x - all.nodes[b].x).hypot(all.nodes[a].y - all.nodes[b].y);
        assert!(distance(1, 2) < distance(2, 3), "linked notes sit closer than strangers");

        let near: Vec<String> = vault.graph(Some("C.md"), 1).nodes.into_iter().map(|n| n.title).collect();
        assert_eq!(near, ["B", "C"]);
        assert_eq!(vault.graph(Some("C.md"), 2).nodes.len(), 3, "C, B, and A two links away");
        assert_eq!(vault.graph(Some("C.md"), 3).nodes.len(), 5, "and what A links to");
        assert_eq!(vault.graph(Some("Nowhere.md"), 1).nodes.len(), 6, "an unknown centre shows everything");
    }
}
