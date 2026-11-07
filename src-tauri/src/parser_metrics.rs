//! Versioned, syntax-based callable anatomy extraction.
//!
//! `decision_count` is the profile-defined branch contribution below. It is
//! deliberately not named cyclomatic or cognitive complexity.

use super::{physical_line_count, AnatomyStatus, CallableAnatomy, CallableVisibility, FileAnatomy};
use sha2::{Digest, Sha256};
use tree_sitter::{Language, Node};

const PROFILE_VERSION: &str = "callable-anatomy-v1";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Rust,
    Ruby,
    JavaScript,
    TypeScript,
    Elixir,
    Elm,
    Unsupported,
}

struct FoundCallable<'tree> {
    name: String,
    display_name: Option<String>,
    kind: String,
    visibility: CallableVisibility,
    arity: Option<usize>,
    owner: String,
    declaration: Node<'tree>,
}

pub(super) fn analyze(
    relative_path: &str,
    extension: &str,
    _configured_language: Language,
    source: &str,
) -> FileAnatomy {
    let family = family(extension);
    let profile_id = format!("{}:{PROFILE_VERSION}", family_name(family));
    let source_id = source_id(source.as_bytes());
    if family == Family::Unsupported || (family == Family::Elm && extension != "elm") {
        let mut unavailable = FileAnatomy::unavailable(
            relative_path,
            extension,
            "No measured anatomy profile is available for this language",
        );
        unavailable.source_id = source_id;
        unavailable.profile_id = profile_id;
        unavailable.line_count = Some(physical_line_count(source.as_bytes()));
        return unavailable;
    }

    let language = match family {
        Family::Rust => tree_sitter::Language::new(tree_sitter_rust::LANGUAGE),
        Family::Ruby => tree_sitter::Language::new(tree_sitter_ruby::LANGUAGE),
        Family::JavaScript => tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
        Family::TypeScript => {
            tree_sitter::Language::new(tree_sitter_typescript::LANGUAGE_TYPESCRIPT)
        }
        Family::Elixir => tree_sitter::Language::new(tree_sitter_elixir::LANGUAGE),
        Family::Elm => tree_sitter::Language::new(tree_sitter_elm::LANGUAGE),
        Family::Unsupported => unreachable!(),
    };
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&language).is_err() {
        let mut unavailable = FileAnatomy::unavailable(
            relative_path,
            extension,
            "Tree-sitter grammar could not be initialized",
        );
        unavailable.source_id = source_id;
        unavailable.profile_id = profile_id;
        unavailable.line_count = Some(physical_line_count(source.as_bytes()));
        return unavailable;
    }
    let Some(tree) = parser.parse(source, None) else {
        let mut unavailable = FileAnatomy::unavailable(
            relative_path,
            extension,
            "Tree-sitter produced no syntax tree",
        );
        unavailable.source_id = source_id;
        unavailable.profile_id = profile_id;
        unavailable.line_count = Some(physical_line_count(source.as_bytes()));
        return unavailable;
    };

    let mut found = Vec::new();
    let mut owner_stack = Vec::new();
    collect_callables(
        tree.root_node(),
        family,
        source.as_bytes(),
        &mut owner_stack,
        &mut found,
    );
    found.sort_by_key(|callable| callable.declaration.start_byte());

    let mut callables = Vec::with_capacity(found.len());
    for (ordinal, callable) in found.iter().enumerate() {
        let declaration = callable.declaration;
        let body = callable_body(declaration, family, source.as_bytes());
        let range = declaration.byte_range();
        let start_line = Some(declaration.start_position().row as u32 + 1);
        let end_line = Some(declaration.end_position().row as u32 + 1);
        let body_line_span = body.map(|body| {
            (body.end_position().row as u32 + 1)
                .saturating_sub(body.start_position().row as u32 + 1)
                + 1
        });
        let decision_count = if declaration.has_error() || body.is_some_and(|body| body.has_error())
        {
            None
        } else {
            body.and_then(|body| count_decisions(body, family, source.as_bytes()))
        };
        let match_key = format!("{}::{}::{}", callable.owner, callable.kind, callable.name);
        callables.push(CallableAnatomy {
            id: format!(
                "{}@{}:{:x}-{:x}:{}",
                relative_path, source_id, range.start, range.end, ordinal
            ),
            match_key,
            name: callable.name.clone(),
            display_name: callable.display_name.clone(),
            kind: callable.kind.clone(),
            visibility: callable.visibility.clone(),
            arity: callable.arity,
            start_line,
            end_line,
            body_line_span,
            decision_count,
        });
    }

    FileAnatomy {
        status: AnatomyStatus::Partial,
        file: relative_path.to_string(),
        language: extension.to_string(),
        source_id,
        profile_id,
        line_count: Some(physical_line_count(source.as_bytes())),
        first_seen_at: None,
        age_as_of: None,
        age_days: None,
        age_reason: None,
        evidence: "tree_sitter".to_string(),
        callables,
        state_slots: None,
        reason: Some(profile_reason(family).to_string()),
    }
}

