# Agent Graph Intelligence — Implementation Plan

Status: superseded by [`ROADMAP.md`](../ROADMAP.md) for active execution ordering.  
Decision update (2026-03-16): `CozoDB` is rejected for immediate foundation work; active path is `SQLite + GraphStore + GraphViewService`.

**Goal:** Evolve Sloposcope from a visual-only codebase explorer into a live structural intelligence backend that serves both humans (Tauri UI) and AI agents (MCP server).

## Implemented vs open (2026-03-17)

Implemented in code:
- canonical graph on SQLite with `GraphStore` / `GraphViewService`
- `Atlas` / `Investigation`, provenance, approximate historical mode
- graph-backed watchdog evidence and graph query helpers
- `sloposcope-mcp` binary with graph/watchdog/work-unit tools
- first clustering slice: persisted `cluster_member`, Julia sidecar contract, heuristic fallback
- first search slice: Tantivy BM25 index plus Tauri/MCP query APIs
- settings UI reshaped into tabs instead of one stacked control page

Still open:
- global graph-search UI on the main canvas page
- cluster-quality upgrades beyond the first connected-component Julia pass
- MCP additions like `impact`, `list_clusters`, and raw query passthrough
- optional semantic search / embeddings / reranking

---

## Phase 1: Embedded Graph Store (SQLite)

The foundation. Persist the graph so it survives restarts and becomes queryable.

- [x] Add `rusqlite` crate to `Cargo.toml` (bundled SQLite)
- [x] Create storage boundary:
  - [x] `src-tauri/src/graph_store.rs` for canonical entities/edges and `SqliteGraphStore`
  - [x] `src-tauri/src/graph_view.rs` for `Atlas` / `Investigation` derivation
- [x] Schema: `entity(id, kind, name, display_name, file_path, language, parent_id, start_line, end_line, line_count)`
- [x] Schema: `edge(src_id, dst_id, kind, confidence, source_file)`
- [x] Schema: `repo_meta(key, value)`
- [x] `open_or_create(repo_path)` stores DB in `.sloposcope/graph.sqlite3`
- [x] File-scoped `remove_file(...)` before re-parse
- [x] Wire into `lib.rs`: repo open and watcher flush update canonical graph store; UI reads derived views
- [x] Verify: existing UI keeps working, DB file appears in `.sloposcope/`

## Phase 2: Richer Edge Resolution

Cross-file resolution to make the graph actually useful for agents.

- [ ] Extend `parser.rs` / `PluggableParser` to extract:
  - [ ] Import/export relationships (not just calls)
  - [ ] Class heritage (extends/implements)
  - [ ] Constructor inference (`self`/`this` receiver types)
  - [ ] Type annotations on function params/returns
- [ ] Store edge confidence scores (direct call = 0.9, import = 0.7, inferred = 0.5)
- [x] Add `edge.confidence` field to stored edge facts
- [ ] New Datalog queries:
  - [ ] `upstream(target, depth)` — what depends on this symbol
  - [ ] `downstream(source, depth)` — what does this symbol depend on
  - [ ] `blast_radius(target, max_depth, min_confidence)` — impact analysis

## Phase 3: Julia Sidecar for Clustering & Scoring

Math-heavy algorithms via Julia subprocess. Optional dependency — degrade gracefully.

- [ ] Create `scripts/cluster.jl`
  - [ ] Read adjacency list (JSON via stdin)
  - [ ] Run Louvain/Leiden community detection (`CommunityDetection.jl`)
  - [ ] Output cluster assignments (JSON via stdout)
- [ ] Create `scripts/Project.toml` with Julia deps
- [ ] Create `src-tauri/src/julia_sidecar.rs`
  - [ ] `detect_julia()` — check if `julia` is on PATH
  - [ ] `run_clustering(graph) -> HashMap<NodeId, ClusterId>` — subprocess call
  - [ ] Timeout + error handling (if Julia missing, fall back to extension-based)
- [ ] Store cluster assignments in graph store
- [ ] Trigger re-clustering on:
  - [ ] Project open
  - [ ] Manual trigger (UI button / Tauri command)
  - [ ] Optionally on large change batches (reuse watchdog infra)
