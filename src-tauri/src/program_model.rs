use crate::graph::{Graph, Node};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemEntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemEntry {
    pub id: String,
    pub kind: FilesystemEntryKind,
    pub path: String,
    pub name: String,
    pub display_name: String,
    pub parent_id: Option<String>,
    pub line_count: u32,
    pub language: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemHierarchy {
    pub entries: Vec<FilesystemEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyntaxEntityKind {
    Module,
    Class,
    Function,
    Method,
    Constant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntaxEntity {
    pub id: String,
    pub kind: SyntaxEntityKind,
    pub name: String,
    pub display_name: String,
    pub file_id: String,
    pub file_path: String,
    pub parent_id: Option<String>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub line_count: u32,
    pub parameter_count: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntaxHierarchy {
    pub entities: Vec<SyntaxEntity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    TreeSitter,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticRelation {
    pub src_id: String,
    pub dst_id: String,
    pub kind: String,
    pub confidence: f64,
    pub source_file: String,
    pub evidence: EvidenceSource,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramModelSnapshot {
    pub filesystem: FilesystemHierarchy,
    pub syntax: SyntaxHierarchy,
    pub static_relations: Vec<StaticRelation>,
    pub repo_meta: HashMap<String, String>,
}

pub fn program_model_from_graph(
    repo: &Path,
    graph: &Graph,
    checkout_revision: &str,
    build_fingerprint: u64,
) -> ProgramModelSnapshot {
    let filesystem = FilesystemHierarchy {
        entries: filesystem_entries_from_nodes(&graph.nodes),
    };
    let file_path_by_node_id = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), normalize_relative_path(&node.file)))
        .collect::<HashMap<_, _>>();

    let mut syntax_entities = Vec::new();
    for node in &graph.nodes {
        let file_path = normalize_relative_path(&node.file);
        if file_path.is_empty() {
            continue;
        }
        let file_id = filesystem_file_id(&file_path);
        let root = syntax_root_kind(&node.kind).map(|kind| {
            let id = format!(
                "syntax:{file_path}#{}:root",
                syntax_entity_kind_label(&kind)
            );
            syntax_entities.push(SyntaxEntity {
                id: id.clone(),
                kind,
                name: node.id.clone(),
                display_name: node.label.clone(),
                file_id: file_id.clone(),
                file_path: file_path.clone(),
                parent_id: None,
                start_line: Some(1),
                end_line: Some(node.line_count as u32),
                line_count: node.line_count as u32,
                parameter_count: None,
            });
            id
        });

        for (ordinal, function) in node.functions.iter().enumerate() {
            let kind = syntax_function_kind(&function.kind);
            syntax_entities.push(SyntaxEntity {
                id: format!(
                    "syntax:{file_path}#{}:{}:{ordinal}",
                    syntax_entity_kind_label(&kind),
                    function.name
                ),
                kind,
                name: function.name.clone(),
                display_name: function.name.clone(),
                file_id: file_id.clone(),
                file_path: file_path.clone(),
                parent_id: root.clone(),
                start_line: None,
                end_line: None,
                line_count: 0,
                parameter_count: Some(function.arity),
            });
        }
    }
    syntax_entities.sort_by(|left, right| left.id.cmp(&right.id));

    let mut static_relations = graph
        .edges
        .iter()
        .filter(|edge| edge.kind != "contains")
        .filter_map(|edge| {
            let source_file = file_path_by_node_id.get(edge.source.as_str())?;
            let target_file = file_path_by_node_id.get(edge.target.as_str())?;
            let kind = normalize_relation_kind(&edge.kind);
            Some(StaticRelation {
                src_id: filesystem_file_id(source_file),
                dst_id: filesystem_file_id(target_file),
                confidence: relation_confidence(&kind),
                kind,
                source_file: source_file.clone(),
                evidence: EvidenceSource::TreeSitter,
            })
        })
        .collect::<Vec<_>>();
    static_relations.sort_by(|left, right| {
        (&left.src_id, &left.dst_id, &left.kind).cmp(&(&right.src_id, &right.dst_id, &right.kind))
    });

    let repo_meta = HashMap::from([
        ("repo_path".to_string(), repo.display().to_string()),
        (
            "checkout_revision".to_string(),
            checkout_revision.to_string(),
        ),
        (
            "build_fingerprint".to_string(),
            build_fingerprint.to_string(),
        ),
        ("built_at".to_string(), unix_now_label()),
    ]);

    ProgramModelSnapshot {
        filesystem,
        syntax: SyntaxHierarchy {
            entities: syntax_entities,
        },
        static_relations,
        repo_meta,
    }
}

pub fn filesystem_file_id(path: &str) -> String {
    format!("fs-file:{}", normalize_relative_path(path))
}

fn syntax_root_kind(kind: &str) -> Option<SyntaxEntityKind> {
    match kind {
        "file" => None,
        "class" => Some(SyntaxEntityKind::Class),
        "constant" => Some(SyntaxEntityKind::Constant),
        _ => Some(SyntaxEntityKind::Module),
    }
}

fn syntax_function_kind(kind: &str) -> SyntaxEntityKind {
    if kind == "method" {
        SyntaxEntityKind::Method
    } else {
        SyntaxEntityKind::Function
    }
}

pub fn syntax_entity_kind_label(kind: &SyntaxEntityKind) -> &'static str {
    match kind {
        SyntaxEntityKind::Module => "module",
        SyntaxEntityKind::Class => "class",
        SyntaxEntityKind::Function => "function",
        SyntaxEntityKind::Method => "method",
        SyntaxEntityKind::Constant => "constant",
    }
}

fn normalize_relation_kind(kind: &str) -> String {
    match kind {
        "alias" => "reference",
        "require" => "use",
        other => other,
    }
    .to_string()
}

fn relation_confidence(kind: &str) -> f64 {
    match kind {
        "extends" => 0.95,
        "call" | "calls" => 0.9,
        "inherits" | "implements" => 0.85,
        "import" => 0.8,
        "use" | "using" | "include" => 0.75,
        "reference" => 0.6,
        "inferred_call" => 0.5,
        _ => 0.5,
    }
}

fn unix_now_label() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("unix:{seconds}")
}

fn filesystem_entries_from_nodes(nodes: &[Node]) -> Vec<FilesystemEntry> {
    let mut entries = BTreeMap::new();
    entries.insert(
        "fs-dir:.".to_string(),
        FilesystemEntry {
            id: "fs-dir:.".to_string(),
            kind: FilesystemEntryKind::Directory,
            path: ".".to_string(),
            name: ".".to_string(),
            display_name: ".".to_string(),
            parent_id: None,
            line_count: 0,
            language: "unknown".to_string(),
        },
    );

    for node in nodes {
        let path = normalize_relative_path(&node.file);
        if path.is_empty() {
            continue;
        }
        let parts = path.split('/').collect::<Vec<_>>();
        let mut directory_parts = Vec::new();
        for part in parts.iter().take(parts.len().saturating_sub(1)) {
            directory_parts.push(*part);
            let directory_path = directory_parts.join("/");
            let parent_id = if directory_parts.len() == 1 {
                "fs-dir:.".to_string()
            } else {
                format!(
                    "fs-dir:{}",
                    directory_parts[..directory_parts.len() - 1].join("/")
                )
            };
            let id = format!("fs-dir:{directory_path}");
            entries
                .entry(id.clone())
                .or_insert_with(|| FilesystemEntry {
                    id,
                    kind: FilesystemEntryKind::Directory,
                    path: directory_path,
                    name: (*part).to_string(),
                    display_name: (*part).to_string(),
                    parent_id: Some(parent_id),
                    line_count: 0,
                    language: "unknown".to_string(),
                });
        }

        let file_name = parts.last().copied().unwrap_or(path.as_str());
        let parent_id = if directory_parts.is_empty() {
            "fs-dir:.".to_string()
        } else {
            format!("fs-dir:{}", directory_parts.join("/"))
        };
        let id = format!("fs-file:{path}");
        entries.insert(
            id.clone(),
            FilesystemEntry {
                id,
                kind: FilesystemEntryKind::File,
                path: path.clone(),
                name: file_name.to_string(),
                display_name: node.label.clone(),
                parent_id: Some(parent_id),
                line_count: node.line_count as u32,
                language: language_from_path(&path),
            },
        );
    }

    entries.into_values().collect()
}

fn normalize_relative_path(path: &str) -> String {
    path.replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

fn language_from_path(path: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "ex" | "exs" => "elixir",
        "rb" | "rake" => "ruby",
        "js" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "py" => "python",
        "rs" => "rust",
        "go" => "go",
        "java" => "java",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "h" => "cpp",
        "php" => "php",
        "cs" => "csharp",
        "clj" | "cljs" | "cljc" => "clojure",
        "kt" | "kts" => "kotlin",
        "json" => "json",
        "md" => "markdown",
        "svelte" => "svelte",
        "" => "unknown",
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_paths_form_a_deterministic_filesystem_hierarchy() {
        let nodes = vec![Node {
            id: "src/lib/parser.rs".into(),
            label: "parser.rs".into(),
            kind: "module".into(),
            file: "src/lib/parser.rs".into(),
            line_count: 80,
            change_status: "unchanged".into(),
            functions: vec![],
        }];

        let entries = filesystem_entries_from_nodes(&nodes);
        let ids = entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec![
                "fs-dir:.",
                "fs-dir:src",
                "fs-dir:src/lib",
                "fs-file:src/lib/parser.rs",
            ]
        );
        assert_eq!(entries[1].parent_id.as_deref(), Some("fs-dir:."));
        assert_eq!(entries[2].parent_id.as_deref(), Some("fs-dir:src"));
        assert_eq!(entries[3].parent_id.as_deref(), Some("fs-dir:src/lib"));
    }

    #[test]
    fn plain_file_functions_are_syntax_without_an_invented_container() {
        let graph = Graph {
            nodes: vec![Node {
                id: "script.js".into(),
                label: "script.js".into(),
                kind: "file".into(),
                file: "script.js".into(),
                line_count: 4,
                change_status: "unchanged".into(),
                functions: vec![crate::graph::Func {
                    name: "run".into(),
                    arity: 1,
                    kind: "function".into(),
                }],
            }],
            edges: vec![],
        };

        let model = program_model_from_graph(Path::new("/repo"), &graph, "head", 7);

        assert_eq!(model.syntax.entities.len(), 1);
        assert_eq!(model.syntax.entities[0].name, "run");
        assert_eq!(model.syntax.entities[0].parent_id, None);
        assert_eq!(model.syntax.entities[0].file_id, "fs-file:script.js");
    }
}
