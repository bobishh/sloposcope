/// sloposcope-mcp — minimal MCP server over stdio.
///
/// Protocol: JSON-RPC 2.0, one message per line on stdin/stdout.
///
/// Supported methods:
///   initialize          → server info + capabilities
///   tools/list          → list available tools
///   tools/call          → invoke a tool
///
/// Tools:
///   watchdog_status     → config + canonical graph size for the given repo
///   watchdog_run        → build + evaluate a watchdog snapshot (no runtime state needed)
///   graph_neighbors     → 1-hop neighbors of an entity in the canonical graph
///   graph_blast_radius  → entities reachable from a changed file set
///   graph_context       → 360° view of an entity: parent, children, in/out edges
///   detect_changes      → map changed files to affected graph entities + blast radius
///   query               → BM25 search across canonical graph entities
use sloposcope_lib::{
    graph_query::GraphIndex,
    graph_store::{GraphStore, SqliteGraphStore},
    search_service::SearchService,
    vcs, watchdog,
    watchdog::WatchdogRuntimeState,
    watchdog_config,
    work_store::{
        derive_agent_session_runtime_slice_at, AgentSessionRuntimeRecord, NewVerificationRecord,
        NewWorkUnit, UnitVerdict, VerdictArtifact, WorkStore,
    },
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, Write};

// ── JSON-RPC types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

impl JsonRpcResponse {
    fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

// ── Tool argument helpers ─────────────────────────────────────────────────────

fn repo_path_from(params: &Value) -> Result<std::path::PathBuf, String> {
    let raw = params
        .get("repo_path")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: repo_path".to_string())?;
    let p = std::path::PathBuf::from(raw);
    if !p.exists() {
        return Err(format!("repo_path does not exist: {}", p.display()));
    }
    Ok(p)
}

fn now_utc() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("unix:{secs}")
}

fn string_array(params: &Value, key: &str) -> Vec<String> {
    params
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn load_agent_session(
    store: &WorkStore,
    mission_id: &str,
    session_id: &str,
) -> Result<AgentSessionRuntimeRecord, String> {
    store
        .list_agent_sessions(mission_id)?
        .into_iter()
        .find(|session| session.id == session_id)
        .ok_or_else(|| {
            format!("agent session not found: mission_id={mission_id} session_id={session_id}")
        })
}

fn dedupe_paths(mut paths: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
    paths
}

fn json_session(session: &AgentSessionRuntimeRecord) -> Value {
    json_session_at(session, now_utc())
}

fn json_session_at(session: &AgentSessionRuntimeRecord, now: String) -> Value {
    let now_unix_seconds = now
        .strip_prefix("unix:")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let runtime = derive_agent_session_runtime_slice_at(session, now_unix_seconds);
    serde_json::json!({
        "id": session.id,
        "missionId": session.mission_id,
        "objectiveId": session.objective_id,
        "profileId": session.profile_id,
        "assignmentKind": session.assignment_kind,
        "state": session.state,
        "runtimeStatus": runtime.runtime_status,
        "heartbeatAgeSec": runtime.heartbeat_age_sec,
        "eventAgeSec": runtime.event_age_sec,
        "leaseExpired": runtime.lease_expired,
        "touchedPaths": session.touched_paths,
        "workspacePath": session.workspace_path,
        "leaseExpiresAt": session.lease_expires_at,
        "lastHeartbeatAt": session.last_heartbeat_at,
        "lastEventAt": session.last_event_at,
        "lastError": session.last_error,
        "createdAt": session.created_at,
        "updatedAt": session.updated_at,
    })
}

fn update_agent_session<F>(
    store: &mut WorkStore,
    mission_id: &str,
    session_id: &str,
    mut apply: F,
) -> Result<AgentSessionRuntimeRecord, String>
where
    F: FnMut(&mut AgentSessionRuntimeRecord),
{
    let mut session = load_agent_session(store, mission_id, session_id)?;
    apply(&mut session);
    session.updated_at = now_utc();
    store
        .upsert_agent_session_runtime(&session)
        .map_err(|e| format!("upsert agent session: {e}"))?;
    Ok(session)
}

// ── Tool implementations ──────────────────────────────────────────────────────

fn tool_watchdog_status(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let (config, config_load) = watchdog_config::load_repo_config(&repo);
    let graph_snap = SqliteGraphStore::open_or_create(&repo)
        .ok()
        .and_then(|store| store.load_snapshot().ok());

    let entity_count = graph_snap.as_ref().map(|s| s.entities.len()).unwrap_or(0);
    let edge_count = graph_snap.as_ref().map(|s| s.edges.len()).unwrap_or(0);
    let clustering_mode = graph_snap
        .as_ref()
        .and_then(|s| s.repo_meta.get("clustering_mode"))
        .cloned()
        .unwrap_or_else(|| "heuristic".to_string());

    Ok(serde_json::json!({
        "config": config,
        "configLoad": config_load,
        "graphEntities": entity_count,
        "graphEdges": edge_count,
        "clusteringMode": clustering_mode,
    }))
}

fn tool_query(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let query = params
        .get("query")
        .or_else(|| params.get("q"))
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: query".to_string())?;
    let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;

    let store = SqliteGraphStore::open_or_create(&repo).map_err(|e| format!("graph store: {e}"))?;
    let snapshot = store
        .load_snapshot()
        .map_err(|e| format!("load snapshot: {e}"))?;
    let search =
        SearchService::open_or_create(&repo).map_err(|e| format!("open search index: {e}"))?;
    search
        .rebuild_from_snapshot(&snapshot)
        .map_err(|e| format!("rebuild search index: {e}"))?;
    let results = search
        .query(query, limit)
        .map_err(|e| format!("search query: {e}"))?;

    Ok(serde_json::json!({
        "query": query,
        "limit": limit,
        "results": results.iter().map(|result| serde_json::json!({
            "entityId": result.entity_id,
            "displayName": result.display_name,
            "filePath": result.file_path,
            "language": result.language,
            "score": result.score,
            "why": result.why,
        })).collect::<Vec<_>>(),
    }))
}

fn tool_graph_neighbors(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let entity_id = params
        .get("entity_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: entity_id".to_string())?;
    let max_hops = params.get("max_hops").and_then(Value::as_u64).unwrap_or(1) as usize;

    let store = SqliteGraphStore::open_or_create(&repo).map_err(|e| format!("graph store: {e}"))?;
    let snap = store
        .load_snapshot()
        .map_err(|e| format!("load snapshot: {e}"))?;
    let index = GraphIndex::build(&snap);

    let neighbors = index.neighbors(entity_id, max_hops);
    let result: Vec<Value> = neighbors
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "displayName": e.display_name,
                "kind": format!("{:?}", e.kind),
                "filePath": e.file_path,
            })
        })
        .collect();

    Ok(serde_json::json!({ "entityId": entity_id, "maxHops": max_hops, "neighbors": result }))
}