// Tree-sitter Nodes carry the tree lifetime; keep handles tied to their tree.
fn collect_callables<'tree>(
    node: Node<'tree>,
    family: Family,
    source: &[u8],
    owners: &mut Vec<String>,
    found: &mut Vec<FoundCallable<'tree>>,
) {
    let owner_name = owner_name(node, family, source);
    if let Some(name) = &owner_name {
        owners.push(name.clone());
    }
    let callable = callable_info(node, family, source);
    if let Some((name, kind, visibility, arity)) = &callable {
        let owner = owners.join("::");
        let display_name = name
            .starts_with("<anonymous@")
            .then(|| anonymous_display_name(node, source, owners));
        found.push(FoundCallable {
            name: name.clone(),
            display_name: display_name.clone(),
            kind: kind.clone(),
            visibility: visibility.clone(),
            arity: *arity,
            owner,
            declaration: node,
        });
    }
    if let Some((name, _, _, _)) = &callable {
        owners.push(name.clone());
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_callables(child, family, source, owners, found);
    }
    if callable.is_some() {
        owners.pop();
    }
    if owner_name.is_some() {
        owners.pop();
    }
}

fn owner_name(node: Node<'_>, family: Family, source: &[u8]) -> Option<String> {
    let recognized = match family {
        Family::Rust => matches!(node.kind(), "impl_item" | "mod_item" | "trait_item"),
        Family::Ruby => matches!(node.kind(), "class" | "module" | "singleton_class"),
        Family::JavaScript | Family::TypeScript => matches!(
            node.kind(),
            "class_declaration" | "class" | "class_expression" | "interface_declaration"
        ),
        Family::Elixir => is_elixir_definition(node, source, "defmodule"),
        Family::Elm => false,
        Family::Unsupported => false,
    };
    if !recognized {
        return None;
    }
    let name_node = match family {
        Family::Rust if node.kind() == "impl_item" => node.child_by_field_name("type"),
        Family::Elixir => child_of_kind(node, "arguments").and_then(|args| first_named(args)),
        _ => node.child_by_field_name("name"),
    }?;
    text(name_node, source)
}

