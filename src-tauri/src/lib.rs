mod graph;
mod jj;
mod parser;
mod vcs;

use graph::Graph;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use parser::{Parser, PluggableParser};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

struct AppState {
    repo: Mutex<PathBuf>,
    parsers: Vec<Box<dyn Parser>>,
    last_fingerprint: Mutex<u64>,
    watcher: Mutex<Option<RecommendedWatcher>>,
    graph: Mutex<Graph>,
    last_since: Mutex<Option<String>>,
    last_include_neighbors: Mutex<bool>,
    last_branch: Mutex<String>,
    last_revision: Mutex<String>,
}

fn debug_enabled() -> bool {
    static DEBUG: OnceLock<bool> = OnceLock::new();
    *DEBUG.get_or_init(|| {
        match std::env::var("SLOPOSCOPE_DEBUG").or_else(|_| std::env::var("EYELOSS_DEBUG")) {
            Ok(v) => {
                let v = v.to_ascii_lowercase();
                matches!(v.as_str(), "1" | "true" | "yes" | "on")
            }
            Err(_) => cfg!(debug_assertions),
        }
    })
}

fn resolve_graph_edges(g: &mut Graph) {
    let all_node_ids: Vec<String> = g.nodes.iter().map(|n| n.id.clone()).collect();

    for edge in &mut g.edges {
        let target = &edge.target;
        if all_node_ids.contains(target) {
            continue;
        }

        let normalized = target
            .replace("::", "/")
            .replace("crate/", "")
            .trim_start_matches("./")
            .to_string();
        if let Some(found) = all_node_ids.iter().find(|id| {
            id.as_str() == normalized.as_str()
                || id.ends_with(&normalized)
                || id.replace("/", ".").contains(target)
        }) {
            edge.target = found.clone();
            continue;
        }

        if let Some(found) = all_node_ids.iter().find(|id| {
            let id_no_ext = id.split('.').next().unwrap_or(id);
            id_no_ext.contains(target) || target.contains(id_no_ext)
        }) {
            edge.target = found.clone();
        }
    }
}

fn perform_graph_build(
    parsers: &[Box<dyn Parser>],
    repo: PathBuf,
    since: Option<String>,
    include_neighbors: bool,
) -> Graph {
    let started = Instant::now();
    if debug_enabled() {
        println!(
            "[BACKEND][build] start repo={} since={:?} include_neighbors={}",
            repo.display(),
            since,
            include_neighbors
        );
    }

    let mut g = Graph::new();
    let files_to_process = if let Some(ref revset) = since {
        let changed = vcs::get_changed_files(&repo, revset);
        changed.keys().cloned().collect::<Vec<String>>()
    } else {
        vcs::get_changed_files(&repo, "@")
            .keys()
            .cloned()
            .collect::<Vec<String>>()
    };

    if debug_enabled() {
        println!(
            "[BACKEND][build] changed-file candidates={} (repo={})",
            files_to_process.len(),
            repo.display()
        );
    }

    let mut parsed_files = 0usize;
    let mut unsupported_files = 0usize;
    let mut unreadable_files = 0usize;
    for rel_path in files_to_process {
        let full_path = repo.join(&rel_path);
        let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");

        if let Some(parser) = parsers.iter().find(|p| p.extensions().contains(&ext)) {
            if let Ok(source) = std::fs::read_to_string(&full_path) {
                let (nodes, edges) = parser.parse_file(&repo, &rel_path, &source);
                g.add_nodes(nodes);
                g.add_edges(edges);
                parsed_files += 1;
            } else {
                unreadable_files += 1;
            }
        } else {
            unsupported_files += 1;
        }
    }

    resolve_graph_edges(&mut g);
    g.finalize();

    if let Some(revset) = since {
        let changed = vcs::get_changed_files(&repo, &revset);
        g.filter_to_changes(&changed, include_neighbors);
    }

    if debug_enabled() {
        println!(
            "[BACKEND][build] done nodes={} edges={} parsed={} unsupported={} unreadable={} took={}ms",
            g.nodes.len(),
            g.edges.len(),
            parsed_files,
            unsupported_files,
            unreadable_files,
            started.elapsed().as_millis()
        );
    }

    g
}

