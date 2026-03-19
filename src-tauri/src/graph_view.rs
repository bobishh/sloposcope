use crate::graph::{Func, Graph, Node};
use crate::graph_store::{
    canonical_entity_kind_label, CanonicalClusterMember, CanonicalEntity, CanonicalEntityKind,
    CanonicalGraphSnapshot,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphViewMode {
    Atlas,
    Investigation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphViewAggregate {
    File,
    Symbol,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GraphViewSeed {
    WorkingCopy,
    NodeIds { node_ids: Vec<String> },
    Paths { paths: Vec<String> },
    Commit { revset: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphViewRequest {
    pub mode: GraphViewMode,
    #[serde(default)]
    pub seed: Option<GraphViewSeed>,
    #[serde(default = "default_graph_depth")]
    pub depth: usize,
    #[serde(default = "default_min_confidence")]
    pub min_confidence: f64,
    #[serde(default = "default_graph_aggregate")]
    pub aggregate: GraphViewAggregate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeProvenance {
    pub node_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphClusterAssignment {
    pub node_id: String,
    pub cluster_id: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphViewResponse {
    pub graph: Graph,
    pub mode: GraphViewMode,
    pub aggregate: GraphViewAggregate,
    pub approximate_historical: bool,
    pub seed_label: Option<String>,
    pub node_provenance: Vec<GraphNodeProvenance>,
    pub canonical_entity_count: usize,
    pub canonical_edge_count: usize,
    pub clustering_mode: String,
    pub cluster_assignments: Vec<GraphClusterAssignment>,
}

pub struct GraphViewService;

impl GraphViewRequest {
    pub fn default_working_copy() -> Self {
        Self {
            mode: GraphViewMode::Investigation,
            seed: Some(GraphViewSeed::WorkingCopy),
            depth: 0,
            min_confidence: 0.0,
            aggregate: GraphViewAggregate::File,
        }
    }
}

impl GraphViewService {
    pub fn derive(
        snapshot: &CanonicalGraphSnapshot,
        request: &GraphViewRequest,
        changed_statuses: &HashMap<String, String>,
        no_vcs_mode: bool,
        approximate_historical: bool,
        clustering_mode: &str,
        cluster_members: &[CanonicalClusterMember],
    ) -> GraphViewResponse {
        snapshot_to_view(
            snapshot,
            request,
            changed_statuses,
            no_vcs_mode,
            approximate_historical,
            clustering_mode,
            cluster_members,
        )
    }
}

fn default_graph_depth() -> usize {
    0
}

fn default_min_confidence() -> f64 {
    0.0
}

fn default_graph_aggregate() -> GraphViewAggregate {
    GraphViewAggregate::File
}

fn snapshot_to_view(
    snapshot: &CanonicalGraphSnapshot,
    request: &GraphViewRequest,
    changed_statuses: &HashMap<String, String>,
    no_vcs_mode: bool,
    approximate_historical: bool,
    clustering_mode: &str,
    cluster_members: &[CanonicalClusterMember],
) -> GraphViewResponse {
    let entities_by_id = snapshot
        .entities
        .iter()
        .map(|entity| (entity.id.clone(), entity))
        .collect::<HashMap<_, _>>();
    let children_by_parent = build_children_map(&snapshot.entities);
    let roots = snapshot
        .entities
        .iter()
        .filter(|entity| entity.parent_id.is_none())
        .cloned()
        .collect::<Vec<_>>();
    let root_ids = roots
        .iter()
        .map(|entity| entity.id.clone())
        .collect::<HashSet<_>>();
    let root_for_entity = build_root_map(&snapshot.entities, &entities_by_id);

    let mut visible_ids = match request.mode {
        GraphViewMode::Atlas => match request.aggregate {
            GraphViewAggregate::File => root_ids.clone(),
            GraphViewAggregate::Symbol => snapshot
                .entities
                .iter()
                .map(|entity| entity.id.clone())
                .collect::<HashSet<_>>(),
        },
        GraphViewMode::Investigation => derive_investigation_ids(
            snapshot,
            request,
            changed_statuses,
            no_vcs_mode,
            &root_for_entity,
            &root_ids,
        ),
    };

    if visible_ids.is_empty() {
        visible_ids = match request.aggregate {
            GraphViewAggregate::File => root_ids.clone(),
            GraphViewAggregate::Symbol => snapshot
                .entities
                .iter()
                .map(|entity| entity.id.clone())
                .collect::<HashSet<_>>(),
        };
    }

    let seed_label = request.seed.as_ref().map(graph_view_seed_label);
    let provenance = if request.mode == GraphViewMode::Investigation {
        derive_provenance(
            snapshot,
            request,
            changed_statuses,
            no_vcs_mode,
            &root_for_entity,
            request.aggregate == GraphViewAggregate::File,
        )
    } else {
        Vec::new()
    };
    let provenance_map = provenance
        .iter()
        .map(|entry| (entry.node_id.clone(), entry.reason.clone()))
        .collect::<HashMap<_, _>>();

    let top_level_function_map = children_by_parent
        .iter()
        .map(|(parent, children)| {
            let funcs = children
                .iter()
                .filter(|entity| {
                    matches!(
                        entity.kind,
                        CanonicalEntityKind::Function | CanonicalEntityKind::Method
                    )
                })
                .map(|entity| Func {
                    name: entity.name.clone(),
                    arity: 0,
                    kind: canonical_entity_kind_label(&entity.kind).to_string(),
                })
                .collect::<Vec<_>>();
            (parent.clone(), funcs)
        })
        .collect::<HashMap<_, _>>();

    let nodes = match request.aggregate {
        GraphViewAggregate::File => roots
            .iter()
            .filter(|entity| visible_ids.contains(&entity.id))
            .map(|entity| Node {
                id: entity.id.clone(),
                label: entity.display_name.clone(),
                kind: canonical_entity_kind_label(&entity.kind).to_string(),
                file: entity.file_path.clone(),
                line_count: entity.line_count as usize,
                change_status: node_change_status(
                    &entity.file_path,
                    changed_statuses,
                    provenance_map.get(&entity.id),
                    no_vcs_mode,
                ),
                functions: top_level_function_map
                    .get(&entity.id)
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>(),
        GraphViewAggregate::Symbol => snapshot
            .entities
            .iter()
            .filter(|entity| visible_ids.contains(&entity.id))
            .map(|entity| Node {
                id: entity.id.clone(),
                label: entity.display_name.clone(),
                kind: canonical_entity_kind_label(&entity.kind).to_string(),
                file: entity.file_path.clone(),
                line_count: entity.line_count as usize,
                change_status: node_change_status(
                    &entity.file_path,
                    changed_statuses,
                    provenance_map.get(&entity.id),
                    no_vcs_mode,
                ),
                functions: Vec::new(),
            })
            .collect::<Vec<_>>(),
    };

    let mut node_ids = nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    let mut edges = Vec::new();
    for edge in &snapshot.edges {
        if edge.confidence + f64::EPSILON < request.min_confidence {
            continue;
        }
        let (src_id, dst_id) = match request.aggregate {
            GraphViewAggregate::File => {
                if edge.kind == "contains" {
                    continue;
                }
                let src_root = root_for_entity
                    .get(&edge.src_id)
                    .cloned()
                    .unwrap_or_else(|| edge.src_id.clone());
                let dst_root = root_for_entity
                    .get(&edge.dst_id)
                    .cloned()
                    .unwrap_or_else(|| edge.dst_id.clone());
                if src_root == dst_root {
                    continue;
                }
                (src_root, dst_root)
            }
            GraphViewAggregate::Symbol => (edge.src_id.clone(), edge.dst_id.clone()),
        };

        if !visible_ids.contains(&src_id) || !visible_ids.contains(&dst_id) {
            continue;
        }
        if !node_ids.contains(&src_id) || !node_ids.contains(&dst_id) {
            continue;
        }
        edges.push(crate::graph::Edge {
            source: src_id,
            target: dst_id,
            kind: edge.kind.clone(),
        });
    }

    let mut graph = Graph { nodes, edges };
    graph.finalize();
    node_ids = graph
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    let node_provenance = provenance
        .into_iter()
        .filter(|entry| node_ids.contains(&entry.node_id))
        .collect::<Vec<_>>();
    let cluster_assignments = derive_cluster_assignments(
        request.aggregate == GraphViewAggregate::File,
        &node_ids,
        &root_for_entity,
        cluster_members,
    );

    GraphViewResponse {
        graph,
        mode: request.mode.clone(),
        aggregate: request.aggregate.clone(),
        approximate_historical,
        seed_label,
        node_provenance,
        canonical_entity_count: snapshot.entities.len(),
        canonical_edge_count: snapshot.edges.len(),
        clustering_mode: clustering_mode.to_string(),
        cluster_assignments,
    }
}

fn derive_cluster_assignments(
    aggregate_file: bool,
    visible_node_ids: &HashSet<String>,
    root_for_entity: &HashMap<String, String>,
    cluster_members: &[CanonicalClusterMember],
) -> Vec<GraphClusterAssignment> {
    let mut counts = HashMap::<String, HashMap<String, usize>>::new();
    let mut labels = HashMap::<(String, String), Option<String>>::new();

    for member in cluster_members {
        let node_id = if aggregate_file {
            root_for_entity
                .get(&member.entity_id)
                .cloned()
                .unwrap_or_else(|| member.entity_id.clone())
        } else {
            member.entity_id.clone()
        };
        if !visible_node_ids.contains(&node_id) {
            continue;
        }
        counts
            .entry(node_id.clone())
            .or_default()
            .entry(member.cluster_id.clone())
            .and_modify(|count| *count += 1)
            .or_insert(1);
        labels.insert((node_id, member.cluster_id.clone()), member.label.clone());
    }

    let mut assignments = counts
        .into_iter()
        .filter_map(|(node_id, cluster_counts)| {
            let best = cluster_counts
                .into_iter()
                .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)))?;
            Some(GraphClusterAssignment {
                label: labels
                    .get(&(node_id.clone(), best.0.clone()))
                    .cloned()
                    .flatten(),
                node_id,
                cluster_id: best.0,
            })
        })
        .collect::<Vec<_>>();
    assignments.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    assignments
}

fn derive_investigation_ids(
    snapshot: &CanonicalGraphSnapshot,
    request: &GraphViewRequest,
    changed_statuses: &HashMap<String, String>,
    no_vcs_mode: bool,
    root_for_entity: &HashMap<String, String>,
    root_ids: &HashSet<String>,
) -> HashSet<String> {
    let aggregate_file = request.aggregate == GraphViewAggregate::File;
    let seed_ids = seed_entity_ids(
        snapshot,
        request.seed.as_ref(),
        changed_statuses,
        no_vcs_mode,
    );
    let normalized_seed_ids = seed_ids
        .into_iter()
        .map(|entity_id| {
            if aggregate_file {
                root_for_entity
                    .get(&entity_id)
                    .cloned()
                    .unwrap_or(entity_id)
            } else {
                entity_id
            }
        })
        .filter(|entity_id| !aggregate_file || root_ids.contains(entity_id))
        .collect::<HashSet<_>>();

    if request.depth == 0 {
        return normalized_seed_ids;
    }

    let adjacency = build_adjacency(snapshot, aggregate_file, root_for_entity);
    let mut visible = normalized_seed_ids.clone();
    let mut queue = normalized_seed_ids
        .iter()
        .cloned()
        .map(|node_id| (node_id, 0usize))
        .collect::<VecDeque<_>>();

    while let Some((node_id, depth)) = queue.pop_front() {
        if depth >= request.depth {
            continue;
        }
        let Some(neighbors) = adjacency.get(&node_id) else {
            continue;
        };
        for neighbor in neighbors {
            if visible.insert(neighbor.clone()) {
                queue.push_back((neighbor.clone(), depth + 1));
            }
        }
    }

    visible
}

fn derive_provenance(
    snapshot: &CanonicalGraphSnapshot,
    request: &GraphViewRequest,
    changed_statuses: &HashMap<String, String>,
    no_vcs_mode: bool,
    root_for_entity: &HashMap<String, String>,
    aggregate_file: bool,
) -> Vec<GraphNodeProvenance> {
    let raw_seed_ids = seed_entity_ids(
        snapshot,
        request.seed.as_ref(),
        changed_statuses,
        no_vcs_mode,
    );
    let seed_ids = raw_seed_ids
        .into_iter()
        .map(|entity_id| {
            if aggregate_file {
                root_for_entity
                    .get(&entity_id)
                    .cloned()
                    .unwrap_or(entity_id)
            } else {
                entity_id
            }
        })
        .collect::<HashSet<_>>();
    let adjacency = build_adjacency(snapshot, aggregate_file, root_for_entity);
    let mut distances = HashMap::new();
    let mut queue = VecDeque::new();
    for seed in &seed_ids {
        distances.insert(seed.clone(), 0usize);
        queue.push_back(seed.clone());
    }

    while let Some(node_id) = queue.pop_front() {
        let depth = *distances.get(&node_id).unwrap_or(&0);
        if depth >= request.depth {
            continue;
        }
        let Some(neighbors) = adjacency.get(&node_id) else {
            continue;
        };
        for neighbor in neighbors {
            if !distances.contains_key(neighbor) {
                distances.insert(neighbor.clone(), depth + 1);
                queue.push_back(neighbor.clone());
            }
        }
    }

    distances
        .into_iter()
        .map(|(node_id, depth)| GraphNodeProvenance {
            node_id,
            reason: if depth == 0 {
                match request.seed.as_ref() {
                    Some(GraphViewSeed::WorkingCopy) => "changed file".to_string(),
                    Some(GraphViewSeed::NodeIds { .. }) => "seed".to_string(),
                    Some(GraphViewSeed::Paths { .. }) => "seed".to_string(),
                    Some(GraphViewSeed::Commit { .. }) => "seed".to_string(),
                    None => "seed".to_string(),
                }
            } else if depth == 1 {
                "1-hop dependency".to_string()
            } else {
                format!("{depth}-hop impact")
            },
        })
        .collect()
}

fn seed_entity_ids(
    snapshot: &CanonicalGraphSnapshot,
    seed: Option<&GraphViewSeed>,
    changed_statuses: &HashMap<String, String>,
    no_vcs_mode: bool,
) -> HashSet<String> {
    match seed {
        Some(GraphViewSeed::WorkingCopy) | None => {
            if no_vcs_mode {
                snapshot
                    .entities
                    .iter()
                    .filter(|entity| entity.parent_id.is_none())
                    .map(|entity| entity.id.clone())
                    .collect()
            } else {
                let changed_files = changed_statuses.keys().cloned().collect::<HashSet<_>>();
                snapshot
                    .entities
                    .iter()
                    .filter(|entity| changed_files.contains(&entity.file_path))
                    .map(|entity| entity.id.clone())
                    .collect()
            }
        }
        Some(GraphViewSeed::NodeIds { node_ids }) => node_ids.iter().cloned().collect(),
        Some(GraphViewSeed::Paths { paths }) => {
            let normalized = paths.iter().cloned().collect::<HashSet<_>>();
            snapshot
                .entities
                .iter()
                .filter(|entity| normalized.contains(&entity.file_path))
                .map(|entity| entity.id.clone())
                .collect()
        }
        Some(GraphViewSeed::Commit { .. }) => {
            let changed_files = changed_statuses.keys().cloned().collect::<HashSet<_>>();
            snapshot
                .entities
                .iter()
                .filter(|entity| changed_files.contains(&entity.file_path))
                .map(|entity| entity.id.clone())
                .collect()
        }
    }
}

fn build_children_map(entities: &[CanonicalEntity]) -> HashMap<String, Vec<CanonicalEntity>> {
    let mut children = HashMap::new();
    for entity in entities {
        if let Some(parent_id) = &entity.parent_id {
            children
                .entry(parent_id.clone())
                .or_insert_with(Vec::new)
                .push(entity.clone());
        }
    }
    children
}

fn build_root_map<'a>(
    entities: &'a [CanonicalEntity],
    entities_by_id: &HashMap<String, &'a CanonicalEntity>,
) -> HashMap<String, String> {
    let mut roots = HashMap::new();
    for entity in entities {
        let mut current = entity;
        while let Some(parent_id) = &current.parent_id {
            let Some(parent) = entities_by_id.get(parent_id) else {
                break;
            };
            current = parent;
        }
        roots.insert(entity.id.clone(), current.id.clone());
    }
    roots
}

fn build_adjacency(
    snapshot: &CanonicalGraphSnapshot,
    aggregate_file: bool,
    root_for_entity: &HashMap<String, String>,
) -> HashMap<String, HashSet<String>> {
    let mut adjacency = HashMap::<String, HashSet<String>>::new();
    for edge in &snapshot.edges {
        let mut src = edge.src_id.clone();
        let mut dst = edge.dst_id.clone();
        if aggregate_file {
            if edge.kind == "contains" {
                continue;
            }
            src = root_for_entity
                .get(&edge.src_id)
                .cloned()
                .unwrap_or_else(|| edge.src_id.clone());
            dst = root_for_entity
                .get(&edge.dst_id)
                .cloned()
                .unwrap_or_else(|| edge.dst_id.clone());
        }
        if src == dst {
            continue;
        }
        adjacency
            .entry(src.clone())
            .or_default()
            .insert(dst.clone());
        adjacency.entry(dst).or_default().insert(src);
    }
    adjacency
}

fn node_change_status(
    file_path: &str,
    changed_statuses: &HashMap<String, String>,
    provenance: Option<&String>,
    no_vcs_mode: bool,
) -> String {
    if let Some(status) = changed_statuses.get(file_path) {
        return status.clone();
    }
    if no_vcs_mode {
        return "added".to_string();
    }
    if provenance.is_some() {
        return "neighbor".to_string();
    }
    "unchanged".to_string()
}

fn graph_view_seed_label(seed: &GraphViewSeed) -> String {
    match seed {
        GraphViewSeed::WorkingCopy => "working_copy".to_string(),
        GraphViewSeed::NodeIds { .. } => "node_ids".to_string(),
        GraphViewSeed::Paths { .. } => "paths".to_string(),
        GraphViewSeed::Commit { revset } => format!("commit:{revset}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_store::{CanonicalEdge, CanonicalEntityKind};

    #[test]
    fn investigation_view_includes_provenance_for_changed_file_and_neighbor() {
        let snapshot = CanonicalGraphSnapshot {
            entities: vec![
                CanonicalEntity {
                    id: "src/App.svelte".to_string(),
                    kind: CanonicalEntityKind::File,
                    name: "src/App.svelte".to_string(),
                    display_name: "App.svelte".to_string(),
                    file_path: "src/App.svelte".to_string(),
                    language: "svelte".to_string(),
                    parent_id: None,
                    start_line: Some(1),
                    end_line: Some(20),
                    line_count: 20,
                },
                CanonicalEntity {
                    id: "src/lib/backendBridge.js".to_string(),
                    kind: CanonicalEntityKind::File,
                    name: "src/lib/backendBridge.js".to_string(),
                    display_name: "backendBridge.js".to_string(),
                    file_path: "src/lib/backendBridge.js".to_string(),
                    language: "javascript".to_string(),
                    parent_id: None,
                    start_line: Some(1),
                    end_line: Some(10),
                    line_count: 10,
                },
            ],
            edges: vec![CanonicalEdge {
                src_id: "src/App.svelte".to_string(),
                dst_id: "src/lib/backendBridge.js".to_string(),
                kind: "import".to_string(),
                confidence: 0.8,
                source_file: "src/App.svelte".to_string(),
            }],
            repo_meta: HashMap::new(),
        };
        let request = GraphViewRequest {
            mode: GraphViewMode::Investigation,
            seed: Some(GraphViewSeed::WorkingCopy),
            depth: 1,
            min_confidence: 0.0,
            aggregate: GraphViewAggregate::File,
        };
        let view = GraphViewService::derive(
            &snapshot,
            &request,
            &HashMap::from([("src/App.svelte".to_string(), "modified".to_string())]),
            false,
            false,
            "heuristic",
            &[],
        );

        assert_eq!(view.graph.nodes.len(), 2);
        assert!(view
            .node_provenance
            .iter()
            .any(|entry| entry.node_id == "src/App.svelte" && entry.reason == "changed file"));
        assert!(view
            .node_provenance
            .iter()
            .any(|entry| entry.node_id == "src/lib/backendBridge.js"
                && entry.reason == "1-hop dependency"));
    }
}
