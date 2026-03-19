use crate::graph_store::{CanonicalEntity, CanonicalGraphSnapshot};
use std::collections::{HashMap, HashSet, VecDeque};

/// An index over a canonical graph snapshot that enables efficient traversal.
pub struct GraphIndex<'a> {
    entities: HashMap<&'a str, &'a CanonicalEntity>,
    out_edges: HashMap<&'a str, Vec<&'a str>>,
    in_edges: HashMap<&'a str, Vec<&'a str>>,
    file_entities: HashMap<&'a str, Vec<&'a str>>,
}

impl<'a> GraphIndex<'a> {
    pub fn build(snapshot: &'a CanonicalGraphSnapshot) -> Self {
        let mut entities = HashMap::new();
        let mut file_entities: HashMap<&str, Vec<&str>> = HashMap::new();
        for entity in &snapshot.entities {
            entities.insert(entity.id.as_str(), entity);
            file_entities
                .entry(entity.file_path.as_str())
                .or_default()
                .push(entity.id.as_str());
        }

        let mut out_edges: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut in_edges: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in &snapshot.edges {
            out_edges
                .entry(edge.src_id.as_str())
                .or_default()
                .push(edge.dst_id.as_str());
            in_edges
                .entry(edge.dst_id.as_str())
                .or_default()
                .push(edge.src_id.as_str());
        }

        Self {
            entities,
            out_edges,
            in_edges,
            file_entities,
        }
    }