fn fingerprint(repo: &PathBuf) -> u64 {
    graph::source_fingerprint(repo)
}

fn resolve_initial_repo() -> PathBuf {
    let from_arg = std::env::args().nth(1).map(PathBuf::from);
    if let Some(path) = from_arg {
        return path;
    }

    let cwd = std::env::current_dir().expect("no cwd");
    let mut candidate = cwd.clone();
    loop {
        if !matches!(vcs::detect_engine(&candidate), vcs::VCSEngine::None) {
            return candidate;
        }
        if !candidate.pop() {
            break;
        }
    }
    cwd
}

// --- Tauri commands ---

#[tauri::command]
async fn select_repo(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<String, String> {
    let started = Instant::now();
    if debug_enabled() {
        println!("[BACKEND][cmd] select_repo start");
    }

    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder);
    });

    let folder = rx.recv().map_err(|_| "Dialog cancelled".to_string())?;

    if let Some(path) = folder {
        let path_buf = match path {
            tauri_plugin_dialog::FilePath::Path(p) => p,
            tauri_plugin_dialog::FilePath::Url(u) => u
                .to_file_path()
                .map_err(|_| "Invalid URL path".to_string())?,
        };

        if matches!(vcs::detect_engine(&path_buf), vcs::VCSEngine::None) {
            return Err("This directory contains no record of its own failures. Sloposcope requires a Git or JJ repository to function. How disappointing.".to_string());
        }

        {
            let mut repo = state.repo.lock();
            *repo = path_buf.clone();
        }

        let fp = fingerprint(&path_buf);
        *state.last_fingerprint.lock() = fp;
        *state.last_since.lock() = None;
        *state.last_include_neighbors.lock() = false;
        *state.last_branch.lock() = vcs::get_current_branch(&path_buf);
        *state.last_revision.lock() = vcs::get_current_revision(&path_buf);

        let watcher = setup_watcher(&app);
        *state.watcher.lock() = watcher;

        if debug_enabled() {
            println!(
                "[BACKEND][cmd] select_repo done path={} took={}ms",
                path_buf.display(),
                started.elapsed().as_millis()
            );
        }
        Ok(path_buf.display().to_string())
    } else {
        if debug_enabled() {
            println!(
                "[BACKEND][cmd] select_repo cancelled after {}ms",
                started.elapsed().as_millis()
            );
        }
        Err("No folder selected".into())
    }
}

#[tauri::command]
async fn get_graph(
    state: tauri::State<'_, AppState>,
    since: Option<String>,
    include_neighbors: Option<bool>,
) -> Result<Graph, String> {
    let started = Instant::now();
    let include_neighbors = include_neighbors.unwrap_or(false);
    let repo = state.repo.lock().clone();
    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_graph start repo={} since={:?} include_neighbors={}",
            repo.display(),
            since,
            include_neighbors
        );
    }
    let current_branch = vcs::get_current_branch(&repo);
    let current_revision = vcs::get_current_revision(&repo);
    {
        let last_since = state.last_since.lock();
        let last_include_neighbors = state.last_include_neighbors.lock();
        let last_branch = state.last_branch.lock();
        let last_revision = state.last_revision.lock();
        if *last_since == since
            && *last_include_neighbors == include_neighbors
            && *last_branch == current_branch
            && *last_revision == current_revision
        {
            if debug_enabled() {
                println!(
                    "[BACKEND][cmd] get_graph cache-hit took={}ms",
                    started.elapsed().as_millis()
                );
            }
            return Ok(state.graph.lock().clone());
        }
    }

    let g = perform_graph_build(&state.parsers, repo, since.clone(), include_neighbors);

    {
        let mut graph = state.graph.lock();
        *graph = g.clone();
        let mut last_since = state.last_since.lock();
        *last_since = since;
        let mut last_include_neighbors = state.last_include_neighbors.lock();
        *last_include_neighbors = include_neighbors;
        let mut last_branch = state.last_branch.lock();
        *last_branch = current_branch;
        let mut last_revision = state.last_revision.lock();
        *last_revision = current_revision;
    }

    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_graph done nodes={} edges={} took={}ms",
            g.nodes.len(),
            g.edges.len(),
            started.elapsed().as_millis()
        );
    }

    Ok(g)
}

