use ignore::WalkBuilder;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

/// A node is a named unit of code — a module, a component, a file.
/// What counts as a "node" is up to the language parser.
#[derive(Debug, Clone, Serialize)]
pub struct Node {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub file: String,
    pub line_count: usize,
    pub change_status: String,
    pub functions: Vec<Func>,
}

/// A function signature extracted from a node.
#[derive(Debug, Clone, Serialize)]
pub struct Func {
    pub name: String,
    pub arity: usize,
    #[serde(rename = "type")]
    pub kind: String,
}

/// An edge captures a relationship: this node references that node.
#[derive(Debug, Clone, Serialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub kind: String,
}

/// The whole graph, ready to serialize to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl Graph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_nodes(&mut self, nodes: Vec<Node>) {
        self.nodes.extend(nodes);
    }

    pub fn add_edges(&mut self, edges: Vec<Edge>) {
        self.edges.extend(edges);
    }

    /// Drop edges that point at nodes we don't have, self-loops,
    /// and duplicates. Call this once after all parsers have contributed.
    pub fn finalize(&mut self) {
        let known: HashSet<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        let mut seen: HashSet<(&str, &str, &str)> = HashSet::new();

        self.edges.retain(|e| {
            e.source != e.target
                && known.contains(e.source.as_str())
                && known.contains(e.target.as_str())
        });

        // Dedup in a second pass (borrow checker won't let us do it in one)
        let mut deduped = Vec::with_capacity(self.edges.len());
        for edge in &self.edges {
            let key = (
                edge.source.as_str(),
                edge.target.as_str(),
                edge.kind.as_str(),
            );
            if seen.insert(key) {
                deduped.push(edge.clone());
            }
        }
        self.edges = deduped;
    }
}

/// Fingerprint: the newest mtime across tracked source files in the repo.
/// Uses gitignore rules so build artifacts and vendored code are excluded.
pub fn source_fingerprint(repo: &Path) -> u64 {
    let mut newest: u64 = 0;
    let mut builder = WalkBuilder::new(repo);
    builder.hidden(false);
    builder.git_ignore(true);
    builder.git_exclude(true);
    builder.git_global(true);
    for entry in builder.build().filter_map(|e| e.ok()) {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let name = entry.file_name().to_str().unwrap_or("");
        if matches!(
            name,
            ".git" | "_build" | "deps" | "node_modules" | "target" | ".sloposcope"
        ) {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if let Ok(modified) = meta.modified() {
                let secs = modified
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                newest = newest.max(secs);
            }
        }
    }
    newest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalize_removes_unknown_and_duplicate_edges() {
        let mut graph = Graph {
            nodes: vec![
                Node {
                    id: "src/App.svelte".into(),
                    label: "App.svelte".into(),
                    kind: "file".into(),
                    file: "src/App.svelte".into(),
                    line_count: 10,
                    change_status: "unchanged".into(),
                    functions: vec![],
                },
                Node {
                    id: "src/lib/backendBridge.js".into(),
                    label: "backendBridge.js".into(),
                    kind: "file".into(),
                    file: "src/lib/backendBridge.js".into(),
                    line_count: 10,
                    change_status: "unchanged".into(),
                    functions: vec![],
                },
            ],
            edges: vec![
                Edge {
                    source: "src/App.svelte".into(),
                    target: "src/lib/backendBridge.js".into(),
                    kind: "import".into(),
                },
                Edge {
                    source: "src/App.svelte".into(),
                    target: "src/lib/backendBridge.js".into(),
                    kind: "import".into(),
                },
                Edge {
                    source: "src/App.svelte".into(),
                    target: "missing".into(),
                    kind: "import".into(),
                },
            ],
        };

        graph.finalize();

        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].target, "src/lib/backendBridge.js");
    }
}
