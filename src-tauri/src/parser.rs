use crate::graph::{Edge, Func, Node};
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Query, QueryCursor};

/// Every language parser implements this trait.
pub trait Parser: Send + Sync {
    fn extensions(&self) -> Vec<&'static str>;
    fn parse_file(&self, repo_root: &Path, relative_path: &str, content: &str) -> (Vec<Node>, Vec<Edge>);
}

/// A generic parser that uses Tree-sitter queries to extract nodes and edges.
pub struct PluggableParser {
    pub extensions: Vec<&'static str>,
    pub language: Language,
    pub queries: &'static str,
    pub default_node_kind: &'static str,
    pub default_edge_kind: &'static str,
}

impl Parser for PluggableParser {
    fn extensions(&self) -> Vec<&'static str> {
        self.extensions.clone()
    }

    fn parse_file(&self, _repo_root: &Path, relative_path: &str, content: &str) -> (Vec<Node>, Vec<Edge>) {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        let mut ts_parser = tree_sitter::Parser::new();
        ts_parser.set_language(&self.language).ok();

        let query = Query::new(&self.language, self.queries).ok();

        // Use the relative path itself as the fallback ID to keep extensions intact
        let fallback_node_id = relative_path.to_string();

        let tree = match ts_parser.parse(content, None) {
            Some(t) => t,
            None => return (nodes, edges),
        };

                    let mut node_label = fallback_node_id.clone();
                    let mut functions = Vec::new();
                    let mut file_edges = Vec::new();
        
                    if let Some(q) = &query {
                        let mut cursor = QueryCursor::new();
                        let mut matches = cursor.matches(q, tree.root_node(), content.as_bytes());
        
                        while let Some(m) = matches.next() {
                            let mut current_edge_target = None;
                            let mut current_edge_kind = self.default_edge_kind.to_string();
                            let mut current_func_name = None;
                            let mut current_func_kind = "def".to_string();
        
                            for cap in m.captures {
                                let capture_name = &q.capture_names()[cap.index as usize];
                                if let Ok(text) = cap.node.utf8_text(content.as_bytes()) {
                                    let clean_text = text.trim_matches(|c| c == '\'' || c == '"').to_string();
                                    match *capture_name {
                                        "node.name" => {
                                            if node_label == fallback_node_id {
                                                node_label = clean_text;
                                            }
                                        }
                                        "func.name" => {
                                            current_func_name = Some(clean_text);
                                        }
                                        "func.kind" => {
                                            current_func_kind = clean_text;
                                        }
                                        "edge.target" => {
                                            current_edge_target = Some(clean_text);
                                        }
                                        "edge.kind" => {
                                            current_edge_kind = clean_text;
                                        }
                                        _ => {}
                                    }
                                }
                            }
        
                            if let Some(target) = current_edge_target {
                                file_edges.push(Edge {
                                    source: fallback_node_id.clone(),
                                    target,
                                    kind: current_edge_kind,
                                });
                            }
        
                            if let Some(name) = current_func_name {
                                functions.push(Func {
                                    name,
                                    arity: 0,
                                    kind: current_func_kind,
                                });
                            }
                        }
                    }
        
                    nodes.push(Node {
                        id: fallback_node_id.clone(),
                        label: node_label,
                        kind: self.default_node_kind.into(),
                        file: relative_path.to_string(),
                        line_count: content.lines().count(),
                        change_status: "unchanged".into(),
                        functions,
                    });
                    edges.extend(file_edges);
                (nodes, edges)
    }
}
