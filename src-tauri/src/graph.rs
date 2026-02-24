use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use walkdir::WalkDir;

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
            let key = (edge.source.as_str(), edge.target.as_str(), edge.kind.as_str());
            if seen.insert(key) {
                deduped.push(edge.clone());
            }
        }
        self.edges = deduped;
    }
}

/// Fingerprint: the newest mtime across all files in the repo (ignoring .git, _build, etc).
pub fn source_fingerprint(repo: &Path) -> u64 {
    let mut newest: u64 = 0;
    for entry in WalkDir::new(repo)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_str().unwrap_or("");
            name != ".git"
                && name != "_build"
                && name != "deps"
                && name != "node_modules"
                && name != "target"
        })
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
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
    }
    newest
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_filter_to_changes_empty() {
        let mut g = Graph::new();
        g.add_nodes(vec![Node {
            id: "Foo".into(),
            label: "Foo".into(),
            kind: "module".into(),
            file: "lib/foo.ex".into(),
            line_count: 10,
            change_status: "unchanged".into(),
            functions: vec![],
        }]);
        
        let changed = HashMap::new();
        g.filter_to_changes(&changed);
        
        assert_eq!(g.nodes.len(), 0);
    }

    #[test]
    fn test_filter_to_changes_fallback() {
        let mut g = Graph::new();
        let mut changed = HashMap::new();
        changed.insert("config/env.exs".into(), "modified".into());
        
        g.filter_to_changes(&changed);
        
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].id, "config/env.exs");
        assert_eq!(g.nodes[0].label, "config/env.exs");
        assert_eq!(g.nodes[0].kind, "file");
    }
}

impl Graph {
    /// Keep only nodes whose file appears in the changed set.
    /// Mark each with its change status (added / modified / deleted).
    /// Also, if a file changed but no parser created a node for it,
    /// add a generic file node.
    pub fn filter_to_changes(&mut self, changed: &std::collections::HashMap<String, String>) {
        let mut parsed_files = HashSet::new();
        
        // Mark status on existing nodes and track which files were handled
        for node in &mut self.nodes {
            if let Some(status) = changed.get(&node.file) {
                node.change_status = status.clone();
                parsed_files.insert(node.file.clone());
            }
        }

        // ONLY keep nodes that appear in the 'changed' map.
        // If 'changed' is empty, this will effectively clear all nodes.
        self.nodes.retain(|n| changed.contains_key(&n.file));

        // For any changed file that was NOT handled by a parser, add a generic node.
        for (file, status) in changed {
            if !parsed_files.contains(file) && status != "deleted" {
                self.nodes.push(Node {
                    id: file.clone(),
                    label: file.clone(),
                    kind: "file".into(),
                    file: file.clone(),
                    line_count: 0,
                    change_status: status.clone(),
                    functions: vec![],
                });
            }
        }

        let remaining: HashSet<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        self.edges.retain(|e| {
            remaining.contains(e.source.as_str()) && remaining.contains(e.target.as_str())
        });
    }
}