fn callable_info(
    node: Node<'_>,
    family: Family,
    source: &[u8],
) -> Option<(String, String, CallableVisibility, Option<usize>)> {
    let kind = node.kind();
    match family {
        Family::Rust if matches!(kind, "function_item" | "function_signature_item") => {
            let name = text(node.child_by_field_name("name")?, source)?;
            let params = node.child_by_field_name("parameters");
            let visibility = if child_of_kind(node, "visibility_modifier")
                .and_then(|modifier| text(modifier, source))
                .as_deref()
                == Some("pub")
            {
                CallableVisibility::Public
            } else {
                CallableVisibility::Private
            };
            Some((
                name,
                "function".into(),
                visibility,
                Some(params.map(parameter_count).unwrap_or(0)),
            ))
        }
        Family::Ruby if matches!(kind, "method" | "singleton_method") => {
            let name = text(node.child_by_field_name("name")?, source)?;
            let params = node.child_by_field_name("parameters");
            let visibility = if kind == "singleton_method" {
                CallableVisibility::Public
            } else {
                CallableVisibility::Unknown
            };
            Some((
                name,
                "method".into(),
                visibility,
                Some(params.map(parameter_count).unwrap_or(0)),
            ))
        }
        Family::JavaScript | Family::TypeScript
            if matches!(
                kind,
                "function_declaration"
                    | "generator_function_declaration"
                    | "method_definition"
                    | "arrow_function"
                    | "function_expression"
                    | "generator_function"
            ) =>
        {
            let name_node = node.child_by_field_name("name").or_else(|| {
                node.parent().and_then(|parent| match parent.kind() {
                    "variable_declarator" => parent.child_by_field_name("name"),
                    "assignment_expression" => parent.child_by_field_name("left"),
                    _ => None,
                })
            });
            let name = name_node
                .and_then(|name| text(name, source))
                .unwrap_or_else(|| format!("<anonymous@{}>", node.start_position().row + 1));
            let params = node.child_by_field_name("parameters").or_else(|| {
                if kind == "arrow_function" {
                    node.child_by_field_name("parameter")
                } else {
                    None
                }
            });
            let visibility = javascript_visibility(node, kind, source);
            let kind = if kind == "method_definition" {
                "method"
            } else {
                "function"
            };
            Some((
                name,
                kind.into(),
                visibility,
                Some(params.map(parameter_count).unwrap_or(0)),
            ))
        }
        Family::Elixir if is_elixir_callable(node, source) => {
            let target = node.child_by_field_name("target")?;
            let keyword = text(target, source)?;
            let args = child_of_kind(node, "arguments")?;
            let signature = first_named(args)?;
            let name = if signature.kind() == "call" {
                text(signature.child_by_field_name("target")?, source)?
            } else {
                text(signature, source)?
            };
            let params = child_of_kind(signature, "arguments");
            let visibility = if keyword == "def" {
                CallableVisibility::Public
            } else {
                CallableVisibility::Private
            };
            Some((
                name,
                "function".into(),
                visibility,
                Some(params.map(parameter_count).unwrap_or(0)),
            ))
        }
        Family::Elm if kind == "value_declaration" => {
            let left = node.child_by_field_name("functionDeclarationLeft")?;
            let name_node = left.named_child(0)?;
            let name = text(name_node, source)?;
            Some((
                name,
                "function".into(),
                CallableVisibility::Unknown,
                Some(parameter_count(left).saturating_sub(1)),
            ))
        }
        _ => None,
    }
}

fn anonymous_display_name(node: Node<'_>, source: &[u8], owners: &[String]) -> String {
    let owner = owners.join("::");
    let mut child = node;
    let mut parent = node.parent();
    while let Some(current) = parent {
        if current.kind() == "arguments" {
            let mut cursor = current.walk();
            let index = current
                .named_children(&mut cursor)
                .position(|argument| {
                    argument.start_byte() <= node.start_byte()
                        && argument.end_byte() >= node.end_byte()
                })
                .map(|index| index + 1)
                .unwrap_or(1);
            if let Some(call) = current.parent() {
                if call.kind() == "call_expression" {
                    if let Some(target) = call_site_target(call, source) {
                        let prefix = if owner.is_empty() {
                            String::new()
                        } else {
                            format!("{owner}::")
                        };
                        return format!("{prefix}{target} callback #{index}");
                    }
                }
            }
        }
        child = current;
        parent = current.parent();
    }
    let prefix = if owner.is_empty() {
        String::new()
    } else {
        format!("{owner}::")
    };
    let context = child
        .parent()
        .map(|parent| parent.kind())
        .unwrap_or("scope");
    format!("{prefix}anonymous {context}")
}

fn call_site_target(call: Node<'_>, source: &[u8]) -> Option<String> {
    let function = call.child_by_field_name("function")?;
    if function.kind() != "member_expression" {
        return text(function, source);
    }
    let object = function.child_by_field_name("object")?;
    let property = function.child_by_field_name("property")?;
    let receiver = if object.kind() == "call_expression" {
        call_receiver_base(object, source)?
    } else {
        text(object, source)?
    };
    Some(format!("{receiver}.{}", text(property, source)?))
}

fn call_receiver_base(call: Node<'_>, source: &[u8]) -> Option<String> {
    let function = call.child_by_field_name("function")?;
    if function.kind() != "member_expression" {
        return text(function, source);
    }
    let object = function.child_by_field_name("object")?;
    if object.kind() == "call_expression" {
        call_receiver_base(object, source)
    } else {
        text(object, source)
    }
}