- [ ] Feed cluster colors into the Svelte force graph (visual upgrade for free)

## Phase 4: MCP Server

Expose the graph to AI agents. The killer feature: live data, not stale indexes.

- [ ] Add MCP server capability to the Tauri app (stdio or HTTP)
  - [ ] Evaluate: run as separate process spawned by Tauri, or embed in Tauri binary
  - [ ] Preference: separate lightweight binary (`sloposcope-mcp`) that reads from same `.sloposcope/` DB
- [ ] Implement MCP tools:
  - [ ] `query` — hybrid search over nodes/edges (start with substring match, add BM25 later)
  - [ ] `context` — 360° symbol view: incoming/outgoing edges, cluster membership
  - [ ] `impact` — blast radius with depth grouping and confidence filtering
  - [ ] `detect_changes` — git-diff mapped to affected nodes/clusters (reuse `vcs.rs`)
  - [ ] `list_clusters` — all detected communities with member counts
  - [ ] `datalog` — raw query passthrough for power users
- [ ] MCP resources:
  - [ ] `sloposcope://context` — repo stats, staleness, available tools
  - [ ] `sloposcope://clusters` — all clusters
  - [ ] `sloposcope://schema` — DB schema for raw queries
- [ ] Editor integration configs:
  - [ ] Claude Code: `claude mcp add sloposcope -- sloposcope-mcp`
  - [ ] Cursor: `~/.cursor/mcp.json` snippet
  - [ ] Document in README

## Phase 5: Search (BM25 + optional embeddings)

Make `query` actually good.

- [ ] Add `tantivy` crate for BM25 full-text search over node labels, file paths, function names
- [ ] Index on repo scan, update incrementally on watcher flush
- [ ] Store tantivy index alongside SQLite graph store in `.sloposcope/`
- [ ] Optional: embeddings via ONNX runtime or calling out to a local model
  - [ ] Only if Julia sidecar pattern works well — could do embeddings there too
- [ ] RRF (Reciprocal Rank Fusion) to merge BM25 + graph-proximity results

## Phase 6: Process/Flow Tracing

Trace execution flows from entry points through the call graph.

- [ ] Identify entry points per language (main functions, HTTP handlers, CLI commands, test functions)
- [ ] Walk call graph from entry points, record ordered steps
- [ ] Store as `:process { id, name, type }` and `:process_step { process_id, step_index, node_id }`
- [ ] Expose via MCP and visualize in UI (highlight a flow path on the force graph)

---

## Architecture Summary

```
Tauri App (sloposcope)
├── Rust core (existing)
│   ├── parser.rs — tree-sitter extraction (enhanced Phase 2)
│   ├── graph.rs — in-memory graph (kept for UI rendering)
│   ├── vcs.rs / jj.rs — Git/JJ integration (unchanged)
│   ├── watchdog.rs — file watcher (unchanged)
│   ├── graph_store.rs — SQLite persistence (NEW, Phase 1)
│   ├── graph_view.rs — view derivation (NEW, Phase 1/2)
│   └── julia_sidecar.rs — clustering subprocess (NEW, Phase 3)
├── Svelte UI (existing, enhanced)
│   └── cluster colors fed from graph store (Phase 3)
├── sloposcope-mcp (NEW binary, Phase 4)
│   └── reads .sloposcope/ DB, serves MCP tools
└── scripts/
    ├── cluster.jl (NEW, Phase 3)
    └── Project.toml
```

## Key Differentiator

Most code intelligence tools index dead snapshots that go stale immediately. Sloposcope has a **live filesystem watcher** — the graph DB stays current as the human edits. When an agent queries impact, it gets the state of the code *right now*, not from the last manual index run. The human and the agent see the same living graph.

## Open Questions

- [ ] Should `sloposcope-mcp` be a separate binary or a Tauri command/plugin?
- [x] CozoDB vs SQLite — SQLite chosen for v1 foundation
- [ ] Julia vs Python for sidecar — Julia has better libs but bigger install footprint
- [ ] Should clustering run automatically on every watcher flush or only on demand?
- [ ] Licensing: don't copy code from other projects, implement from concepts