fn tool_graph_blast_radius(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let files: Vec<&str> = params
        .get("file_paths")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if files.is_empty() {
        return Err("missing param: file_paths (non-empty array of strings)".to_string());
    }
    let max_hops = params.get("max_hops").and_then(Value::as_u64).unwrap_or(2) as usize;

    let store = SqliteGraphStore::open_or_create(&repo).map_err(|e| format!("graph store: {e}"))?;
    let snap = store
        .load_snapshot()
        .map_err(|e| format!("load snapshot: {e}"))?;
    let index = GraphIndex::build(&snap);

    let blast = index.blast_radius(&files, max_hops);
    let result: Vec<Value> = blast
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "displayName": e.display_name,
                "kind": format!("{:?}", e.kind),
                "filePath": e.file_path,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "filePaths": files,
        "maxHops": max_hops,
        "blastRadiusCount": result.len(),
        "entities": result,
    }))
}

fn tool_watchdog_run(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let since = params.get("since").and_then(Value::as_str).unwrap_or("@");

    let (config, _config_load) = watchdog_config::load_repo_config(&repo);
    let mut runtime = WatchdogRuntimeState::default();
    let snapshot = watchdog::build_snapshot_for_repo(
        &repo,
        since,
        &mut runtime,
        config.max_files_in_snapshot,
        config.max_diff_lines_per_file,
    );
    let verdict = watchdog::evaluate_snapshot(&snapshot, &config);

    Ok(serde_json::json!({
        "branch": snapshot.branch,
        "revision": snapshot.revision,
        "since": snapshot.since,
        "changedFilesTotal": snapshot.changed_files_total,
        "changedFiles": snapshot.changed_files.iter().map(|f| &f.path).collect::<Vec<_>>(),
        "affectedEntities": snapshot.affected_entities,
        "relatedEntities": snapshot.related_entities,
        "blastRadiusCount": snapshot.blast_radius_count,
        "verdict": {
            "status": format!("{:?}", verdict.status),
            "confidence": verdict.confidence,
            "summary": verdict.summary,
            "issueCount": verdict.issues.len(),
            "issues": verdict.issues.iter().map(|i| serde_json::json!({
                "kind": format!("{:?}", i.kind),
                "severity": format!("{:?}", i.severity),
                "title": i.title,
                "why": i.why,
            })).collect::<Vec<_>>(),
        }
    }))
}

fn tool_graph_context(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let entity_id = params
        .get("entity_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: entity_id".to_string())?;

    let store = SqliteGraphStore::open_or_create(&repo).map_err(|e| format!("graph store: {e}"))?;
    let snap = store
        .load_snapshot()
        .map_err(|e| format!("load snapshot: {e}"))?;

    let entity = snap
        .entities
        .iter()
        .find(|e| e.id == entity_id)
        .ok_or_else(|| format!("entity not found: {entity_id}"))?;

    let parent = entity
        .parent_id
        .as_deref()
        .and_then(|pid| snap.entities.iter().find(|e| e.id == pid));

    let children: Vec<_> = snap
        .entities
        .iter()
        .filter(|e| e.parent_id.as_deref() == Some(entity_id))
        .map(|e| serde_json::json!({ "id": e.id, "displayName": e.display_name, "kind": format!("{:?}", e.kind) }))
        .collect();

    let out_edges: Vec<_> = snap
        .edges
        .iter()
        .filter(|e| e.src_id == entity_id)
        .map(|e| serde_json::json!({ "targetId": e.dst_id, "kind": e.kind, "confidence": e.confidence }))
        .collect();

    let in_edges: Vec<_> = snap
        .edges
        .iter()
        .filter(|e| e.dst_id == entity_id)
        .map(|e| serde_json::json!({ "sourceId": e.src_id, "kind": e.kind, "confidence": e.confidence }))
        .collect();

    Ok(serde_json::json!({
        "entity": {
            "id": entity.id,
            "displayName": entity.display_name,
            "kind": format!("{:?}", entity.kind),
            "filePath": entity.file_path,
            "language": entity.language,
            "lineCount": entity.line_count,
        },
        "parent": parent.map(|p| serde_json::json!({ "id": p.id, "displayName": p.display_name })),
        "children": children,
        "outEdges": out_edges,
        "inEdges": in_edges,
    }))
}

fn tool_detect_changes(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let since = params.get("since").and_then(Value::as_str).unwrap_or("@");
    let max_hops = params.get("max_hops").and_then(Value::as_u64).unwrap_or(2) as usize;

    let changed = vcs::get_changed_files(&repo, since);
    let mut changed_vec: Vec<(String, String)> = changed.into_iter().collect();
    changed_vec.sort_by(|a, b| a.0.cmp(&b.0));

    let store = SqliteGraphStore::open_or_create(&repo).map_err(|e| format!("graph store: {e}"))?;
    let snap = store
        .load_snapshot()
        .map_err(|e| format!("load snapshot: {e}"))?;
    let index = GraphIndex::build(&snap);

    let changed_paths: Vec<&str> = changed_vec.iter().map(|(p, _)| p.as_str()).collect();
    let blast = index.blast_radius(&changed_paths, max_hops);

    let changed_files_json: Vec<Value> = changed_vec
        .iter()
        .map(|(path, status)| serde_json::json!({ "path": path, "status": status }))
        .collect();

    let blast_json: Vec<Value> = blast
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "displayName": e.display_name,
                "kind": format!("{:?}", e.kind),
                "filePath": e.file_path,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "since": since,
        "changedFilesTotal": changed_vec.len(),
        "changedFiles": changed_files_json,
        "blastRadiusCount": blast_json.len(),
        "blastRadius": blast_json,
    }))
}