    /// Entities reachable in either direction within `max_hops`, excluding the seed itself.
    pub fn neighbors(&self, entity_id: &str, max_hops: usize) -> Vec<&'a CanonicalEntity> {
        self.bfs(&[entity_id], max_hops, true, true, true)
    }

    /// Entities this entity depends on (following out-edges), excluding the seed.
    pub fn upstream(&self, entity_id: &str, max_hops: usize) -> Vec<&'a CanonicalEntity> {
        self.bfs(&[entity_id], max_hops, true, false, true)
    }

    /// Entities that depend on this entity (following in-edges), excluding the seed.
    pub fn downstream(&self, entity_id: &str, max_hops: usize) -> Vec<&'a CanonicalEntity> {
        self.bfs(&[entity_id], max_hops, false, true, true)
    }

    /// All entities reachable (bidirectionally) from entities in the given file paths.
    /// Includes entities within those files as well as neighbors.
    pub fn blast_radius(&self, file_paths: &[&str], max_hops: usize) -> Vec<&'a CanonicalEntity> {
        let seeds: Vec<&str> = file_paths
            .iter()
            .flat_map(|p| {
                self.file_entities
                    .get(p)
                    .map(|ids| ids.as_slice())
                    .unwrap_or(&[])
            })
            .copied()
            .collect();
        self.bfs(&seeds, max_hops, true, true, false)
    }

    fn bfs(
        &self,
        seeds: &[&str],
        max_hops: usize,
        follow_out: bool,
        follow_in: bool,
        skip_seeds: bool,
    ) -> Vec<&'a CanonicalEntity> {
        let seed_set: HashSet<&str> = seeds.iter().copied().collect();
        let mut visited: HashSet<&str> = HashSet::new();
        let mut queue: VecDeque<(&str, usize)> = VecDeque::new();
        for &id in seeds {
            if visited.insert(id) {
                queue.push_back((id, 0));
            }
        }

        let mut result = Vec::new();
        while let Some((id, hop)) = queue.pop_front() {
            if let Some(&entity) = self.entities.get(id) {
                if !skip_seeds || !seed_set.contains(id) {
                    result.push(entity);
                }
            }
            if hop >= max_hops {
                continue;
            }
            if follow_out {
                for &next in self.out_edges.get(id).map(|v| v.as_slice()).unwrap_or(&[]) {
                    if visited.insert(next) {
                        queue.push_back((next, hop + 1));
                    }
                }
            }
            if follow_in {
                for &next in self.in_edges.get(id).map(|v| v.as_slice()).unwrap_or(&[]) {
                    if visited.insert(next) {
                        queue.push_back((next, hop + 1));
                    }
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_store::{CanonicalEdge, CanonicalEntityKind, CanonicalGraphSnapshot};

    fn entity(id: &str, file: &str, display: &str) -> CanonicalEntity {
        CanonicalEntity {
            id: id.to_string(),
            kind: CanonicalEntityKind::Function,
            name: id.to_string(),
            display_name: display.to_string(),
            file_path: file.to_string(),
            language: "rust".to_string(),
            parent_id: None,
            start_line: None,
            end_line: None,
            line_count: 0,
        }
    }

    fn edge(src: &str, dst: &str, kind: &str) -> CanonicalEdge {
        CanonicalEdge {
            src_id: src.to_string(),
            dst_id: dst.to_string(),
            kind: kind.to_string(),
            confidence: 1.0,
            source_file: src.to_string(),
        }
    }

    fn make_snapshot() -> CanonicalGraphSnapshot {
        // Graph:  A --call--> B --call--> C
        //                     B --call--> D
        // Files:  A in "src/a.rs", B/C/D in "src/b.rs"
        CanonicalGraphSnapshot {
            entities: vec![
                entity("A", "src/a.rs", "fn_a"),
                entity("B", "src/b.rs", "fn_b"),
                entity("C", "src/b.rs", "fn_c"),
                entity("D", "src/b.rs", "fn_d"),
            ],
            edges: vec![
                edge("A", "B", "call"),
                edge("B", "C", "call"),
                edge("B", "D", "call"),
            ],
            repo_meta: Default::default(),
        }
    }

    #[test]
    fn neighbors_returns_adjacent_entities_both_directions() {
        let snap = make_snapshot();
        let idx = GraphIndex::build(&snap);

        let result = idx.neighbors("B", 1);
        let ids: Vec<&str> = result.iter().map(|e| e.id.as_str()).collect();
        // B's neighbors: A (in-edge), C and D (out-edges)
        assert!(ids.contains(&"A"), "expected A, got {ids:?}");
        assert!(ids.contains(&"C"), "expected C, got {ids:?}");
        assert!(ids.contains(&"D"), "expected D, got {ids:?}");
        assert!(!ids.contains(&"B"), "seed B should not appear");
    }

    #[test]
    fn upstream_follows_out_edges_only() {
        let snap = make_snapshot();
        let idx = GraphIndex::build(&snap);

        let result = idx.upstream("A", 2);
        let ids: Vec<&str> = result.iter().map(|e| e.id.as_str()).collect();
        // A calls B, B calls C and D
        assert!(ids.contains(&"B"), "expected B");
        assert!(ids.contains(&"C"), "expected C");
        assert!(ids.contains(&"D"), "expected D");
        assert!(!ids.contains(&"A"), "seed A should not appear");
    }

    #[test]
    fn downstream_follows_in_edges_only() {
        let snap = make_snapshot();
        let idx = GraphIndex::build(&snap);

        let result = idx.downstream("C", 2);
        let ids: Vec<&str> = result.iter().map(|e| e.id.as_str()).collect();
        // C is called by B, B is called by A
        assert!(ids.contains(&"B"), "expected B");
        assert!(ids.contains(&"A"), "expected A");
        assert!(!ids.contains(&"C"), "seed C should not appear");
        assert!(!ids.contains(&"D"), "D not downstream of C");
    }

    #[test]
    fn blast_radius_from_file_includes_neighbors() {
        let snap = make_snapshot();
        let idx = GraphIndex::build(&snap);

        // blast from "src/a.rs" (contains A) with 1 hop
        let result = idx.blast_radius(&["src/a.rs"], 1);
        let ids: Vec<&str> = result.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"A"), "A is in the file");
        assert!(ids.contains(&"B"), "B is 1 hop from A");
        assert!(!ids.contains(&"C"), "C is 2 hops from A, beyond limit");
    }

    #[test]
    fn blast_radius_unknown_file_returns_empty() {
        let snap = make_snapshot();
        let idx = GraphIndex::build(&snap);
        let result = idx.blast_radius(&["src/nonexistent.rs"], 2);
        assert!(result.is_empty());
    }

    #[test]
    fn neighbors_max_hops_zero_returns_empty() {
        let snap = make_snapshot();
        let idx = GraphIndex::build(&snap);
        let result = idx.neighbors("A", 0);
        assert!(
            result.is_empty(),
            "max_hops=0 should yield nothing beyond seed"
        );
    }

    #[test]
    fn blast_radius_includes_seed_entities() {
        let snap = make_snapshot();
        let idx = GraphIndex::build(&snap);
        let result = idx.blast_radius(&["src/b.rs"], 0);
        let ids: Vec<&str> = result.iter().map(|e| e.id.as_str()).collect();
        // With 0 hops, only entities IN "src/b.rs" are returned
        assert!(ids.contains(&"B"));
        assert!(ids.contains(&"C"));
        assert!(ids.contains(&"D"));
        assert!(!ids.contains(&"A"));
    }
}
