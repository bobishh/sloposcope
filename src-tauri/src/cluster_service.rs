use crate::graph_store::{
    CanonicalClusterMember, CanonicalEntity, CanonicalGraphSnapshot, GraphStore, SqliteGraphStore,
};
use crate::julia_sidecar;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterRefreshResult {
    pub mode: String,
    pub assignment_count: usize,
    pub stale: bool,
    pub error: Option<String>,
}

pub struct ClusterService;

impl ClusterService {
    pub fn refresh(repo: &Path) -> Result<ClusterRefreshResult, String> {
        let store = SqliteGraphStore::open_or_create(repo)?;
        let snapshot = store.load_snapshot()?;
        Self::refresh_from_snapshot(repo, &store, &snapshot, None)
    }

    pub fn mark_stale(repo: &Path) -> Result<ClusterRefreshResult, String> {
        let store = SqliteGraphStore::open_or_create(repo)?;
        store.replace_clusters(&[], "heuristic")?;
        Ok(ClusterRefreshResult {
            mode: "heuristic".to_string(),
            assignment_count: 0,
            stale: true,
            error: None,
        })
    }

    fn refresh_from_snapshot(
        repo: &Path,
        store: &SqliteGraphStore,
        snapshot: &CanonicalGraphSnapshot,
        julia_bin: Option<&str>,
    ) -> Result<ClusterRefreshResult, String> {
        if snapshot.entities.is_empty() {
            store.replace_clusters(&[], "heuristic")?;
            return Ok(ClusterRefreshResult {
                mode: "heuristic".to_string(),
                assignment_count: 0,
                stale: false,
                error: None,
            });
        }

        match julia_sidecar::cluster_snapshot(repo, snapshot, julia_bin) {
            Ok(members) => {
                store.replace_clusters(&members, "persisted")?;
                Ok(ClusterRefreshResult {
                    mode: "persisted".to_string(),
                    assignment_count: members.len(),
                    stale: false,
                    error: None,
                })
            }
            Err(julia_err) => {
                // Julia unavailable — fall back to Rust-native connected components.
                let members = compute_connected_components(snapshot);
                store.replace_clusters(&members, "persisted")?;
                Ok(ClusterRefreshResult {
                    mode: "persisted".to_string(),
                    assignment_count: members.len(),
                    stale: false,
                    error: Some(format!(
                        "julia unavailable ({julia_err}); used native fallback"
                    )),
                })
            }
        }
    }
}

/// Assign each entity to a connected component (undirected DFS).
/// Cluster IDs are `cluster-1`, `cluster-2`, … ordered by smallest entity ID in each component.
/// Labels are derived from the most common directory prefix and language within the component.
fn compute_connected_components(snapshot: &CanonicalGraphSnapshot) -> Vec<CanonicalClusterMember> {
    if snapshot.entities.is_empty() {
        return Vec::new();
    }

    let entity_set: HashSet<&str> = snapshot.entities.iter().map(|e| e.id.as_str()).collect();
    let mut adj: HashMap<&str, Vec<&str>> = snapshot
        .entities
        .iter()
        .map(|e| (e.id.as_str(), Vec::new()))
        .collect();
    for edge in &snapshot.edges {
        if edge.src_id == edge.dst_id {
            continue;
        }
        let src = edge.src_id.as_str();
        let dst = edge.dst_id.as_str();
        if !entity_set.contains(src) || !entity_set.contains(dst) {
            continue;
        }
        adj.entry(src).or_default().push(dst);
        adj.entry(dst).or_default().push(src);
    }

    let meta: HashMap<&str, &CanonicalEntity> = snapshot
        .entities
        .iter()
        .map(|e| (e.id.as_str(), e))
        .collect();

    let mut visited: HashSet<&str> = HashSet::new();
    let mut sorted_ids: Vec<&str> = entity_set.iter().copied().collect();
    sorted_ids.sort_unstable();

    let mut result = Vec::new();
    let mut cluster_index = 0usize;

    for &id in &sorted_ids {
        if visited.contains(id) {
            continue;
        }
        cluster_index += 1;
        let cluster_id = format!("cluster-{cluster_index}");
        let mut stack = vec![id];
        let mut members: Vec<&str> = Vec::new();
        while let Some(current) = stack.pop() {
            if visited.contains(current) {
                continue;
            }
            visited.insert(current);
            members.push(current);
            for &neighbor in adj.get(current).map(Vec::as_slice).unwrap_or(&[]) {
                if !visited.contains(neighbor) {
                    stack.push(neighbor);
                }
            }
        }
        members.sort_unstable();
        let label = derive_cluster_label(&members, &meta);
        for &entity_id in &members {
            result.push(CanonicalClusterMember {
                cluster_id: cluster_id.clone(),
                entity_id: entity_id.to_string(),
                label: Some(label.clone()),
            });
        }
    }

    result
}