fn callable_body<'tree>(node: Node<'tree>, family: Family, source: &[u8]) -> Option<Node<'tree>> {
    if let Some(body) = node.child_by_field_name("body") {
        return Some(body);
    }
    match family {
        Family::Ruby => node.named_children(&mut node.walk()).last(),
        Family::Elixir => node
            .child_by_field_name("do")
            .or_else(|| find_descendant(node, "do_block"))
            .or_else(|| elixir_short_body(node, source)),
        Family::Elm => node.child_by_field_name("body"),
        _ => None,
    }
}

fn elixir_short_body<'tree>(node: Node<'tree>, source: &[u8]) -> Option<Node<'tree>> {
    let arguments = child_of_kind(node, "arguments")?;
    let mut cursor = arguments.walk();
    for child in arguments.named_children(&mut cursor) {
        if child.kind() != "keywords" {
            continue;
        }
        let mut pairs = child.walk();
        for pair in child.named_children(&mut pairs) {
            if pair.kind() == "pair"
                && pair
                    .child_by_field_name("key")
                    .and_then(|key| text(key, source))
                    .as_deref()
                    == Some("do:")
            {
                return pair.child_by_field_name("value");
            }
        }
    }
    None
}

fn count_decisions(body: Node<'_>, family: Family, source: &[u8]) -> Option<u32> {
    // Contributions: Rust if/loop forms, match arms, &&/||; JavaScript and
    // TypeScript if/loop/ternary/switch cases, &&/||/??; Ruby condition and
    // loop forms, `elsif`, case `when`/pattern `in`, &&/||/and/or; Elixir if/unless/case/cond/with;
    // Elm if expressions and case branches. Each case alternative adds one.
    let mut count = 0;
    count_decisions_inner(body, body, family, source, &mut count);
    Some(count)
}

fn count_decisions_inner(
    node: Node<'_>,
    root: Node<'_>,
    family: Family,
    source: &[u8],
    count: &mut u32,
) {
    if node.id() != root.id()
        && (callable_info(node, family, source).is_some()
            || is_unmodeled_callable_body(node, family))
    {
        return;
    }
    let kind = node.kind();
    match family {
        Family::Rust => match kind {
            "if_expression" | "while_expression" | "for_expression" | "loop_expression" => {
                *count += 1
            }
            "match_arm"
                if node.parent().map(|parent| parent.kind()) == Some("match_expression") =>
            {
                *count += 1
            }
            "binary_expression"
                if operator_text(node, source).is_some_and(|op| matches!(op, "&&" | "||")) =>
            {
                *count += 1
            }
            _ => {}
        },
        Family::JavaScript | Family::TypeScript => match kind {
            "if_statement" | "for_statement" | "for_in_statement" | "while_statement"
            | "do_statement" | "ternary_expression" => *count += 1,
            "switch_case" if node.child_by_field_name("value").is_some() => *count += 1,
            "binary_expression"
                if operator_text(node, source)
                    .is_some_and(|op| matches!(op, "&&" | "||" | "??")) =>
            {
                *count += 1
            }
            _ => {}
        },
        Family::Ruby => match kind {
            "if" | "unless" | "while" | "until" | "for" | "if_modifier" | "unless_modifier"
            | "while_modifier" | "until_modifier" | "elsif" => *count += 1,
            "when" if node.parent().map(|parent| parent.kind()) == Some("case") => *count += 1,
            "in_clause" if node.parent().map(|parent| parent.kind()) == Some("case_match") => {
                *count += 1
            }
            "binary"
                if operator_text(node, source)
                    .is_some_and(|op| matches!(op, "&&" | "||" | "and" | "or")) =>
            {
                *count += 1
            }
            _ => {}
        },
        Family::Elixir => {
            if is_elixir_definition(node, source, "if")
                || is_elixir_definition(node, source, "unless")
                || is_elixir_definition(node, source, "case")
                || is_elixir_definition(node, source, "cond")
                || is_elixir_definition(node, source, "with")
            {
                *count += 1;
            }
        }
        Family::Elm => match kind {
            "if_else_expr" => *count += 1,
            "case_of_branch" => *count += 1,
            _ => {}
        },
        Family::Unsupported => return,
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        count_decisions_inner(child, root, family, source, count);
    }
}

fn is_elixir_definition(node: Node<'_>, source: &[u8], keyword: &str) -> bool {
    node.kind() == "call"
        && node
            .child_by_field_name("target")
            .and_then(|target| text(target, source))
            .as_deref()
            == Some(keyword)
}