fn tool_ingest_run_event(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let task_id = params
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: task_id".to_string())?;
    let unit_id = params.get("unit_id").and_then(Value::as_str).unwrap_or("");
    let session_id = params
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let event_type = params
        .get("event_type")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            "missing param: event_type (started|progress|completed|failed)".to_string()
        })?;
    let payload = params
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let store = WorkStore::open_or_create(&repo).map_err(|e| format!("work store: {e}"))?;
    let id = store
        .persist_run_event(task_id, unit_id, session_id, event_type, payload)
        .map_err(|e| format!("persist_run_event: {e}"))?;

    Ok(serde_json::json!({
        "eventId": id,
        "taskId": task_id,
        "unitId": unit_id,
        "sessionId": session_id,
        "eventType": event_type,
    }))
}

fn tool_agent_request_context(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let mission_id = params
        .get("mission_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: mission_id".to_string())?;
    let requested_objective_id = params
        .get("objective_id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let requested_session_id = params
        .get("session_id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let event_limit = params
        .get("event_limit")
        .and_then(Value::as_u64)
        .map(|value| value.clamp(1, 50) as usize)
        .unwrap_or(8);

    let store = WorkStore::open_or_create(&repo).map_err(|e| format!("work store: {e}"))?;
    let state = store
        .load_mission_state(Some(mission_id), event_limit)
        .map_err(|e| format!("load_mission_state: {e}"))?;
    let mission = state
        .mission
        .ok_or_else(|| format!("mission not found: {mission_id}"))?;
    let session = requested_session_id
        .as_deref()
        .and_then(|session_id| state.sessions.iter().find(|entry| entry.id == session_id))
        .cloned();
    let resolved_objective_id = requested_objective_id
        .or_else(|| session.as_ref().and_then(|entry| entry.objective_id.clone()));
    let objective = resolved_objective_id
        .as_deref()
        .and_then(|objective_id| state.objectives.iter().find(|entry| entry.id == objective_id))
        .cloned();
    let instruction_pack = watchdog::load_agents_context(&repo);
    let scope_graph = if let Some(objective) = objective.as_ref() {
            let graph_store = SqliteGraphStore::open_or_create(&repo)
                .map_err(|e| format!("graph store: {e}"))?;
            let snapshot = graph_store
                .load_snapshot()
                .map_err(|e| format!("load canonical graph: {e}"))?;
            let index = GraphIndex::build(&snapshot);
            objective
                .meta
                .scope_paths
                .iter()
                .map(|scope_path| {
                    let related = index
                        .blast_radius(&[scope_path.as_str()], 1)
                        .into_iter()
                        .map(|entity| {
                            serde_json::json!({
                                "id": entity.id,
                                "kind": entity.kind,
                                "displayName": entity.display_name,
                                "filePath": entity.file_path,
                            })
                        })
                        .collect::<Vec<_>>();
                    serde_json::json!({
                        "scopePath": scope_path,
                        "relatedEntities": related,
                    })
                })
                .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    Ok(serde_json::json!({
        "mission": mission,
        "objective": objective,
        "session": session.as_ref().map(json_session),
        "instructionPack": {
            "source": instruction_pack.load.path,
            "rules": instruction_pack.rules,
        },
        "recentEvents": state.events,
        "recentVerifications": state.verifications,
        "scopeGraph": scope_graph,
        "summary": {
            "missionState": state.summary,
            "objectiveCount": state.objectives.len(),
            "sessionCount": state.sessions.len(),
        }
    }))
}

fn tool_agent_login(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let mission_id = params
        .get("mission_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: mission_id".to_string())?;
    let session_id = params
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: session_id".to_string())?;
    let profile_id = params
        .get("profile_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: profile_id".to_string())?;
    let assignment_kind = params
        .get("assignment_kind")
        .and_then(Value::as_str)
        .unwrap_or("execute_objective");
    let objective_id = params
        .get("objective_id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let state = params
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("running");
    let touched_paths = string_array(params, "touched_paths");
    let workspace_path = params
        .get("workspace_path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let lease_expires_at = params
        .get("lease_expires_at")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let now = now_utc();

    let mut store = WorkStore::open_or_create(&repo).map_err(|e| format!("work store: {e}"))?;
    let session = AgentSessionRuntimeRecord {
        id: session_id.to_string(),
        mission_id: mission_id.to_string(),
        objective_id,
        profile_id: profile_id.to_string(),
        assignment_kind: assignment_kind.to_string(),
        state: state.to_string(),
        touched_paths: dedupe_paths(touched_paths),
        workspace_path,
        lease_expires_at,
        last_heartbeat_at: Some(now.clone()),
        last_event_at: Some(now.clone()),
        last_error: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    store
        .upsert_agent_session_runtime(&session)
        .map_err(|e| format!("upsert agent session: {e}"))?;

    Ok(serde_json::json!({
        "loggedIn": true,
        "session": json_session(&session),
    }))
}

fn tool_agent_activity_set(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let mission_id = params
        .get("mission_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: mission_id".to_string())?;
    let session_id = params
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: session_id".to_string())?;
    let state = params
        .get("state")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let workspace_path = params
        .get("workspace_path")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let lease_expires_at = params
        .get("lease_expires_at")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let last_error = params
        .get("last_error")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let touched_paths = string_array(params, "touched_paths");
    let now = now_utc();

    let mut store = WorkStore::open_or_create(&repo).map_err(|e| format!("work store: {e}"))?;
    let session = update_agent_session(&mut store, mission_id, session_id, |session| {
        if let Some(state) = state.as_deref() {
            session.state = state.to_string();
        }
        if let Some(workspace_path) = workspace_path.as_deref() {
            session.workspace_path = workspace_path.to_string();
        }
        if let Some(lease_expires_at) = lease_expires_at.as_deref() {
            session.lease_expires_at = Some(lease_expires_at.to_string());
        }
        if let Some(last_error) = last_error.as_deref() {
            session.last_error = Some(last_error.to_string());
        }
        if !touched_paths.is_empty() {
            session.touched_paths = dedupe_paths(touched_paths.clone());
        }
        session.last_heartbeat_at = Some(now.clone());
        session.last_event_at = Some(now.clone());
    })?;

    Ok(serde_json::json!({
        "updated": true,
        "session": json_session(&session),
    }))
}

fn tool_agent_report_event(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let mission_id = params
        .get("mission_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: mission_id".to_string())?;
    let session_id = params
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: session_id".to_string())?;
    let event_type = params
        .get("event_type")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: event_type".to_string())?;
    let unit_id = params.get("unit_id").and_then(Value::as_str).unwrap_or("");
    let payload = params.get("payload").cloned().unwrap_or(Value::Null);

    let mut store = WorkStore::open_or_create(&repo).map_err(|e| format!("work store: {e}"))?;
    let now = now_utc();
    let session = update_agent_session(&mut store, mission_id, session_id, |session| {
        session.last_heartbeat_at = Some(now.clone());
        session.last_event_at = Some(now.clone());
    })?;
    let event_id = store
        .persist_run_event(mission_id, unit_id, session_id, event_type, payload)
        .map_err(|e| format!("persist_run_event: {e}"))?;

    Ok(serde_json::json!({
        "eventId": event_id,
        "eventType": event_type,
        "unitId": unit_id,
        "session": json_session(&session),
    }))
}

fn tool_agent_report_touched_files(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let mission_id = params
        .get("mission_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: mission_id".to_string())?;
    let session_id = params
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: session_id".to_string())?;
    let mut touched_paths = string_array(params, "touched_paths");
    if touched_paths.is_empty() {
        return Err("missing param: touched_paths (non-empty array of strings)".to_string());
    }

    let mut store = WorkStore::open_or_create(&repo).map_err(|e| format!("work store: {e}"))?;
    let now = now_utc();
    let session = update_agent_session(&mut store, mission_id, session_id, |session| {
        let mut merged = session.touched_paths.clone();
        merged.append(&mut touched_paths);
        session.touched_paths = dedupe_paths(merged);
        session.last_heartbeat_at = Some(now.clone());
        session.last_event_at = Some(now.clone());
    })?;
    let event_id = store
        .persist_run_event(
            mission_id,
            session.objective_id.as_deref().unwrap_or(""),
            session_id,
            "touched_files",
            serde_json::json!({
                "touchedPaths": session.touched_paths,
            }),
        )
        .map_err(|e| format!("persist_run_event: {e}"))?;

    Ok(serde_json::json!({
        "eventId": event_id,
        "session": json_session(&session),
    }))
}

fn tool_agent_submit_verdict(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let mission_id = params
        .get("mission_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: mission_id".to_string())?;
    let session_id = params
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: session_id".to_string())?;
    let status = params
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("passed");
    let summary = params.get("summary").and_then(Value::as_str).unwrap_or("");
    let objective_id = params
        .get("objective_id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let details = params
        .get("details")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let artifacts: Vec<VerdictArtifact> = params
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|artifact| VerdictArtifact {
                    kind: artifact
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("text")
                        .to_string(),
                    content: artifact
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    let mut store = WorkStore::open_or_create(&repo).map_err(|e| format!("work store: {e}"))?;
    let session = load_agent_session(&store, mission_id, session_id)?;
    let objective_id = objective_id.or(session.objective_id.clone());
    let verification = NewVerificationRecord {
        mission_id: mission_id.to_string(),
        objective_id: objective_id.clone(),
        session_id: Some(session_id.to_string()),
        status: status.to_string(),
        summary: summary.to_string(),
        details: match details {
            Value::Object(mut map) => {
                map.entry("sessionId".to_string())
                    .or_insert_with(|| Value::String(session_id.to_string()));
                map.entry("profileId".to_string())
                    .or_insert_with(|| Value::String(session.profile_id.clone()));
                map.entry("workspacePath".to_string())
                    .or_insert_with(|| Value::String(session.workspace_path.clone()));
                map.entry("touchedPaths".to_string()).or_insert_with(|| {
                    Value::Array(
                        session
                            .touched_paths
                            .iter()
                            .cloned()
                            .map(Value::String)
                            .collect(),
                    )
                });
                Value::Object(map)
            }
            other => other,
        },
    };
    let verification_id = store
        .insert_verification(&verification)
        .map_err(|e| format!("insert_verification: {e}"))?;
    let now = now_utc();
    let objective_id =
        objective_id.ok_or_else(|| "missing objective id for verdict".to_string())?;
    let verdict = UnitVerdict {
        unit_id: objective_id.clone(),
        task_id: mission_id.to_string(),
        status: if status == "passed" || status == "done" {
            "done".to_string()
        } else {
            "failed".to_string()
        },
        summary: summary.to_string(),
        submitted_at: now.clone(),
        artifacts: artifacts.clone(),
    };
    store
        .submit_verdict(verdict)
        .map_err(|e| format!("submit_verdict: {e}"))?;
    let session = update_agent_session(&mut store, mission_id, session_id, |session| {
        session.state = if status == "passed" {
            "completed".to_string()
        } else if status == "failed" {
            "failed".to_string()
        } else {
            status.to_string()
        };
        session.last_heartbeat_at = Some(now.clone());
        session.last_event_at = Some(now.clone());
    })?;
    let event_id = store
        .persist_run_event(
            mission_id,
            &objective_id,
            session_id,
            "verification_submitted",
            serde_json::json!({
                "status": status,
                "summary": summary,
                "verificationId": verification_id,
                "artifacts": artifacts,
            }),
        )
        .map_err(|e| format!("persist_run_event: {e}"))?;

    Ok(serde_json::json!({
        "verificationId": verification_id,
        "eventId": event_id,
        "session": json_session(&session),
    }))
}

fn tool_agent_logout(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let mission_id = params
        .get("mission_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: mission_id".to_string())?;
    let session_id = params
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: session_id".to_string())?;
    let state = params
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("completed");
    let reason = params.get("reason").and_then(Value::as_str).unwrap_or("");

    let mut store = WorkStore::open_or_create(&repo).map_err(|e| format!("work store: {e}"))?;
    let now = now_utc();
    let session = update_agent_session(&mut store, mission_id, session_id, |session| {
        session.state = state.to_string();
        session.lease_expires_at = None;
        session.last_heartbeat_at = Some(now.clone());
        session.last_event_at = Some(now.clone());
    })?;
    let event_id = store
        .persist_run_event(
            mission_id,
            session.objective_id.as_deref().unwrap_or(""),
            session_id,
            "logout",
            serde_json::json!({
                "state": state,
                "reason": reason,
            }),
        )
        .map_err(|e| format!("persist_run_event: {e}"))?;

    Ok(serde_json::json!({
        "eventId": event_id,
        "session": json_session(&session),
    }))
}

fn tool_get_run_events(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let task_id = params
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: task_id".to_string())?;
    let unit_id = params.get("unit_id").and_then(Value::as_str);
    let session_id = params.get("session_id").and_then(Value::as_str);
    let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize;

    let store = WorkStore::open_or_create(&repo).map_err(|e| format!("work store: {e}"))?;

    let events = if let Some(sid) = session_id {
        store.get_run_events_for_session(task_id, sid, limit)
    } else {
        store.get_run_events(task_id, unit_id, limit)
    }
    .map_err(|e| format!("get_run_events: {e}"))?;

    let events_json: Vec<Value> = events
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "unitId": e.unit_id,
                "sessionId": e.session_id,
                "eventType": e.event_type,
                "payload": e.payload,
                "createdAt": e.created_at,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "taskId": task_id,
        "total": events.len(),
        "events": events_json,
    }))
}

fn tool_plan_task(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let task_id = params
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: task_id".to_string())?;
    let title = params
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(task_id);
    let prompt = params.get("prompt").and_then(Value::as_str).unwrap_or("");
    let units_raw = params
        .get("units")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing param: units (non-empty array)".to_string())?;
    if units_raw.is_empty() {
        return Err("units must be non-empty".to_string());
    }

    let units: Vec<NewWorkUnit> = units_raw
        .iter()
        .enumerate()
        .map(|(i, u)| {
            let id = u
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("unit-{i}"));
            let title = u
                .get("title")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("Unit {i}"));
            let prompt = u
                .get("prompt")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            NewWorkUnit { id, title, prompt }
        })
        .collect();

    let mut store = WorkStore::open_or_create(&repo).map_err(|e| format!("work store: {e}"))?;
    store
        .plan_task(task_id, title, prompt, &units)
        .map_err(|e| format!("plan_task: {e}"))?;

    Ok(serde_json::json!({
        "taskId": task_id,
        "title": title,
        "unitsCreated": units.len(),
        "units": units.iter().map(|u| serde_json::json!({ "id": u.id, "title": u.title })).collect::<Vec<_>>(),
    }))
}

fn tool_claim_work_unit(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let task_id = params
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: task_id".to_string())?;
    let agent_id = params
        .get("agent_id")
        .and_then(Value::as_str)
        .unwrap_or("anonymous");

    let mut store = WorkStore::open_or_create(&repo).map_err(|e| format!("work store: {e}"))?;
    let unit = store
        .claim_work_unit(task_id, agent_id)
        .map_err(|e| format!("claim_work_unit: {e}"))?;

    match unit {
        Some(u) => Ok(serde_json::json!({
            "claimed": true,
            "unit": {
                "id": u.id,
                "taskId": u.task_id,
                "title": u.title,
                "prompt": u.prompt,
                "claimedBy": u.claimed_by,
                "claimedAt": u.claimed_at,
            }
        })),
        None => Ok(serde_json::json!({ "claimed": false, "unit": null })),
    }
}

fn tool_submit_verdict(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let task_id = params
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: task_id".to_string())?;
    let unit_id = params
        .get("unit_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: unit_id".to_string())?;
    let status = params
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("done");
    let summary = params.get("summary").and_then(Value::as_str).unwrap_or("");
    let artifacts: Vec<VerdictArtifact> = params
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|a| VerdictArtifact {
                    kind: a
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("text")
                        .to_string(),
                    content: a
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    use std::time::{SystemTime, UNIX_EPOCH};
    let now = format!(
        "unix:{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );

    let verdict = UnitVerdict {
        unit_id: unit_id.to_string(),
        task_id: task_id.to_string(),
        status: status.to_string(),
        summary: summary.to_string(),
        submitted_at: now,
        artifacts,
    };

    let mut store = WorkStore::open_or_create(&repo).map_err(|e| format!("work store: {e}"))?;
    store
        .submit_verdict(verdict)
        .map_err(|e| format!("submit_verdict: {e}"))?;

    Ok(serde_json::json!({
        "taskId": task_id,
        "unitId": unit_id,
        "status": status,
        "summary": summary,
    }))
}

fn tool_graph_clusters(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let store = SqliteGraphStore::open_or_create(&repo).map_err(|e| format!("graph store: {e}"))?;
    let members = store
        .load_clusters()
        .map_err(|e| format!("load clusters: {e}"))?;
    let snap = store
        .load_snapshot()
        .map_err(|e| format!("load snapshot: {e}"))?;
    let clustering_mode = snap
        .repo_meta
        .get("clustering_mode")
        .cloned()
        .unwrap_or_else(|| "heuristic".to_string());

    let mut clusters: std::collections::HashMap<String, (Option<String>, Vec<String>)> =
        std::collections::HashMap::new();
    for member in &members {
        let entry = clusters
            .entry(member.cluster_id.clone())
            .or_insert((member.label.clone(), Vec::new()));
        entry.1.push(member.entity_id.clone());
    }

    let mut cluster_keys: Vec<String> = clusters.keys().cloned().collect();
    cluster_keys.sort();

    let clusters_json: Vec<Value> = cluster_keys
        .iter()
        .map(|cid| {
            let (label, entity_ids) = &clusters[cid];
            serde_json::json!({
                "clusterId": cid,
                "label": label,
                "size": entity_ids.len(),
                "entityIds": entity_ids,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "mode": clustering_mode,
        "clusterCount": clusters_json.len(),
        "assignmentCount": members.len(),
        "clusters": clusters_json,
    }))
}

fn tool_list_work_units(params: &Value) -> Result<Value, String> {
    let repo = repo_path_from(params)?;
    let task_id = params
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing param: task_id".to_string())?;

    let store = WorkStore::open_or_create(&repo).map_err(|e| format!("work store: {e}"))?;
    let units = store
        .list_work_units(task_id)
        .map_err(|e| format!("list_work_units: {e}"))?;

    let pending = units
        .iter()
        .filter(|u| {
            matches!(
                u.status,
                sloposcope_lib::work_store::WorkUnitStatus::Pending
            )
        })
        .count();
    let claimed = units
        .iter()
        .filter(|u| {
            matches!(
                u.status,
                sloposcope_lib::work_store::WorkUnitStatus::Claimed
            )
        })
        .count();
    let done = units
        .iter()
        .filter(|u| matches!(u.status, sloposcope_lib::work_store::WorkUnitStatus::Done))
        .count();
    let failed = units
        .iter()
        .filter(|u| matches!(u.status, sloposcope_lib::work_store::WorkUnitStatus::Failed))
        .count();

    let units_json: Vec<Value> = units
        .iter()
        .map(|u| {
            serde_json::json!({
                "id": u.id,
                "title": u.title,
                "status": format!("{:?}", u.status).to_ascii_lowercase(),
                "claimedBy": u.claimed_by,
                "claimedAt": u.claimed_at,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "taskId": task_id,
        "total": units.len(),
        "summary": { "pending": pending, "claimed": claimed, "done": done, "failed": failed },
        "units": units_json,
    }))
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use sloposcope_lib::work_store::MissionRecord;
    use tempfile::TempDir;

    fn tmp_repo() -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().to_path_buf();
        (dir, repo)
    }

    fn seed_mission(store: &mut WorkStore, mission_id: &str) -> MissionRecord {
        let mission = MissionRecord {
            id: mission_id.to_string(),
            repo_path: "/repo".into(),
            worktree_path: "/repo".into(),
            graph_snapshot: "@".into(),
            title: "Mission".into(),
            brief: "Brief".into(),
            validation_command: "npm test".into(),
            planner_notes: None,
            planner_profile_id: None,
            planner_prompt_override: None,
            issue_key: None,
            issue_url: None,
            instruction_pack_hash: "abc123".into(),
            state: "running".into(),
            created_at: "unix:1".into(),
            updated_at: "unix:1".into(),
        };
        store.upsert_mission(&mission).expect("upsert mission");
        mission
    }

    #[test]
    fn tools_list_exposes_agent_slice() {
        let tools = tools_list();
        let names: Vec<String> = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str().map(ToString::to_string))
            .collect();
        assert!(names.contains(&"agent_login".to_string()));
        assert!(names.contains(&"agent_activity_set".to_string()));
        assert!(names.contains(&"agent_report_event".to_string()));
        assert!(names.contains(&"agent_report_touched_files".to_string()));
        assert!(names.contains(&"agent_submit_verdict".to_string()));
        assert!(names.contains(&"agent_logout".to_string()));
        assert!(names.contains(&"agent_request_context".to_string()));
    }

    #[test]
    fn agent_session_tools_roundtrip() {
        let (dir, repo) = tmp_repo();
        let mut store = WorkStore::open_or_create(&repo).expect("open store");
        seed_mission(&mut store, "mission-mcp");
        store
            .plan_task(
                "mission-mcp",
                "Mission",
                "Prompt",
                &[NewWorkUnit {
                    id: "obj-1".into(),
                    title: "Objective".into(),
                    prompt: "Do thing".into(),
                }],
            )
            .expect("plan task");
        drop(store);

        let repo_path = dir.path().display().to_string();
        let login = tool_agent_login(&serde_json::json!({
            "repo_path": repo_path.clone(),
            "mission_id": "mission-mcp",
            "session_id": "sess-1",
            "profile_id": "worker-7",
            "assignment_kind": "execute_objective",
            "objective_id": "obj-1",
            "touched_paths": ["src/lib.rs"],
        }))
        .expect("login");
        assert_eq!(login["loggedIn"], true);
        assert_eq!(login["session"]["runtimeStatus"], "running");
        assert_eq!(login["session"]["leaseExpired"], false);

        let activity = tool_agent_activity_set(&serde_json::json!({
            "repo_path": repo_path.clone(),
            "mission_id": "mission-mcp",
            "session_id": "sess-1",
            "state": "running",
            "workspace_path": "/tmp/work",
            "lease_expires_at": "unix:9"
        }))
        .expect("activity");
        assert_eq!(activity["session"]["workspacePath"], "/tmp/work");

        let event = tool_agent_report_event(&serde_json::json!({
            "repo_path": repo_path.clone(),
            "mission_id": "mission-mcp",
            "session_id": "sess-1",
            "unit_id": "obj-1",
            "event_type": "started",
            "payload": { "message": "go" }
        }))
        .expect("event");
        assert_eq!(event["eventType"], "started");

        let touched = tool_agent_report_touched_files(&serde_json::json!({
            "repo_path": repo_path.clone(),
            "mission_id": "mission-mcp",
            "session_id": "sess-1",
            "touched_paths": ["src/main.rs", "src/lib.rs"]
        }))
        .expect("touched");
        assert_eq!(
            touched["session"]["touchedPaths"].as_array().unwrap().len(),
            2
        );

        let verdict = tool_agent_submit_verdict(&serde_json::json!({
            "repo_path": repo_path.clone(),
            "mission_id": "mission-mcp",
            "session_id": "sess-1",
            "objective_id": "obj-1",
            "status": "passed",
            "summary": "green",
            "details": { "validation": { "success": true } },
            "artifacts": [{ "kind": "text", "content": "ok" }]
        }))
        .expect("verdict");
        assert!(verdict["verificationId"].as_i64().unwrap() > 0);

        let logout = tool_agent_logout(&serde_json::json!({
            "repo_path": repo_path.clone(),
            "mission_id": "mission-mcp",
            "session_id": "sess-1",
            "state": "completed",
            "reason": "done"
        }))
        .expect("logout");
        assert_eq!(logout["session"]["state"], "completed");
        assert_eq!(logout["session"]["runtimeStatus"], "completed");

        let store = WorkStore::open_or_create(&repo).expect("reopen store");
        let sessions = store.list_agent_sessions("mission-mcp").expect("sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "sess-1");
        assert_eq!(sessions[0].state, "completed");
        assert_eq!(sessions[0].touched_paths, vec!["src/lib.rs", "src/main.rs"]);
        let verifications = store
            .list_verifications("mission-mcp")
            .expect("verifications");
        assert_eq!(verifications.len(), 1);
        assert_eq!(verifications[0].status, "passed");
        let units = store.list_work_units("mission-mcp").expect("units");
        assert_eq!(
            units[0].status,
            sloposcope_lib::work_store::WorkUnitStatus::Done
        );
        let events = store
            .get_run_events("mission-mcp", None, 20)
            .expect("events");
        assert_eq!(events.len(), 4);
    }

    #[test]
    fn agent_request_context_returns_mission_objective_and_agents_rules() {
        let (dir, repo) = tmp_repo();
        std::fs::write(
            dir.path().join("AGENTS.md"),
            "Rule one.\nRule two.\n",
        )
        .expect("agents");
        let mut store = WorkStore::open_or_create(&repo).expect("open store");
        let mission = seed_mission(&mut store, "mission-context");
        store
            .plan_task(
                "mission-context",
                "Mission",
                "Prompt",
                &[NewWorkUnit {
                    id: "obj-ctx".into(),
                    title: "Objective".into(),
                    prompt: "Do thing".into(),
                }],
            )
            .expect("plan task");
        drop(store);

        let context = tool_agent_request_context(&serde_json::json!({
            "repo_path": dir.path().display().to_string(),
            "mission_id": mission.id,
            "objective_id": "obj-ctx",
            "event_limit": 5
        }))
        .expect("context");

        assert_eq!(context["mission"]["title"], "Mission");
        assert_eq!(context["objective"]["id"], "obj-ctx");
        assert_eq!(context["instructionPack"]["rules"][0], "Rule one.");
        assert_eq!(context["instructionPack"]["rules"][1], "Rule two.");
    }

    #[test]
    fn agent_activity_set_fails_when_session_missing() {
        let (dir, repo) = tmp_repo();
        let mut store = WorkStore::open_or_create(&repo).expect("open store");
        let mission = seed_mission(&mut store, "mission-missing");
        drop(store);

        let err = tool_agent_activity_set(&serde_json::json!({
            "repo_path": dir.path().display().to_string(),
            "mission_id": mission.id,
            "session_id": "absent"
        }))
        .expect_err("missing session");
        assert!(err.contains("agent session not found"));
    }
}

// ── Tool registry ─────────────────────────────────────────────────────────────

fn tools_list() -> Value {
    serde_json::json!({
        "tools": [
            {
                "name": "watchdog_status",
                "description": "Returns watchdog config and canonical graph size for a repository.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string", "description": "Absolute path to the repository root." }
                    },
                    "required": ["repo_path"]
                }
            },
            {
                "name": "graph_neighbors",
                "description": "Returns entities adjacent to a given entity in the canonical graph.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "entity_id": { "type": "string", "description": "Entity ID (usually the file path or function id)." },
                        "max_hops": { "type": "integer", "default": 1, "description": "BFS depth (1 = direct neighbors)." }
                    },
                    "required": ["repo_path", "entity_id"]
                }
            },
            {
                "name": "graph_blast_radius",
                "description": "Returns all entities reachable from a set of changed files within max_hops.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "file_paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Relative file paths to trace the blast radius from."
                        },
                        "max_hops": { "type": "integer", "default": 2 }
                    },
                    "required": ["repo_path", "file_paths"]
                }
            },
            {
                "name": "watchdog_run",
                "description": "Builds and evaluates a watchdog snapshot for a repository. Returns verdict, issues, and graph evidence.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string", "description": "Absolute path to the repository root." },
                        "since": { "type": "string", "default": "@", "description": "Git ref to compare from. '@' means current branch base." }
                    },
                    "required": ["repo_path"]
                }
            },
            {
                "name": "graph_context",
                "description": "Returns a 360° view of an entity: its parent, children, and all incoming/outgoing edges.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "entity_id": { "type": "string", "description": "Entity ID to look up (e.g. a function or module id)." }
                    },
                    "required": ["repo_path", "entity_id"]
                }
            },
            {
                "name": "detect_changes",
                "description": "Detects changed files since a git ref and maps them to affected graph entities via blast radius.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "since": { "type": "string", "default": "@", "description": "Git ref to diff from." },
                        "max_hops": { "type": "integer", "default": 2, "description": "BFS depth for blast radius." }
                    },
                    "required": ["repo_path"]
                }
            },
            {
                "name": "query",
                "description": "Search canonical graph entities by display name, entity name, file path, and language.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "default": 10 }
                    },
                    "required": ["repo_path", "query"]
                }
            },
            {
                "name": "ingest_run_event",
                "description": "Record a run event from an external runner (started/progress/completed/failed) for a work unit.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "task_id": { "type": "string" },
                        "unit_id": { "type": "string", "description": "Work unit this event belongs to (empty for task-level events)." },
                        "session_id": { "type": "string", "description": "Runner session identifier." },
                        "event_type": { "type": "string", "enum": ["started", "progress", "completed", "failed"] },
                        "payload": { "type": "object", "description": "Event-specific data (e.g. {message} for progress, {summary} for completed, {error} for failed)." }
                    },
                    "required": ["repo_path", "task_id", "event_type"]
                }
            },
            {
                "name": "agent_login",
                "description": "Create or replace an active agent session for a mission.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "mission_id": { "type": "string" },
                        "session_id": { "type": "string" },
                        "profile_id": { "type": "string" },
                        "assignment_kind": { "type": "string", "default": "execute_objective" },
                        "objective_id": { "type": "string" },
                        "state": { "type": "string", "default": "running" },
                        "workspace_path": { "type": "string" },
                        "lease_expires_at": { "type": "string" },
                        "touched_paths": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["repo_path", "mission_id", "session_id", "profile_id"]
                }
            },
            {
                "name": "agent_request_context",
                "description": "Return distilled mission, objective, AGENTS rules, recent events, and scope-graph facts for an agent session.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "mission_id": { "type": "string" },
                        "objective_id": { "type": "string" },
                        "session_id": { "type": "string" },
                        "event_limit": { "type": "integer", "default": 8 }
                    },
                    "required": ["repo_path", "mission_id"]
                }
            },
            {
                "name": "agent_activity_set",
                "description": "Update an existing agent session heartbeat and activity state.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "mission_id": { "type": "string" },
                        "session_id": { "type": "string" },
                        "state": { "type": "string" },
                        "workspace_path": { "type": "string" },
                        "lease_expires_at": { "type": "string" },
                        "last_error": { "type": "string" },
                        "touched_paths": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["repo_path", "mission_id", "session_id"]
                }
            },
            {
                "name": "agent_report_event",
                "description": "Persist a mission-scoped event and refresh the session heartbeat.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "mission_id": { "type": "string" },
                        "session_id": { "type": "string" },
                        "unit_id": { "type": "string" },
                        "event_type": { "type": "string" },
                        "payload": { "type": "object" }
                    },
                    "required": ["repo_path", "mission_id", "session_id", "event_type"]
                }
            },
            {
                "name": "agent_report_touched_files",
                "description": "Replace the session touched file list and emit a touched-files event.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "mission_id": { "type": "string" },
                        "session_id": { "type": "string" },
                        "touched_paths": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["repo_path", "mission_id", "session_id", "touched_paths"]
                }
            },
            {
                "name": "agent_submit_verdict",
                "description": "Record a mission verification and mark the agent session complete or failed.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "mission_id": { "type": "string" },
                        "session_id": { "type": "string" },
                        "objective_id": { "type": "string" },
                        "status": { "type": "string", "default": "passed" },
                        "summary": { "type": "string" },
                        "details": { "type": "object" },
                        "artifacts": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "kind": { "type": "string" },
                                    "content": { "type": "string" }
                                }
                            }
                        }
                    },
                    "required": ["repo_path", "mission_id", "session_id"]
                }
            },
            {
                "name": "agent_logout",
                "description": "Close an active agent session and emit a logout event.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "mission_id": { "type": "string" },
                        "session_id": { "type": "string" },
                        "state": { "type": "string", "default": "completed" },
                        "reason": { "type": "string" }
                    },
                    "required": ["repo_path", "mission_id", "session_id"]
                }
            },
            {
                "name": "get_run_events",
                "description": "Retrieve run events for a task, optionally filtered by unit or session.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "task_id": { "type": "string" },
                        "unit_id": { "type": "string", "description": "Filter to a specific work unit." },
                        "session_id": { "type": "string", "description": "Filter to a specific runner session." },
                        "limit": { "type": "integer", "default": 50 }
                    },
                    "required": ["repo_path", "task_id"]
                }
            },
            {
                "name": "plan_task",
                "description": "Create a plan: store a task with an ordered list of work units. Idempotent — replanning replaces all existing units.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "task_id": { "type": "string", "description": "Stable identifier for this task (e.g. issue number or slug)." },
                        "title": { "type": "string" },
                        "prompt": { "type": "string", "description": "High-level task description." },
                        "units": {
                            "type": "array",
                            "description": "Ordered work units to create.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "title": { "type": "string" },
                                    "prompt": { "type": "string" }
                                },
                                "required": ["id", "title", "prompt"]
                            }
                        }
                    },
                    "required": ["repo_path", "task_id", "units"]
                }
            },
            {
                "name": "claim_work_unit",
                "description": "Atomically claim the next pending work unit. Returns null if all units are already claimed or done.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "task_id": { "type": "string" },
                        "agent_id": { "type": "string", "description": "Identifier for the agent claiming the unit." }
                    },
                    "required": ["repo_path", "task_id"]
                }
            },
            {
                "name": "submit_verdict",
                "description": "Submit the outcome for a claimed work unit. Advances its status to done or failed.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "task_id": { "type": "string" },
                        "unit_id": { "type": "string" },
                        "status": { "type": "string", "enum": ["done", "failed"], "default": "done" },
                        "summary": { "type": "string", "description": "Human-readable outcome." },
                        "artifacts": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "kind": { "type": "string" },
                                    "content": { "type": "string" }
                                }
                            }
                        }
                    },
                    "required": ["repo_path", "task_id", "unit_id"]
                }
            },
            {
                "name": "list_work_units",
                "description": "List all work units for a task with their current status.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string" },
                        "task_id": { "type": "string" }
                    },
                    "required": ["repo_path", "task_id"]
                }
            },
            {
                "name": "graph_clusters",
                "description": "Returns the stored cluster assignments for a repository. Each cluster is a connected component of the canonical graph, labelled by its dominant directory prefix and language.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_path": { "type": "string", "description": "Absolute path to the repository root." }
                    },
                    "required": ["repo_path"]
                }
            }
        ]
    })
}