/// Label a cluster as `"dir/subdir · language"` using the most common path prefix and language.
fn derive_cluster_label(members: &[&str], meta: &HashMap<&str, &CanonicalEntity>) -> String {
    let mut prefixes: HashMap<String, usize> = HashMap::new();
    let mut languages: HashMap<String, usize> = HashMap::new();

    for &id in members {
        if let Some(entity) = meta.get(id) {
            let parts: Vec<&str> = entity.file_path.split('/').collect();
            let prefix = if parts.len() >= 2 {
                format!("{}/{}", parts[0], parts[1])
            } else {
                entity.file_path.clone()
            };
            *prefixes.entry(prefix).or_default() += 1;
            *languages.entry(entity.language.clone()).or_default() += 1;
        }
    }

    // Higher count wins; among ties, lexicographically smaller key wins.
    let best_prefix = prefixes
        .into_iter()
        .max_by(|(ka, va), (kb, vb)| va.cmp(vb).then_with(|| kb.cmp(ka)))
        .map(|(k, _)| k)
        .unwrap_or_else(|| "graph".to_string());

    let best_lang = languages
        .into_iter()
        .max_by(|(ka, va), (kb, vb)| va.cmp(vb).then_with(|| kb.cmp(ka)))
        .map(|(k, _)| k);

    match best_lang {
        Some(lang) => format!("{best_prefix} · {lang}"),
        None => best_prefix,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_store::{
        CanonicalEdge, CanonicalEntity, CanonicalEntityKind, CanonicalGraphSnapshot,
    };
    use std::collections::HashMap;

    fn sample_snapshot() -> CanonicalGraphSnapshot {
        CanonicalGraphSnapshot {
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
                    end_line: Some(10),
                    line_count: 10,
                },
                CanonicalEntity {
                    id: "src/lib/Sloposcope.svelte".to_string(),
                    kind: CanonicalEntityKind::File,
                    name: "src/lib/Sloposcope.svelte".to_string(),
                    display_name: "Sloposcope.svelte".to_string(),
                    file_path: "src/lib/Sloposcope.svelte".to_string(),
                    language: "svelte".to_string(),
                    parent_id: None,
                    start_line: Some(1),
                    end_line: Some(10),
                    line_count: 10,
                },
            ],
            edges: vec![CanonicalEdge {
                src_id: "src/App.svelte".to_string(),
                dst_id: "src/lib/Sloposcope.svelte".to_string(),
                kind: "use".to_string(),
                confidence: 0.8,
                source_file: "src/App.svelte".to_string(),
            }],
            repo_meta: HashMap::new(),
        }
    }

    #[test]
    fn refresh_falls_back_to_native_when_julia_is_unavailable() {
        let repo = tempfile::tempdir().expect("tempdir");
        let store = SqliteGraphStore::open_or_create(repo.path()).expect("store");
        let snapshot = sample_snapshot();
        store.replace_all(&snapshot).expect("replace snapshot");

        let result = ClusterService::refresh_from_snapshot(
            repo.path(),
            &store,
            &snapshot,
            Some("/definitely/missing/julia"),
        )
        .expect("cluster refresh result");

        // Native fallback produces persisted clusters, not empty heuristic.
        assert_eq!(result.mode, "persisted");
        // The two entities share one edge → one connected component of size 2.
        assert_eq!(result.assignment_count, 2);
        // Julia error is preserved for debugging.
        assert!(result.error.is_some());

        let loaded = store.load_snapshot().expect("load snapshot");
        assert_eq!(
            loaded.repo_meta.get("clustering_mode").map(String::as_str),
            Some("persisted")
        );
        let clusters = store.load_clusters().expect("load clusters");
        assert_eq!(clusters.len(), 2);
        // Both entities land in the same cluster.
        let cluster_ids: Vec<&str> = clusters.iter().map(|m| m.cluster_id.as_str()).collect();
        assert!(cluster_ids.iter().all(|&c| c == cluster_ids[0]));
        // Label contains the common path prefix.
        assert!(clusters[0].label.as_deref().unwrap_or("").contains("src"));
    }

    #[test]
    fn native_clustering_disconnected_entities_form_separate_clusters() {
        let repo = tempfile::tempdir().expect("tempdir");
        let store = SqliteGraphStore::open_or_create(repo.path()).expect("store");
        let snapshot = CanonicalGraphSnapshot {
            entities: vec![
                CanonicalEntity {
                    id: "a/foo.rs".to_string(),
                    kind: CanonicalEntityKind::File,
                    name: "a/foo.rs".to_string(),
                    display_name: "foo.rs".to_string(),
                    file_path: "a/foo.rs".to_string(),
                    language: "rust".to_string(),
                    parent_id: None,
                    start_line: Some(1),
                    end_line: Some(5),
                    line_count: 5,
                },
                CanonicalEntity {
                    id: "b/bar.rs".to_string(),
                    kind: CanonicalEntityKind::File,
                    name: "b/bar.rs".to_string(),
                    display_name: "bar.rs".to_string(),
                    file_path: "b/bar.rs".to_string(),
                    language: "rust".to_string(),
                    parent_id: None,
                    start_line: Some(1),
                    end_line: Some(5),
                    line_count: 5,
                },
            ],
            edges: vec![],
            repo_meta: HashMap::new(),
        };
        store.replace_all(&snapshot).expect("replace snapshot");
        let members = compute_connected_components(&snapshot);
        // Two disconnected entities → two singleton clusters.
        assert_eq!(members.len(), 2);
        let ids: Vec<&str> = members.iter().map(|m| m.cluster_id.as_str()).collect();
        assert_ne!(
            ids[0], ids[1],
            "disconnected entities must be in separate clusters"
        );
    }
}