#[tauri::command]
fn get_changes(
    state: tauri::State<AppState>,
    limit: Option<usize>,
    before_id: Option<String>,
) -> Vec<vcs::Change> {
    let started = Instant::now();
    let repo = state.repo.lock().clone();
    let limit = limit.unwrap_or(20);
    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_changes start repo={} limit={} before_id={:?}",
            repo.display(),
            limit,
            before_id
        );
    }
    let changes = vcs::get_changes(&repo, limit, before_id);
    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_changes done count={} took={}ms",
            changes.len(),
            started.elapsed().as_millis()
        );
    }
    changes
}

#[tauri::command]
fn get_bookmarks(state: tauri::State<AppState>) -> Vec<vcs::Bookmark> {
    let started = Instant::now();
    let repo = state.repo.lock().clone();
    let bookmarks = vcs::get_bookmarks(&repo);
    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_bookmarks repo={} count={} took={}ms",
            repo.display(),
            bookmarks.len(),
            started.elapsed().as_millis()
        );
    }
    bookmarks
}

#[tauri::command]
fn get_current_branch(state: tauri::State<AppState>) -> String {
    let started = Instant::now();
    let repo = state.repo.lock().clone();
    let branch = vcs::get_current_branch(&repo);
    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_current_branch repo={} branch='{}' took={}ms",
            repo.display(),
            branch,
            started.elapsed().as_millis()
        );
    }
    branch
}

#[tauri::command]
fn get_file_diff(state: tauri::State<AppState>, file: String, since: Option<String>) -> String {
    let started = Instant::now();
    let repo = state.repo.lock().clone();
    let diff = vcs::get_file_diff(&repo, &file, since.as_deref());
    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_file_diff file={} bytes={} took={}ms",
            file,
            diff.len(),
            started.elapsed().as_millis()
        );
    }
    diff
}

#[tauri::command]
fn get_file_source(state: tauri::State<AppState>, file: String) -> String {
    let started = Instant::now();
    let repo = state.repo.lock().clone();
    let path = repo.join(&file);
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|_| "--- FILE DELETED OR INACCESSIBLE ---".to_string());
    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_file_source file={} bytes={} took={}ms",
            file,
            content.len(),
            started.elapsed().as_millis()
        );
    }
    content
}

#[tauri::command]
fn save_file(state: tauri::State<AppState>, file: String, content: String) -> Result<(), String> {
    let started = Instant::now();
    let repo = state.repo.lock().clone();
    let path = repo.join(&file);
    let bytes = content.len();
    let result = std::fs::write(path, content).map_err(|e| e.to_string());
    if debug_enabled() {
        match &result {
            Ok(_) => println!(
                "[BACKEND][cmd] save_file file={} bytes={} took={}ms",
                file,
                bytes,
                started.elapsed().as_millis()
            ),
            Err(err) => println!(
                "[BACKEND][cmd] save_file file={} failed='{}' took={}ms",
                file,
                err,
                started.elapsed().as_millis()
            ),
        }
    }
    result
}

#[tauri::command]
fn get_repo_path(state: tauri::State<AppState>) -> String {
    let path = state.repo.lock().display().to_string();
    if debug_enabled() {
        println!("[BACKEND][cmd] get_repo_path -> {}", path);
    }
    path
}

// --- File watcher and polling ---

fn refresh_vcs_state(handle: &AppHandle) {
    if let Some(state) = handle.try_state::<AppState>() {
        let repo = state.repo.lock().clone();
        let current_branch = vcs::get_current_branch(&repo);
        let current_revision = vcs::get_current_revision(&repo);
        let last_branch = state.last_branch.lock().clone();
        let last_revision = state.last_revision.lock().clone();

        if current_branch == last_branch && current_revision == last_revision {
            return;
        }

        if debug_enabled() {
            println!(
                "[BACKEND] VCS state changed: branch '{}' -> '{}', rev '{}' -> '{}'. Rebuilding graph.",
                last_branch,
                current_branch,
                if last_revision.len() > 12 { &last_revision[..12] } else { &last_revision },
                if current_revision.len() > 12 { &current_revision[..12] } else { &current_revision },
            );
        }

        let since = state.last_since.lock().clone();
        let include_neighbors = *state.last_include_neighbors.lock();
        let rebuilt = perform_graph_build(&state.parsers, repo.clone(), since, include_neighbors);
        {
            let mut graph = state.graph.lock();
            *graph = rebuilt.clone();
        }
        *state.last_branch.lock() = current_branch.clone();
        *state.last_revision.lock() = current_revision;

        let _ = handle.emit(
            "graph-updated",
            serde_json::json!({
                "graph": rebuilt,
                "changes": vcs::get_changes(&repo, 20, None),
                "bookmarks": vcs::get_bookmarks(&repo),
                "current_branch": current_branch,
            }),
        );
    }
}

