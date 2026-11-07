use sloposcope_lib::graph::{Edge, Func, Graph, Node};
use sloposcope_lib::program_model::{
    program_model_from_graph, EvidenceSource, FilesystemEntryKind, SyntaxEntityKind,
};
use std::path::Path;

#[test]
fn graph_becomes_separate_filesystem_syntax_and_static_relation_projections() {
    let graph = Graph {
        nodes: vec![
            Node {
                id: "src/lib/parser.rs".into(),
                label: "parser.rs (Parser)".into(),
                kind: "module".into(),
                file: "src/lib/parser.rs".into(),
                line_count: 80,
                change_status: "unchanged".into(),
                functions: vec![Func {
                    name: "parse_file".into(),
                    arity: 3,
                    kind: "function".into(),
                }],
            },
            Node {
                id: "src/model.rs".into(),
                label: "model.rs".into(),
                kind: "file".into(),
                file: "src/model.rs".into(),
                line_count: 20,
                change_status: "unchanged".into(),
                functions: vec![],
            },
        ],
        edges: vec![Edge {
            source: "src/lib/parser.rs".into(),
            target: "src/model.rs".into(),
            kind: "import".into(),
        }],
    };

    let model = program_model_from_graph(Path::new("/repo"), &graph, "abc123", 42);

    let root = model
        .filesystem
        .entries
        .iter()
        .find(|entry| entry.id == "fs-dir:.")
        .expect("repository root directory");
    assert_eq!(root.kind, FilesystemEntryKind::Directory);
    assert_eq!(root.parent_id, None);

    let src_lib = model
        .filesystem
        .entries
        .iter()
        .find(|entry| entry.id == "fs-dir:src/lib")
        .expect("nested directory");
    assert_eq!(src_lib.parent_id.as_deref(), Some("fs-dir:src"));

    let parser_file = model
        .filesystem
        .entries
        .iter()
        .find(|entry| entry.id == "fs-file:src/lib/parser.rs")
        .expect("physical parser file");
    assert_eq!(parser_file.kind, FilesystemEntryKind::File);
    assert_eq!(parser_file.parent_id.as_deref(), Some("fs-dir:src/lib"));

    let module = model
        .syntax
        .entities
        .iter()
        .find(|entity| entity.id == "syntax:src/lib/parser.rs#module:root")
        .expect("module syntax root");
    assert_eq!(module.kind, SyntaxEntityKind::Module);
    assert_eq!(module.file_id, "fs-file:src/lib/parser.rs");
    assert_eq!(module.parent_id, None);

    let function = model
        .syntax
        .entities
        .iter()
        .find(|entity| entity.name == "parse_file")
        .expect("function syntax child");
    assert_eq!(function.kind, SyntaxEntityKind::Function);
    assert_eq!(function.parent_id.as_deref(), Some(module.id.as_str()));
    assert_eq!(function.parameter_count, Some(3));

    let relation = model
        .static_relations
        .first()
        .expect("static import relation");
    assert_eq!(relation.src_id, "fs-file:src/lib/parser.rs");
    assert_eq!(relation.dst_id, "fs-file:src/model.rs");
    assert_eq!(relation.kind, "import");
    assert_eq!(relation.evidence, EvidenceSource::TreeSitter);
    assert_eq!(relation.confidence, 0.8);
}