fn dispatch_tool(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "watchdog_status" => tool_watchdog_status(args),
        "watchdog_run" => tool_watchdog_run(args),
        "graph_neighbors" => tool_graph_neighbors(args),
        "graph_blast_radius" => tool_graph_blast_radius(args),
        "graph_context" => tool_graph_context(args),
        "detect_changes" => tool_detect_changes(args),
        "query" => tool_query(args),
        "ingest_run_event" => tool_ingest_run_event(args),
        "agent_login" => tool_agent_login(args),
        "agent_request_context" => tool_agent_request_context(args),
        "agent_activity_set" => tool_agent_activity_set(args),
        "agent_report_event" => tool_agent_report_event(args),
        "agent_report_touched_files" => tool_agent_report_touched_files(args),
        "agent_submit_verdict" => tool_agent_submit_verdict(args),
        "agent_logout" => tool_agent_logout(args),
        "get_run_events" => tool_get_run_events(args),
        "plan_task" => tool_plan_task(args),
        "claim_work_unit" => tool_claim_work_unit(args),
        "submit_verdict" => tool_submit_verdict(args),
        "list_work_units" => tool_list_work_units(args),
        "graph_clusters" => tool_graph_clusters(args),
        _ => Err(format!("unknown tool: {name}")),
    }
}

