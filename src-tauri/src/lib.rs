mod graph;
mod jj;
mod vcs;
mod parser;

use graph::Graph;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use parser::{Parser, PluggableParser};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

struct AppState {
    repo: Mutex<PathBuf>,
    parsers: Vec<Box<dyn Parser>>,
    last_fingerprint: Mutex<u64>,
    watcher: Mutex<Option<RecommendedWatcher>>,
}

fn perform_graph_build(parsers: &[Box<dyn Parser>], repo: PathBuf, since: Option<String>) -> Graph {
    let mut g = Graph::new();

    let files_to_process = if let Some(ref revset) = since {
        let changed = vcs::get_changed_files(&repo, revset);
        changed.keys().cloned().collect::<Vec<String>>()
    } else {
        vcs::get_all_files(&repo)
    };

    for rel_path in files_to_process {
        let full_path = repo.join(&rel_path);
        let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        
        if let Some(parser) = parsers.iter().find(|p| p.extensions().contains(&ext)) {
            if let Ok(source) = std::fs::read_to_string(&full_path) {
                let (nodes, edges) = parser.parse_file(&repo, &rel_path, &source);
                g.add_nodes(nodes);
                g.add_edges(edges);
            }
        }
    }

    // --- Post-process edges to resolve targets to IDs ---
    // Many parsers find targets like "crate::foo" or "./bar.svelte", but node IDs are "src/foo.rs" or "src/bar.svelte"
    let all_node_ids: Vec<String> = g.nodes.iter().map(|n| n.id.clone()).collect();
    
    for edge in &mut g.edges {
        let target = &edge.target;
        
        // 1. Direct match (best case)
        if all_node_ids.contains(target) {
            continue;
        }

        // 2. Try various normalizations
        let normalized = target
            .replace("::", "/")
            .replace("crate/", "")
            .trim_start_matches("./")
            .to_string();

        if let Some(found) = all_node_ids.iter().find(|id| {
            id.as_str() == normalized.as_str() || 
            id.ends_with(&normalized) || 
            id.replace("/", ".").contains(target)
        }) {
            edge.target = found.clone();
            continue;
        }

        // 3. Last resort: check if target is a substring of any ID (module match)
        if let Some(found) = all_node_ids.iter().find(|id| {
            let id_no_ext = id.split('.').next().unwrap_or(id);
            id_no_ext.contains(target) || target.contains(id_no_ext)
        }) {
            edge.target = found.clone();
        }
    }

    g.finalize();

    if let Some(revset) = since {
        let changed = vcs::get_changed_files(&repo, &revset);
        g.filter_to_changes(&changed);
    }

    g
}

fn fingerprint(repo: &PathBuf) -> u64 {
    graph::source_fingerprint(repo)
}

// --- Tauri commands ---

#[tauri::command]
async fn select_repo(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder);
    });
    
    let folder = rx.recv().map_err(|_| "Dialog cancelled".to_string())?;
    
    if let Some(path) = folder {
        let path_buf = match path {
            tauri_plugin_dialog::FilePath::Path(p) => p,
            tauri_plugin_dialog::FilePath::Url(u) => {
                u.to_file_path().map_err(|_| "Invalid URL path".to_string())?
            }
        };
        
        {
            let mut repo = state.repo.lock();
            *repo = path_buf.clone();
        }
        
        let fp = fingerprint(&path_buf);
        *state.last_fingerprint.lock() = fp;

        let watcher = setup_watcher(&app);
        *state.watcher.lock() = watcher;
        
        Ok(path_buf.display().to_string())
    } else {
        Err("No folder selected".into())
    }
}

#[tauri::command]
async fn get_graph(state: tauri::State<'_, AppState>, since: Option<String>) -> Result<Graph, String> {
    let repo = state.repo.lock().clone();
    let g = perform_graph_build(&state.parsers, repo, since);
    Ok(g)
}

#[tauri::command]
fn get_changes(state: tauri::State<AppState>, limit: Option<usize>) -> Vec<vcs::Change> {
    let repo = state.repo.lock().clone();
    vcs::get_changes(&repo, limit.unwrap_or(20))
}

#[tauri::command]
fn get_bookmarks(state: tauri::State<AppState>) -> Vec<vcs::Bookmark> {
    let repo = state.repo.lock().clone();
    vcs::get_bookmarks(&repo)
}

#[tauri::command]
fn get_current_branch(state: tauri::State<AppState>) -> String {
    let repo = state.repo.lock().clone();
    vcs::get_current_branch(&repo)
}

#[tauri::command]
fn get_file_diff(state: tauri::State<AppState>, file: String, since: Option<String>) -> String {
    let repo = state.repo.lock().clone();
    vcs::get_file_diff(&repo, &file, since.as_deref())
}

#[tauri::command]
fn get_file_source(state: tauri::State<AppState>, file: String) -> String {
    let repo = state.repo.lock().clone();
    let path = repo.join(&file);
    std::fs::read_to_string(path).unwrap_or_else(|_| "--- FILE DELETED OR INACCESSIBLE ---".to_string())
}

#[tauri::command]
fn save_file(state: tauri::State<AppState>, file: String, content: String) -> Result<(), String> {
    let repo = state.repo.lock().clone();
    let path = repo.join(&file);
    std::fs::write(path, content).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_repo_path(state: tauri::State<AppState>) -> String {
    state.repo.lock().display().to_string()
}

// --- File watcher ---

fn setup_watcher(app: &AppHandle) -> Option<RecommendedWatcher> {
    let handle = app.clone();

    let mut watcher = match notify::recommended_watcher(move |res: Result<Event, _>| {
        if let Ok(event) = res {
            let interesting = event.paths.iter().any(|p| {
                let s = p.display().to_string();
                !s.contains("/.git/") && !s.contains("/.jj/") && !s.contains("/_build/") && !s.contains("/deps/") && !s.contains("/node_modules/") && !s.contains("/target/")
            });

            if interesting {
                if let Some(state) = handle.try_state::<AppState>() {
                    let repo = state.repo.lock().clone();
                    let current = fingerprint(&repo);
                    let mut last = state.last_fingerprint.lock();
                    if current != *last {
                        *last = current;
                        let graph = perform_graph_build(&state.parsers, repo.clone(), None);
                        let changes = vcs::get_changes(&repo, 20);
                        let _ = handle.emit("graph-updated", serde_json::json!({
                            "graph": graph,
                            "changes": changes,
                        }));
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
    }

    Some(watcher)
}

// --- Entry point ---

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let repo = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("no cwd"));

    let repo = repo.canonicalize().unwrap_or(repo);

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

    let fp = graph::source_fingerprint(&repo);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            repo: Mutex::new(repo),
            parsers,
            last_fingerprint: Mutex::new(fp),
            watcher: Mutex::new(None),
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
            let watcher = setup_watcher(app.handle());
            let state = app.state::<AppState>();
            *state.watcher.lock() = watcher;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run codegraph");
}
