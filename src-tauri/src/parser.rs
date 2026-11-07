use crate::graph::{Edge, Func, Node};
use serde::Serialize;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Query, QueryCursor};

#[path = "parser_metrics.rs"]
mod parser_metrics;
pub(crate) use parser_metrics::source_id;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnatomyStatus {
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallableVisibility {
    Public,
    Private,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallableAnatomy {
    pub id: String,
    pub match_key: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub kind: String,
    pub visibility: CallableVisibility,
    pub arity: Option<usize>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub body_line_span: Option<u32>,
    pub decision_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSlotAnatomy {
    pub name: String,
    pub kind: String,
    pub visibility: CallableVisibility,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAnatomy {
    pub status: AnatomyStatus,
    pub file: String,
    pub language: String,
    pub source_id: String,
    pub profile_id: String,
    pub line_count: Option<u32>,
    pub first_seen_at: Option<String>,
    pub age_as_of: Option<String>,
    pub age_days: Option<u32>,
    pub age_reason: Option<String>,
    pub evidence: String,
    pub callables: Vec<CallableAnatomy>,
    pub state_slots: Option<Vec<StateSlotAnatomy>>,
    pub reason: Option<String>,
}

impl FileAnatomy {
    pub fn unavailable(file: &str, language: &str, reason: &str) -> Self {
        Self {
            status: AnatomyStatus::Unavailable,
            file: file.to_string(),
            language: language.to_string(),
            source_id: String::new(),
            profile_id: format!("{language}:unavailable"),
            line_count: None,
            first_seen_at: None,
            age_as_of: None,
            age_days: None,
            age_reason: None,
            evidence: "tree_sitter".to_string(),
            callables: Vec::new(),
            state_slots: None,
            reason: Some(reason.to_string()),
        }
    }
}

/// Every language parser implements this trait.
pub trait Parser: Send + Sync {
    fn extensions(&self) -> Vec<&'static str>;
    fn parse_file(
        &self,
        repo_root: &Path,
        relative_path: &str,
        content: &str,
    ) -> (Vec<Node>, Vec<Edge>);

    fn analyze_file(&self, repo_root: &Path, relative_path: &str, content: &str) -> FileAnatomy {
        let language = Path::new(relative_path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown");
        let _ = (repo_root, content);
        let mut anatomy =
            FileAnatomy::unavailable(relative_path, language, "anatomy profile unavailable");
        anatomy.source_id = parser_metrics::source_id(content.as_bytes());
        anatomy.line_count = Some(physical_line_count(content.as_bytes()));
        anatomy
    }
}

pub(crate) fn physical_line_count(content: &[u8]) -> u32 {
    let line_feeds = content.iter().filter(|byte| **byte == b'\n').count() as u64;
    let trailing_line = u64::from(!content.is_empty() && content.last() != Some(&b'\n'));
    line_feeds
        .saturating_add(trailing_line)
        .min(u32::MAX as u64) as u32
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

    fn parse_file(
        &self,
        _repo_root: &Path,
        relative_path: &str,
        content: &str,
    ) -> (Vec<Node>, Vec<Edge>) {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        let mut ts_parser = tree_sitter::Parser::new();
        ts_parser.set_language(&self.language).ok();

        let query = Query::new(&self.language, self.queries).ok();

        // Use the relative path itself as the fallback ID to keep extensions intact
        let fallback_node_id = relative_path.to_string();
        let filename = Path::new(relative_path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(relative_path);

        let tree = match ts_parser.parse(content, None) {
            Some(t) => t,
            None => return (nodes, edges),
        };

        let mut extracted_name = None;
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
                let mut current_calls_receiver = None;
                let mut current_calls_method = None;

                for cap in m.captures {
                    let capture_name = &q.capture_names()[cap.index as usize];
                    if let Ok(text) = cap.node.utf8_text(content.as_bytes()) {
                        let clean_text = text.trim_matches(|c| c == '\'' || c == '"').to_string();
                        // Capture names of the form "<kind>.target" (e.g. `@extends.target`,
                        // `@implements.target`) directly emit an edge with that static kind,
                        // bypassing the per-match current_edge_target accumulator.
                        if capture_name.ends_with(".target") && *capture_name != "edge.target" {
                            let kind = &capture_name[..capture_name.len() - ".target".len()];
                            file_edges.push(Edge {
                                source: fallback_node_id.clone(),
                                target: clean_text,
                                kind: kind.to_string(),
                            });
                            continue;
                        }
                        match *capture_name {
                            "node.name" => {
                                if extracted_name.is_none() {
                                    extracted_name = Some(clean_text);
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
                            "calls.receiver" => {
                                current_calls_receiver = Some(clean_text);
                            }
                            "calls.method" => {
                                current_calls_method = Some(clean_text);
                            }
                            _ => {}
                        }
                    }
                }

                // Compound call-graph capture: @calls.receiver + @calls.method
                // → emit a "calls" edge with target "Receiver.method".
                if let (Some(receiver), Some(method)) =
                    (current_calls_receiver, current_calls_method)
                {
                    file_edges.push(Edge {
                        source: fallback_node_id.clone(),
                        target: format!("{receiver}.{method}"),
                        kind: "calls".to_string(),
                    });
                } else if let Some(target) = current_edge_target {
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

        let node_label = match extracted_name {
            Some(name) if name != filename => format!("{} ({})", filename, name),
            _ => filename.to_string(),
        };

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

    fn analyze_file(&self, repo_root: &Path, relative_path: &str, content: &str) -> FileAnatomy {
        let extension = Path::new(relative_path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown")
            .to_ascii_lowercase();
        let _ = repo_root;
        parser_metrics::analyze(relative_path, &extension, self.language.clone(), content)
    }
}

#[cfg(test)]
mod tests {
    /// Given callable declarations in each supported grammar, when anatomy is
    /// extracted, then body spans and decision counts come from source syntax.
    #[test]
    fn measured_anatomy_captures_ranges_and_decisions_across_language_families() {
        use super::{Parser as _, PluggableParser};
        use std::path::Path;

        let cases = [
            (
                "rs",
                tree_sitter::Language::new(tree_sitter_rust::LANGUAGE),
                "fn f(x: bool) {\n if x {\n  return;\n }\n}\n",
                "f",
                5,
                1,
            ),
            (
                "rb",
                tree_sitter::Language::new(tree_sitter_ruby::LANGUAGE),
                "def f(x)\n  if x\n    return\n  end\nend\n",
                "f",
                3,
                1,
            ),
            (
                "js",
                tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
                "function f(x) {\n if (x) {\n  return;\n }\n}\n",
                "f",
                5,
                1,
            ),
            (
                "ts",
                tree_sitter::Language::new(tree_sitter_typescript::LANGUAGE_TYPESCRIPT),
                "function f(x: boolean) {\n if (x) {\n  return;\n }\n}\n",
                "f",
                5,
                1,
            ),
            (
                "ex",
                tree_sitter::Language::new(tree_sitter_elixir::LANGUAGE),
                "defmodule M do\n def f(x) do\n  if x do\n   :ok\n  end\n end\nend\n",
                "f",
                5,
                1,
            ),
            (
                "elm",
                tree_sitter::Language::new(tree_sitter_elm::LANGUAGE),
                "f x =\n    if x then 1 else 0\n",
                "f",
                1,
                1,
            ),
        ];

        for (extension, language, source, expected_name, expected_span, expected_decisions) in cases
        {
            let parser = PluggableParser {
                extensions: vec!["test"],
                language,
                queries: "",
                default_node_kind: "file",
                default_edge_kind: "call",
            };
            let anatomy =
                parser.analyze_file(Path::new("/repo"), &format!("sample.{extension}"), source);
            assert_eq!(
                anatomy.status,
                super::AnatomyStatus::Partial,
                ".{extension}"
            );
            assert!(anatomy.source_id.starts_with("sha256:"), ".{extension}");
            assert!(
                anatomy.profile_id.ends_with("callable-anatomy-v1"),
                ".{extension}"
            );
            let callable = anatomy
                .callables
                .iter()
                .find(|callable| callable.name == expected_name)
                .unwrap_or_else(|| {
                    panic!("missing {expected_name} callable for .{extension}: {anatomy:?}")
                });
            assert_eq!(callable.body_line_span, Some(expected_span), ".{extension}");
            assert_eq!(
                callable.decision_count,
                Some(expected_decisions),
                ".{extension}"
            );
            assert!(callable.start_line.is_some(), ".{extension}");
            assert!(callable.end_line.is_some(), ".{extension}");
        }
    }

    #[test]
    fn anatomy_preserves_same_name_methods_from_distinct_source_captures() {
        let parser = super::PluggableParser {
            extensions: vec!["js"],
            language: tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
            queries: "(method_definition name: (property_identifier) @func.name)",
            default_node_kind: "file",
            default_edge_kind: "call",
        };
        let anatomy = super::Parser::analyze_file(
            &parser,
            std::path::Path::new("."),
            "classes.js",
            "class A { run() {} } class B { run() {} }",
        );
        assert_eq!(
            anatomy.callables.len(),
            2,
            "Distinct method captures must not merge by name"
        );
        assert!(anatomy.callables.iter().all(|c| c.name == "run"));
    }
    use super::*;

    #[test]
    fn elixir_anatomy_preserves_captured_visibility_and_unknown_facts() {
        let parser = PluggableParser {
            extensions: vec!["ex", "exs"],
            language: tree_sitter::Language::new(tree_sitter_elixir::LANGUAGE),
            default_node_kind: "module",
            default_edge_kind: "call",
            queries: r#"
                (call target: (identifier) @func.kind (#match? @func.kind "^(def|defp)$") (arguments [(identifier) @func.name (call target: (identifier) @func.name)]))
            "#,
        };

        let anatomy = parser.analyze_file(
            Path::new("/repo"),
            "lib/sample.ex",
            "defmodule Sample do\n  def shown(x), do: x\n  defp hidden, do: :ok\nend\n",
        );

        assert_eq!(anatomy.status, AnatomyStatus::Partial);
        assert_eq!(anatomy.evidence, "tree_sitter");
        assert_eq!(anatomy.state_slots, None);
        assert!(
            anatomy.callables.iter().any(|callable| {
                callable.name == "shown"
                    && callable.visibility == CallableVisibility::Public
                    && callable.arity == Some(1)
                    && callable.start_line.is_some()
            }),
            "{anatomy:?}"
        );
        assert!(anatomy.callables.iter().any(|callable| {
            callable.name == "hidden" && callable.visibility == CallableVisibility::Private
        }));
    }

    // ── Outer RED: named-kind captures produce correct edge kinds ─────────────

    /// Parsing a JS class with `extends` must yield an "extends" edge, not a
    /// generic "import" edge.  This test drives the `@<kind>.target` capture
    /// convention into `PluggableParser::parse_file`.
    #[test]
    fn js_class_extends_emits_extends_edge() {
        let parser = PluggableParser {
            extensions: vec!["js"],
            language: tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
            default_node_kind: "class",
            default_edge_kind: "import",
            // class_heritage is an anonymous child of class_declaration;
            // it wraps the superclass expression.
            queries: r#"
                (class_declaration name: (identifier) @node.name
                    (class_heritage (identifier) @extends.target))
            "#,
        };
        let (_, edges) = parser.parse_file(
            Path::new("/repo"),
            "src/animal.js",
            "class Dog extends Animal {}",
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "extends" && e.target == "Animal"),
            "expected an 'extends' edge to 'Animal', got: {edges:?}"
        );
        // must NOT emit a generic import edge for a heritage capture
        assert!(
            !edges.iter().any(|e| e.kind == "import"),
            "should not emit import edges for heritage: {edges:?}"
        );
    }

    /// Python class bases must be extracted with kind "extends".
    #[test]
    fn python_class_base_emits_extends_edge() {
        let parser = PluggableParser {
            extensions: vec!["py"],
            language: tree_sitter::Language::new(tree_sitter_python::LANGUAGE),
            default_node_kind: "module",
            default_edge_kind: "import",
            // tree-sitter-python: superclasses is a field on class_definition
            queries: r#"
                (class_definition name: (identifier) @node.name
                    superclasses: (argument_list (identifier) @extends.target))
            "#,
        };
        let (_, edges) = parser.parse_file(
            Path::new("/repo"),
            "src/dog.py",
            "class Dog(Animal):\n    pass\n",
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "extends" && e.target == "Animal"),
            "expected extends edge to Animal, got: {edges:?}"
        );
    }

    /// Ruby superclass must be extracted with kind "extends".
    #[test]
    fn ruby_superclass_emits_extends_edge() {
        let parser = PluggableParser {
            extensions: vec!["rb"],
            language: tree_sitter::Language::new(tree_sitter_ruby::LANGUAGE),
            default_node_kind: "class",
            default_edge_kind: "call",
            // AST: (class (superclass (constant)))  ← "superclass" is a named node
            queries: r#"
                (class name: [(constant) @node.name (scope_resolution) @node.name])
                (class (superclass (constant) @extends.target))
                (class (superclass (scope_resolution) @extends.target))
            "#,
        };
        let (_, edges) = parser.parse_file(
            Path::new("/repo"),
            "app/models/dog.rb",
            "class Dog < Animal\nend\n",
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "extends" && e.target == "Animal"),
            "expected extends edge to Animal, got: {edges:?}"
        );
    }

    /// Java extends/implements produce distinct edge kinds.
    #[test]
    fn java_heritage_emits_extends_and_implements_edges() {
        let parser = PluggableParser {
            extensions: vec!["java"],
            language: tree_sitter::Language::new(tree_sitter_java::LANGUAGE),
            default_node_kind: "class",
            default_edge_kind: "import",
            // AST: (class_declaration (superclass (type_identifier)))
            //      (class_declaration (super_interfaces (type_list (type_identifier))))
            queries: r#"
                (class_declaration name: (identifier) @node.name)
                (class_declaration (superclass (type_identifier) @extends.target))
                (class_declaration (super_interfaces (type_list (type_identifier) @implements.target)))
            "#,
        };
        let (_, edges) = parser.parse_file(
            Path::new("/repo"),
            "src/Dog.java",
            "public class Dog extends Animal implements Runnable {}",
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "extends" && e.target == "Animal"),
            "expected extends edge to Animal, got: {edges:?}"
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "implements" && e.target == "Runnable"),
            "expected implements edge to Runnable, got: {edges:?}"
        );
    }

    /// PHP extends/implements produce distinct edge kinds.
    #[test]
    fn php_heritage_emits_extends_and_implements_edges() {
        let parser = PluggableParser {
            extensions: vec!["php"],
            language: tree_sitter::Language::new(tree_sitter_php::LANGUAGE_PHP),
            default_node_kind: "class",
            default_edge_kind: "use",
            // PHP AST: name node type is 'name' (not 'identifier')
            // (base_clause (name)) for extends, (class_interface_clause (name)) for implements
            queries: r#"
                (class_declaration name: (name) @node.name)
                (class_declaration (base_clause (name) @extends.target))
                (class_declaration (class_interface_clause (name) @implements.target))
            "#,
        };
        let (_, edges) = parser.parse_file(
            Path::new("/repo"),
            "src/Dog.php",
            "<?php class Dog extends Animal implements Runnable {}",
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "extends" && e.target == "Animal"),
            "expected extends edge to Animal, got: {edges:?}"
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "implements" && e.target == "Runnable"),
            "expected implements edge to Runnable, got: {edges:?}"
        );
    }

    // ── Call-graph edges: receiver type inference ──────────────────────────────

    /// Elixir `Module.function()` calls must produce "calls" edges with
    /// target "Module.function".
    #[test]
    fn elixir_dot_call_emits_calls_edge() {
        let parser = PluggableParser {
            extensions: vec!["ex"],
            language: tree_sitter::Language::new(tree_sitter_elixir::LANGUAGE),
            default_node_kind: "module",
            default_edge_kind: "call",
            queries: r#"
                (call target: (identifier) @kw (#eq? @kw "defmodule") (arguments (alias) @node.name))
                (call target: (identifier) @func.kind (#match? @func.kind "^(def|defp)$") (arguments [(identifier) @func.name (call target: (identifier) @func.name)]))
                (call target: (dot left: (alias) @calls.receiver right: (identifier) @calls.method))
                (call target: (identifier) @edge.kind (#match? @edge.kind "^(use|import|alias)$") (arguments (alias) @edge.target))
            "#,
        };
        let (_, edges) = parser.parse_file(
            Path::new("/repo"),
            "lib/user_service.ex",
            r#"
defmodule MyApp.UserService do
  def create_user(attrs) do
    Repo.insert(changeset)
  end
end
"#,
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "calls" && e.target == "Repo.insert"),
            "expected a 'calls' edge to 'Repo.insert', got: {edges:?}"
        );
    }

    /// Ruby `Receiver.method()` calls must produce "calls" edges with
    /// target "Receiver.method".
    #[test]
    fn ruby_receiver_call_emits_calls_edge() {
        let parser = PluggableParser {
            extensions: vec!["rb"],
            language: tree_sitter::Language::new(tree_sitter_ruby::LANGUAGE),
            default_node_kind: "class",
            default_edge_kind: "call",
            queries: r#"
                (module name: [(constant) @node.name (scope_resolution) @node.name])
                (class name: [(constant) @node.name (scope_resolution) @node.name])
                (class (superclass (constant) @extends.target))
                (class (superclass (scope_resolution) @extends.target))
                (method name: (identifier) @func.name)
                (call receiver: (constant) @calls.receiver method: (identifier) @calls.method)
            "#,
        };
        let (_, edges) = parser.parse_file(
            Path::new("/repo"),
            "app/models/user_service.rb",
            r#"
class UserService
  def create_user(attrs)
    User.create(attrs)
  end
end
"#,
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "calls" && e.target == "User.create"),
            "expected a 'calls' edge to 'User.create', got: {edges:?}"
        );
    }

    /// Elixir call-graph edges must NOT emit duplicate generic "call" edges
    /// for receiver-qualified calls.
    #[test]
    fn elixir_dot_call_does_not_emit_generic_call_edge() {
        let parser = PluggableParser {
            extensions: vec!["ex"],
            language: tree_sitter::Language::new(tree_sitter_elixir::LANGUAGE),
            default_node_kind: "module",
            default_edge_kind: "call",
            queries: r#"
                (call target: (dot left: (alias) @calls.receiver right: (identifier) @calls.method))
            "#,
        };
        let (_, edges) =
            parser.parse_file(Path::new("/repo"), "lib/svc.ex", "Repo.insert(changeset)");
        assert!(
            !edges.iter().any(|e| e.kind == "call"),
            "should not emit generic 'call' edges for receiver-qualified calls: {edges:?}"
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "calls" && e.target == "Repo.insert"),
            "expected 'calls' edge, got: {edges:?}"
        );
    }

    /// TypeScript `implements` clause produces "implements" edges.
    /// Uses tree-sitter-typescript (not tree-sitter-javascript) which has implements_clause.
    #[test]
    fn ts_class_implements_emits_implements_edge() {
        let parser = PluggableParser {
            extensions: vec!["ts"],
            language: tree_sitter::Language::new(tree_sitter_typescript::LANGUAGE_TYPESCRIPT),
            default_node_kind: "lib",
            default_edge_kind: "import",
            queries: r#"
                (class_declaration name: (type_identifier) @node.name)
                (class_declaration (class_heritage (extends_clause (identifier) @extends.target)))
                (class_declaration (class_heritage (implements_clause (type_identifier) @implements.target)))
            "#,
        };
        let (_, edges) = parser.parse_file(
            Path::new("/repo"),
            "src/dog.ts",
            "class Foo extends Bar implements Baz {}",
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "implements" && e.target == "Baz"),
            "expected implements edge to Baz, got: {edges:?}"
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "extends" && e.target == "Bar"),
            "expected extends edge to Bar, got: {edges:?}"
        );
    }

    /// TS/JS re-export `export { X } from './foo'` produces an "import" edge.
    #[test]
    fn ts_export_reexport_emits_import_edge() {
        let parser = PluggableParser {
            extensions: vec!["ts"],
            language: tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
            default_node_kind: "lib",
            default_edge_kind: "import",
            queries: r#"
                (import_statement source: (string (string_fragment) @edge.target))
                (export_statement source: (string (string_fragment) @edge.target))
            "#,
        };
        let (_, edges) = parser.parse_file(
            Path::new("/repo"),
            "src/index.ts",
            "export { Foo } from './foo'",
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "import" && e.target == "./foo"),
            "expected import edge to './foo', got: {edges:?}"
        );
    }

    /// Rust `impl Display for Point` produces an "implements" edge to the trait.
    #[test]
    fn rust_impl_trait_emits_implements_edge() {
        let parser = PluggableParser {
            extensions: vec!["rs"],
            language: tree_sitter::Language::new(tree_sitter_rust::LANGUAGE),
            default_node_kind: "module",
            default_edge_kind: "use",
            queries: r#"
                (struct_item name: (type_identifier) @node.name)
                (impl_item trait: (type_identifier) @implements.target)
            "#,
        };
        let (_, edges) = parser.parse_file(
            Path::new("/repo"),
            "src/point.rs",
            "struct Point { x: f64, y: f64 } impl Display for Point {}",
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "implements" && e.target == "Display"),
            "expected implements edge to Display, got: {edges:?}"
        );
    }

    /// Kotlin `class Dog : Animal()` produces an "extends" edge.
    /// Kotlin grammar uses `identifier` (not `type_identifier`) inside user_type.
    #[test]
    fn kotlin_class_delegation_emits_extends_edge() {
        let parser = PluggableParser {
            extensions: vec!["kt"],
            language: tree_sitter::Language::new(tree_sitter_kotlin_ng::LANGUAGE),
            default_node_kind: "class",
            default_edge_kind: "import",
            queries: r#"
                (class_declaration (delegation_specifiers (delegation_specifier (constructor_invocation (user_type (identifier) @extends.target)))))
                (class_declaration (delegation_specifiers (delegation_specifier (user_type (identifier) @extends.target))))
            "#,
        };
        let (_, edges) =
            parser.parse_file(Path::new("/repo"), "src/Dog.kt", "class Dog : Animal()");
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "extends" && e.target == "Animal"),
            "expected extends edge to Animal, got: {edges:?}"
        );
    }

    /// C# `using System.Collections.Generic;` produces a "using" edge.
    #[test]
    fn cs_qualified_using_emits_edge() {
        let parser = PluggableParser {
            extensions: vec!["cs"],
            language: tree_sitter::Language::new(tree_sitter_c_sharp::LANGUAGE),
            default_node_kind: "class",
            default_edge_kind: "using",
            queries: r#"
                (using_directive [(identifier) (qualified_name)] @edge.target)
            "#,
        };
        let (_, edges) = parser.parse_file(
            Path::new("/repo"),
            "src/App.cs",
            "using System.Collections.Generic;",
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "using" && e.target.contains("Generic")),
            "expected using edge containing 'Generic', got: {edges:?}"
        );
    }

    /// C# base_list entries produce "extends" edges.
    #[test]
    fn cs_base_list_emits_extends_edges() {
        let parser = PluggableParser {
            extensions: vec!["cs"],
            language: tree_sitter::Language::new(tree_sitter_c_sharp::LANGUAGE),
            default_node_kind: "class",
            default_edge_kind: "using",
            // AST: (class_declaration (base_list (identifier)))
            queries: r#"
                (class_declaration name: (identifier) @node.name)
                (class_declaration (base_list (identifier) @extends.target))
            "#,
        };
        let (_, edges) = parser.parse_file(
            Path::new("/repo"),
            "src/Dog.cs",
            "class Dog : Animal, IRunnable {}",
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "extends" && e.target == "Animal"),
            "expected extends edge to Animal, got: {edges:?}"
        );
        assert!(
            edges
                .iter()
                .any(|e| e.kind == "extends" && e.target == "IRunnable"),
            "expected extends edge to IRunnable, got: {edges:?}"
        );
    }
}