// ── Method dispatch ───────────────────────────────────────────────────────────

fn handle_request(req: &JsonRpcRequest) -> JsonRpcResponse {
    let id = req.id.clone().unwrap_or(Value::Null);

    match req.method.as_str() {
        "initialize" => JsonRpcResponse::ok(
            id,
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "sloposcope-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        ),

        "tools/list" => JsonRpcResponse::ok(id, tools_list()),

        "tools/call" => {
            let name = req.params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = req.params.get("arguments").unwrap_or(&Value::Null);

            match dispatch_tool(name, args) {
                Ok(result) => JsonRpcResponse::ok(
                    id,
                    serde_json::json!({
                        "content": [{ "type": "text", "text": result.to_string() }],
                        "isError": false
                    }),
                ),
                Err(err) => JsonRpcResponse::ok(
                    id,
                    serde_json::json!({
                        "content": [{ "type": "text", "text": err }],
                        "isError": true
                    }),
                ),
            }
        }

        "notifications/initialized" => {
            // fire-and-forget notification, no response needed
            // Return a sentinel so caller can skip writing to stdout.
            JsonRpcResponse::ok(Value::Null, Value::Null)
        }

        _ => JsonRpcResponse::err(id, -32601, format!("method not found: {}", req.method)),
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(err) => {
                let resp = JsonRpcResponse::err(Value::Null, -32700, format!("parse error: {err}"));
                let _ = writeln!(out, "{}", serde_json::to_string(&resp).unwrap_or_default());
                let _ = out.flush();
                continue;
            }
        };

        // Skip writing a response for notifications (id is null AND method doesn't need reply).
        let is_notification = req.id.is_none() && req.method.starts_with("notifications/");
        if is_notification {
            continue;
        }

        let resp = handle_request(&req);
        let serialized = serde_json::to_string(&resp).unwrap_or_default();
        let _ = writeln!(out, "{serialized}");
        let _ = out.flush();
    }
}