fn setup_watcher(app: &AppHandle) -> Option<RecommendedWatcher> {
    if debug_enabled() {
        println!("[BACKEND][watcher] setup start");
    }
    let handle = app.clone();

    let mut watcher = match notify::recommended_watcher(move |res: Result<Event, _>| {
        if let Ok(event) = res {
            if let Some(state) = handle.try_state::<AppState>() {
                let repo = state.repo.lock().clone();

                let is_vcs_meta_path = |p: &PathBuf| {
                    let s = p.display().to_string();
                    s.contains("/.git/")
                        || s.ends_with("/.git")
                        || s.contains("/.jj/")
                        || s.ends_with("/.jj")
                };
                let is_ignored_runtime_path = |p: &PathBuf| {
                    let s = p.display().to_string();
                    s.contains("/_build/")
                        || s.contains("/deps/")
                        || s.contains("/dist/")
                        || s.contains("/.svelte-kit/")
                        || s.contains("/.output/")
                        || s.contains("/node_modules/")
                        || s.contains("/target/")
                        || s.contains("/src-tauri/gen/")
                };

                let vcs_meta_changed = event.paths.iter().any(|p| is_vcs_meta_path(p));
                let interesting_paths: Vec<PathBuf> = event
                    .paths
                    .iter()
                    .filter(|p| !is_vcs_meta_path(p) && !is_ignored_runtime_path(p))
                    .cloned()
                    .collect();

                // Branch/bookmark/HEAD updates may only touch .git/.jj metadata.
                // Rebuild from current filters so UI follows VCS changes immediately.
                if vcs_meta_changed {
                    refresh_vcs_state(&handle);
                    return;
                }

                if !interesting_paths.is_empty() {
                    if debug_enabled() {
                        println!(
                            "[BACKEND] Watch event {:?}, {} interesting path(s)",
                            event.kind,
                            interesting_paths.len()
                        );
                    }
                    // 1. Emit heat events immediately
                    for path in &interesting_paths {
                        if let Ok(rel) = path.strip_prefix(&repo) {
                            let _ = handle.emit("file-touched", rel.display().to_string());
                        }
                    }

                    // 2. Perform incremental graph update
                    let mut graph = state.graph.lock();
                    let mut changed = false;

                    for path in &interesting_paths {
                        if let Ok(rel_path_buf) = path.strip_prefix(&repo) {
                            let rel_path = rel_path_buf.display().to_string();

                            // Always remove old version of this file's nodes
                            graph.nodes.retain(|n| n.file != rel_path);
                            changed = true;

                            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                            if let Some(parser) =
                                state.parsers.iter().find(|p| p.extensions().contains(&ext))
                            {
                                if let Ok(source) = std::fs::read_to_string(path) {
                                    // Parse and add new version if file still exists
                                    let (new_nodes, new_edges) =
                                        parser.parse_file(&repo, &rel_path, &source);
                                    graph.add_nodes(new_nodes);
                                    graph.add_edges(new_edges);
                                }
                            }
                        }
                    }

                    if changed {
                        if debug_enabled() {
                            println!("[BACKEND] Incremental graph update emitted");
                        }
                        resolve_graph_edges(&mut graph);
                        graph.finalize();
                        let graph_snapshot = graph.clone();
                        drop(graph);
                        let current_branch = vcs::get_current_branch(&repo);
                        let current_revision = vcs::get_current_revision(&repo);
                        *state.last_branch.lock() = current_branch.clone();
                        *state.last_revision.lock() = current_revision;
                        let _ = handle.emit(
                            "graph-updated",
                            serde_json::json!({
                                "graph": graph_snapshot,
                                "changes": vcs::get_changes(&repo, 20, None),
                                "bookmarks": vcs::get_bookmarks(&repo),
                                "current_branch": current_branch,
                            }),
                        );
                    }
                }
            }
        }
    }) {
        Ok(w) => w,
        Err(_) => return None,
    };

    if let Some(state) = app.try_state::<AppState>() {
        let repo_path = state.repo.lock().clone();
        let _ = watcher.watch(&repo_path, RecursiveMode::Recursive);
        if debug_enabled() {
            println!(
                "[BACKEND][watcher] watching repo={} recursively",
                repo_path.display()
            );
        }
    }

    if debug_enabled() {
        println!("[BACKEND][watcher] setup complete");
    }
    Some(watcher)
}