fn is_unmodeled_callable_body(node: Node<'_>, family: Family) -> bool {
    match family {
        Family::Rust => node.kind() == "closure_expression",
        Family::Ruby => matches!(node.kind(), "block" | "do_block" | "lambda"),
        Family::Elixir => node.kind() == "anonymous_function",
        Family::Elm => node.kind() == "anonymous_function_expr",
        Family::JavaScript | Family::TypeScript | Family::Unsupported => false,
    }
}

fn is_elixir_callable(node: Node<'_>, source: &[u8]) -> bool {
    is_elixir_definition(node, source, "def") || is_elixir_definition(node, source, "defp")
}

fn parameter_count(node: Node<'_>) -> usize {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| !matches!(child.kind(), "comment" | "type_annotation"))
        .count()
}

fn first_named(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).next();
    found
}

fn child_of_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    let found = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == kind);
    found
}

fn find_descendant<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = find_descendant(child, kind) {
            return Some(found);
        }
    }
    None
}

fn text(node: Node<'_>, source: &[u8]) -> Option<String> {
    node.utf8_text(source).ok().map(str::to_string)
}

fn operator_text<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let op = node.child_by_field_name("operator")?;
    op.utf8_text(source).ok()
}

fn exported(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        parent.kind() == "export_statement"
            || parent.kind() == "export_declaration"
            || parent.kind() == "export_default_declaration"
    })
}

fn javascript_visibility(node: Node<'_>, kind: &str, source: &[u8]) -> CallableVisibility {
    if node
        .child_by_field_name("name")
        .and_then(|name| text(name, source))
        .is_some_and(|name| name.starts_with('#'))
    {
        return CallableVisibility::Private;
    }
    if kind == "method_definition" {
        if let Some(modifier) = child_of_kind(node, "accessibility_modifier")
            .and_then(|modifier| text(modifier, source))
        {
            return if modifier == "public" {
                CallableVisibility::Public
            } else {
                CallableVisibility::Private
            };
        }
        return CallableVisibility::Public;
    }
    if exported(node) {
        CallableVisibility::Public
    } else {
        CallableVisibility::Private
    }
}

fn family(extension: &str) -> Family {
    match extension {
        "rs" => Family::Rust,
        "rb" | "rake" => Family::Ruby,
        "js" | "jsx" | "mjs" | "cjs" => Family::JavaScript,
        "ts" | "tsx" => Family::TypeScript,
        "ex" | "exs" => Family::Elixir,
        "elm" => Family::Elm,
        _ => Family::Unsupported,
    }
}

fn family_name(family: Family) -> &'static str {
    match family {
        Family::Rust => "rust",
        Family::Ruby => "ruby",
        Family::JavaScript => "javascript",
        Family::TypeScript => "typescript",
        Family::Elixir => "elixir",
        Family::Elm => "elm",
        Family::Unsupported => "unknown",
    }
}

fn profile_reason(family: Family) -> &'static str {
    match family {
        Family::Rust => "Recognizes function items and trait signatures; closure bodies and macro-generated declarations are not callable records. State slots remain unavailable.",
        Family::Ruby => "Recognizes methods and singleton methods; block and lambda bodies are not callable records. Lexical private visibility and state slots remain unavailable.",
        Family::JavaScript => "Recognizes function declarations, expressions, arrows, and methods; accessor declarations are not callable records. State slots remain unavailable.",
        Family::TypeScript => "Recognizes function declarations, expressions, arrows, and methods; accessor declarations and overload signatures are not complete callable records. State slots remain unavailable.",
        Family::Elixir => "Recognizes def and defp declarations; macros and anonymous function bodies are not callable records. State slots remain unavailable.",
        Family::Elm => "Recognizes value declarations; anonymous functions and module-exposure visibility are not represented. State slots remain unavailable.",
        Family::Unsupported => "No measured anatomy profile is available for this language.",
    }
}