// --- Entry point ---

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let boot_start = Instant::now();
    let repo = resolve_initial_repo();
    let repo = repo.canonicalize().unwrap_or(repo);
    if debug_enabled() {
        println!("[BACKEND][boot] run start repo={}", repo.display());
    }

    let parsers: Vec<Box<dyn Parser>> = vec![
        Box::new(PluggableParser {
            extensions: vec!["ex", "exs"],
            language: tree_sitter::Language::new(tree_sitter_elixir::LANGUAGE),
            default_node_kind: "module",
            default_edge_kind: "call",
            queries: r#"
                (call target: (identifier) @kw (#eq? @kw "defmodule") (arguments (alias) @node.name))
                (call target: (identifier) @func.kind (#match? @func.kind "^(def|defp)$") (arguments [(identifier) @func.name (call target: (identifier) @func.name)]))
                (dot (alias) @edge.target (identifier))
                (call target: (identifier) @edge.kind (#match? @edge.kind "^(use|import|alias)$") (arguments (alias) @edge.target))
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["rb", "rake"],
            language: tree_sitter::Language::new(tree_sitter_ruby::LANGUAGE),
            default_node_kind: "class",
            default_edge_kind: "call",
            queries: r#"
                (module name: [(constant) @node.name (scope_resolution) @node.name])
                (class name: [(constant) @node.name (scope_resolution) @node.name])
                (method name: (identifier) @func.name)
                (call receiver: [(constant) @edge.target (scope_resolution) @edge.target])
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["js", "ts", "jsx", "tsx"],
            language: tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
            default_node_kind: "lib",
            default_edge_kind: "import",
            queries: r#"
                (import_statement (import_clause (named_imports (import_specifier name: (identifier) @edge.target))) source: (string (string_fragment) @edge.target))
                (import_statement (import_clause (identifier) @edge.target) source: (string (string_fragment) @edge.target))
                (function_declaration name: (identifier) @func.name)
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["svelte"],
            language: tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
            default_node_kind: "component",
            default_edge_kind: "import",
            queries: r#"
                (import_statement source: (string (string_fragment) @edge.target))
                (import_statement (import_clause) source: (string (string_fragment) @edge.target))
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["py"],
            language: tree_sitter::Language::new(tree_sitter_python::LANGUAGE),
            default_node_kind: "module",
            default_edge_kind: "import",
            queries: r#"
                (class_definition name: (identifier) @node.name)
                (function_definition name: (identifier) @func.name)
                (import_from_statement module: (dotted_name) @edge.target)
                (import_statement name: (dotted_name) @edge.target)
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["rs"],
            language: tree_sitter::Language::new(tree_sitter_rust::LANGUAGE),
            default_node_kind: "module",
            default_edge_kind: "use",
            queries: r#"
                (struct_item name: (type_identifier) @node.name)
                (enum_item name: (type_identifier) @node.name)
                (function_item name: (identifier) @func.name)
                (mod_item name: (identifier) @node.name)
                (use_declaration argument: [(scoped_identifier) @edge.target (identifier) @edge.target])
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["go"],
            language: tree_sitter::Language::new(tree_sitter_go::LANGUAGE),
            default_node_kind: "package",
            default_edge_kind: "import",
            queries: r#"
                (package_clause (package_identifier) @node.name)
                (function_declaration name: (identifier) @func.name)
                (method_declaration name: (field_identifier) @func.name)
                (import_spec path: (string_literal) @edge.target)
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["java"],
            language: tree_sitter::Language::new(tree_sitter_java::LANGUAGE),
            default_node_kind: "class",
            default_edge_kind: "import",
            queries: r#"
                (class_declaration name: (identifier) @node.name)
                (interface_declaration name: (identifier) @node.name)
                (method_declaration name: (identifier) @func.name)
                (import_declaration (scoped_identifier) @edge.target)
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["cpp", "hpp", "cc", "h"],
            language: tree_sitter::Language::new(tree_sitter_cpp::LANGUAGE),
            default_node_kind: "module",
            default_edge_kind: "include",
            queries: r#"
                (class_specifier name: (type_identifier) @node.name)
                (function_definition declarator: (function_declarator declarator: (identifier) @func.name))
                (preproc_include path: [(string_literal) @edge.target (system_lib_string) @edge.target])
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["php"],
            language: tree_sitter::Language::new(tree_sitter_php::LANGUAGE_PHP),
            default_node_kind: "class",
            default_edge_kind: "use",
            queries: r#"
                (class_declaration name: (identifier) @node.name)
                (method_declaration name: (identifier) @func.name)
                (namespace_use_declaration (namespace_use_clause (qualified_name) @edge.target))
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["cs"],
            language: tree_sitter::Language::new(tree_sitter_c_sharp::LANGUAGE),
            default_node_kind: "class",
            default_edge_kind: "using",
            queries: r#"
                (class_declaration name: (identifier) @node.name)
                (method_declaration name: (identifier) @func.name)
                (using_directive (identifier) @edge.target)
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["clj", "cljs", "cljc", "edn"],
            language: tree_sitter::Language::new(tree_sitter_clojure::LANGUAGE),
            default_node_kind: "namespace",
            default_edge_kind: "require",
            queries: r#"
                (list_lit (symbol) @ns_kw (#eq? @ns_kw "ns") (symbol) @node.name)
                (list_lit (symbol) @defn_kw (#match? @defn_kw "^defn-?$") (symbol) @func.name)
                (list_lit (keyword) @req_kw (#eq? @req_kw ":require") (list_lit (symbol) @edge.target))
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["kt", "kts"],
            language: tree_sitter::Language::new(tree_sitter_kotlin_ng::LANGUAGE),
            default_node_kind: "class",
            default_edge_kind: "import",
            queries: r#"
                (package_header (identifier) @node.name)
                (class_declaration (type_identifier) @node.name)
                (function_declaration (identifier) @func.name)
                (import_header (identifier) @edge.target)
            "#,
        }),
    ];

    let graph_start = Instant::now();
    let g = perform_graph_build(&parsers, repo.clone(), Some("@".to_string()), false);
    let fp = graph::source_fingerprint(&repo);
    let branch = vcs::get_current_branch(&repo);
    let revision = vcs::get_current_revision(&repo);
    if debug_enabled() {
        println!(
            "[BACKEND][boot] initial graph/build metadata done nodes={} edges={} branch='{}' rev='{}' took={}ms",
            g.nodes.len(),
            g.edges.len(),
            branch,
            if revision.len() > 12 { &revision[..12] } else { &revision },
            graph_start.elapsed().as_millis()
        );
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            repo: Mutex::new(repo),
            parsers,
            last_fingerprint: Mutex::new(fp),
            watcher: Mutex::new(None),
            graph: Mutex::new(g),
            last_since: Mutex::new(Some("@".to_string())),
            last_include_neighbors: Mutex::new(false),
            last_branch: Mutex::new(branch),
            last_revision: Mutex::new(revision),
        })
        .invoke_handler(tauri::generate_handler![
            get_graph,
            get_changes,
            get_bookmarks,
            get_current_branch,
            get_file_diff,
            get_file_source,
            save_file,
            get_repo_path,
            select_repo,
        ])
        .setup(|app| {
            if debug_enabled() {
                println!("[BACKEND][boot] tauri setup start");
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(5));
                refresh_vcs_state(&handle);
            });

            let watcher = setup_watcher(app.handle());
            let state = app.state::<AppState>();
            *state.watcher.lock() = watcher;
            if debug_enabled() {
                println!("[BACKEND][boot] tauri setup complete");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run codegraph");

    if debug_enabled() {
        println!(
            "[BACKEND][boot] run exited after {}ms",
            boot_start.elapsed().as_millis()
        );
    }
}