pub(crate) fn source_id(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_decisions_are_distinct_from_unavailable_measurements() {
        let source = "fn calm() {}";
        let anatomy = analyze(
            "calm.rs",
            "rs",
            tree_sitter::Language::new(tree_sitter_rust::LANGUAGE),
            source,
        );
        assert_eq!(anatomy.callables.len(), 1);
        assert_eq!(anatomy.callables[0].decision_count, Some(0));
        assert!(anatomy.callables[0].body_line_span.is_some());
        assert_eq!(anatomy.line_count, Some(1));
    }

    #[test]
    fn physical_file_line_count_handles_empty_and_final_newline() {
        let empty = analyze(
            "empty.rs",
            "rs",
            tree_sitter::Language::new(tree_sitter_rust::LANGUAGE),
            "",
        );
        let terminated = analyze(
            "lines.rs",
            "rs",
            tree_sitter::Language::new(tree_sitter_rust::LANGUAGE),
            "fn one() {}\nfn two() {}\n",
        );
        assert_eq!(empty.line_count, Some(0));
        assert_eq!(terminated.line_count, Some(2));
    }

    /// Given anonymous callbacks at several call sites, when Rust extracts
    /// anatomy, then display labels identify their lexical owner, call site,
    /// and one-based argument position without changing matching identity.
    #[test]
    fn anonymous_callbacks_receive_contextual_display_names() {
        let source = "function buildPathRegions(points, seed) { return points.map((point, index) => point).reduce(seed, (value, item) => value).filter((point) => point.ok); }";
        let anatomy = analyze(
            "regions.js",
            "js",
            tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
            source,
        );
        let callbacks = anatomy
            .callables
            .iter()
            .filter(|callable| callable.name.starts_with("<anonymous@"))
            .collect::<Vec<_>>();
        let display_names = callbacks
            .iter()
            .map(|callable| callable.display_name.as_deref().unwrap())
            .collect::<Vec<_>>();
        assert!(display_names
            .iter()
            .any(|name| *name == "buildPathRegions::points.map callback #1"));
        assert!(display_names
            .iter()
            .any(|name| *name == "buildPathRegions::points.reduce callback #2"));
        assert!(display_names
            .iter()
            .any(|name| *name == "buildPathRegions::points.filter callback #1"));
        assert_eq!(callbacks.len(), 3);
        assert_eq!(
            callbacks
                .iter()
                .map(|callable| callable.id.as_str())
                .collect::<std::collections::HashSet<_>>()
                .len(),
            3
        );
        assert!(callbacks
            .iter()
            .all(|callable| callable.match_key.contains(&callable.name)));
    }

    #[test]
    fn nested_callable_decisions_belong_only_to_nested_callable() {
        let source = "function outer() { if (true) {} function inner() { if (true) {} } }";
        let anatomy = analyze(
            "nested.js",
            "js",
            tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
            source,
        );
        let outer = anatomy
            .callables
            .iter()
            .find(|callable| callable.name == "outer")
            .unwrap();
        let inner = anatomy
            .callables
            .iter()
            .find(|callable| callable.name == "inner")
            .unwrap();
        assert_eq!(outer.decision_count, Some(1));
        assert_eq!(inner.decision_count, Some(1));
    }

    #[test]
    fn nested_callable_decisions_are_excluded_in_every_profile() {
        let cases = [
            (
                "rs",
                tree_sitter::Language::new(tree_sitter_rust::LANGUAGE),
                "fn outer() { if true {} fn inner() { if true {} } }",
                "outer",
            ),
            (
                "rb",
                tree_sitter::Language::new(tree_sitter_ruby::LANGUAGE),
                "def outer\n if true\n end\n def inner\n  if true\n  end\n end\nend",
                "outer",
            ),
            (
                "js",
                tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
                "function outer() { if (true) {} function inner() { if (true) {} } }",
                "outer",
            ),
            (
                "ts",
                tree_sitter::Language::new(tree_sitter_typescript::LANGUAGE_TYPESCRIPT),
                "function outer() { if (true) {} function inner() { if (true) {} } }",
                "outer",
            ),
            (
                "ex",
                tree_sitter::Language::new(tree_sitter_elixir::LANGUAGE),
                "defmodule M do\n def outer do\n  if true do\n   def inner do\n    if true do\n     :ok\n    end\n   end\n  end\n end\nend",
                "outer",
            ),
            (
                "elm",
                tree_sitter::Language::new(tree_sitter_elm::LANGUAGE),
                "outer x =\n    if x then (\\y -> if y then 2 else 3) else 4\n",
                "outer",
            ),
        ];

        for (extension, language, source, outer_name) in cases {
            let anatomy = analyze(&format!("nested.{extension}"), extension, language, source);
            let outer = anatomy
                .callables
                .iter()
                .find(|callable| callable.name == outer_name)
                .unwrap_or_else(|| panic!("missing {outer_name} for .{extension}: {anatomy:?}"));
            assert_eq!(outer.decision_count, Some(1), ".{extension}: {anatomy:?}");
        }
    }

    #[test]
    fn declaration_without_a_body_keeps_body_metrics_unknown() {
        let anatomy = analyze(
            "api.rs",
            "rs",
            tree_sitter::Language::new(tree_sitter_rust::LANGUAGE),
            "trait Api { fn fetch(&self); }",
        );
        let function = anatomy
            .callables
            .iter()
            .find(|callable| callable.name == "fetch")
            .unwrap();
        assert_eq!(function.body_line_span, None);
        assert_eq!(function.decision_count, None);
    }

    #[test]
    fn javascript_logical_operators_are_explicit_decisions() {
        let source = "function check(a, b, c) { return a && (b || c); }";
        let anatomy = analyze(
            "logic.js",
            "js",
            tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
            source,
        );
        assert_eq!(anatomy.callables[0].decision_count, Some(2));
    }

    #[test]
    fn captured_javascript_visibility_is_preserved() {
        let js = analyze(
            "class.js",
            "js",
            tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
            "class A { #hidden() {} open() {} }",
        );
        assert_eq!(
            js.callables
                .iter()
                .find(|callable| callable.name == "#hidden")
                .unwrap()
                .visibility,
            CallableVisibility::Private
        );
        assert_eq!(
            js.callables
                .iter()
                .find(|callable| callable.name == "open")
                .unwrap()
                .visibility,
            CallableVisibility::Public
        );

        let ts = analyze(
            "class.ts",
            "ts",
            tree_sitter::Language::new(tree_sitter_typescript::LANGUAGE_TYPESCRIPT),
            "class A { private hidden() {} public open() {} }",
        );
        assert_eq!(
            ts.callables
                .iter()
                .find(|callable| callable.name == "hidden")
                .unwrap()
                .visibility,
            CallableVisibility::Private
        );
        assert_eq!(
            ts.callables
                .iter()
                .find(|callable| callable.name == "open")
                .unwrap()
                .visibility,
            CallableVisibility::Public
        );
    }

    #[test]
    fn same_named_methods_have_distinct_owner_match_keys() {
        let source = "class A { run() {} } class B { run() {} }";
        let anatomy = analyze(
            "classes.js",
            "js",
            tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
            source,
        );
        let keys: Vec<_> = anatomy
            .callables
            .iter()
            .map(|callable| callable.match_key.as_str())
            .collect();
        assert_eq!(keys.len(), 2);
        assert_ne!(keys[0], keys[1]);
        assert_ne!(anatomy.callables[0].id, anatomy.callables[1].id);
    }

    #[test]
    fn malformed_source_remains_partial_without_inventing_callables() {
        let anatomy = analyze(
            "broken.rs",
            "rs",
            tree_sitter::Language::new(tree_sitter_rust::LANGUAGE),
            "fn broken( {",
        );
        assert_eq!(anatomy.status, AnatomyStatus::Partial);
        assert!(anatomy.callables.is_empty());
        assert!(anatomy.reason.is_some());
    }

    #[test]
    fn syntax_error_in_another_declaration_does_not_change_measured_callable() {
        let anatomy = analyze(
            "partly-broken.rs",
            "rs",
            tree_sitter::Language::new(tree_sitter_rust::LANGUAGE),
            "fn sound() { if true {} }\nfn broken( {",
        );
        let sound = anatomy
            .callables
            .iter()
            .find(|callable| callable.name == "sound")
            .unwrap();
        assert_eq!(sound.decision_count, Some(1));
    }

    #[test]
    fn elm_uses_the_installed_tree_sitter_grammar() {
        let source = "f x =\n    if x then 1 else 0\n";
        let anatomy = analyze(
            "Main.elm",
            "elm",
            tree_sitter::Language::new(tree_sitter_elm::LANGUAGE),
            source,
        );
        assert!(anatomy
            .callables
            .iter()
            .any(|callable| callable.name == "f"));
        assert_eq!(anatomy.callables[0].decision_count, Some(1));
    }
}
