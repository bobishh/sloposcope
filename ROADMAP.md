# Sloposcope Unified Roadmap

Status: active
Last updated: 2026-04-24 (NIM planner/watchdog provider landed)

## Product north star

Sloposcope is a control plane and observability layer for AI-assisted development.

The app is not just a code graph viewer and not just another coding agent. It should show:
- what code terrain exists,
- what mission is active,
- which agents/robots are doing work,
- which files/entities they touched,
- whether the work still matches the mission,
- and whether the result is safe to accept.

Visual direction: strategy-game / space-sim control room.

## Strategy glossary

Use these terms consistently:

- `World` = repository identity across time.
- `Map` = one concrete checkout/worktree of a repo.
- `Mission` = one active task/issue/user story on one map.
- `Objective` = a work unit inside a mission.
- `Unit` / `Robot` = one worker run executing one objective.
- `Sector` = graph cluster or code area.
- `Terrain` = canonical code graph for the current map.
- `Fog / activity` = current diff, touched files, changed entities.
- `Radar` = watchdog + graph evidence.
- `Command Center` = conductor/planner/reviewer runtime.

Hard rule:
- One active mission operates on one map.
- A parallel unrelated feature is a different mission, usually on a different worktree/map.
- Workers inside one mission can run in parallel only when their scopes do not conflict.

## Current execution status

Delivered and wired:
- [x] Canonical graph storage moved to SQLite in `.sloposcope/graph.sqlite3`
- [x] `GraphStore` boundary introduced without a global `StorageEngine`
- [x] `Atlas` / `Investigation` served from canonical facts through `get_graph_view(request)`
- [x] Watcher updates canonical file facts instead of mutating the old changed-files graph
- [x] Frontend tracks `graphViewMode`, `graphMeta`, provenance, approximate historical mode, and clustering metadata
- [x] Graph-backed watchdog with affected entities, related entities, and blast-radius evidence
- [x] `ConductorRuntimeState` state machine + Tauri commands for task lifecycle and runner ingest/dispatch
- [x] `WorkStore` SQLite backend for swarm work plans, claims, verdicts, and run events
- [x] `sloposcope-mcp` binary with graph / watchdog / work-unit tools
- [x] Settings page restructured into tabs: `Task`, `Watchdog`, `Conductor`, `Runs`
- [x] Persisted clustering path added with Julia sidecar + heuristic fallback
- [x] BM25 search index added with Tantivy + MCP/Tauri query surfaces

Important current limitation:
- Provider keys/models are worker-profile details, not product architecture.
- OpenAI and NVIDIA NIM are wired as HTTP planning/evaluator providers, not code-writing horses.
- Mission runtime now persists in `WorkStore`, and active-MCP session lifecycle is wired for workers.
- `ConductorRuntimeState` still exists for debug/legacy runner flow; mission path should keep shrinking around persisted store truth.

## Phase board

### M1: Canonical Graph Foundation
- [x] Remove abandoned Cozo spike
- [x] Add SQLite-backed `GraphStore`
- [x] Build full canonical graph for current checkout on repo open
- [x] Update file-scoped entities/edges from watcher events
- [x] Serve graph views from persisted canonical facts instead of the old in-memory changed-files graph

### M2: Atlas / Investigation
- [x] Add backend `get_graph_view(request)`
- [x] Add frontend `Investigation / Atlas` toggle
- [x] Add provenance labels for investigation nodes
- [x] Mark historical commit-focused investigation as approximate

### M3: Graph-backed Watchdog
- [x] Add graph query helpers over canonical facts
- [x] Extend watchdog snapshot with affected entities, related entities, and blast radius
- [x] Add structural drift checks based on graph neighborhoods
- [x] Expose graph-backed verdict evidence in watchdog UI/traces

### M4: Conductor Runtime Cleanup
- [x] Normalize `TaskContext`, `IssueRun`, `WorkUnit`, `WorkerRun`, `RunEvent`, `Artifact`, `Verdict`
- [x] Move runtime orchestration behind `RunnerAdapter`
- [x] Add event ingest surface for external runners

### M5: MCP + Swarm Bootstrap
- [x] Add separate `sloposcope-mcp` binary
- [x] Expose graph/task/watchdog/work-unit tools through MCP
- [x] Wire `conductor_prepare_launch`, `conductor_ingest_runner_event`, and `conductor_dispatch_launch`

### M6: Richer Graph
- [x] Parser `@<kind>.target` capture convention for named-kind edges
- [x] Class heritage edges: extends/implements for JS, TS, Python, Ruby, Java, C#, PHP, Kotlin, Rust
- [x] Confidence scores on edges by kind
- [x] Elixir/Ruby receiver-based call graph edges

### M7: Clustering, Search, and Settings Cleanup
- [x] Persist cluster assignments in `cluster_member`
- [x] Add Julia sidecar contract (`scripts/cluster.jl`, `scripts/Project.toml`, `julia_sidecar.rs`)
- [x] Fall back to heuristic clustering when Julia is unavailable
- [x] Expose clustering mode in `graphMeta` and prefer persisted assignments in the canvas layout
- [x] Add Tantivy search index in `.sloposcope/search-index`
- [x] Add Tauri `search_graph_entities` command
- [x] Add MCP `query` tool over canonical entities
- [x] Replace stacked settings page with tabbed layout

### M8: Mission / Swarm Runtime
- [x] Introduce explicit `Mission` model as active task + map/worktree + graph snapshot context
- [x] Persist mission/objective/session/event truth in `WorkStore`
- [x] Add `Mission / Runs / Debug` viewer-first settings surface
- [x] Add mission event ledger and persisted verification records in `Runs`
- [x] Add first dry-run worker flow: fixture planner creates objectives, fixture worker claims/reports/completes, watchdog can gate mission
- [x] Keep robot overlay sourced from mission/session state instead of only file-touch animation
- [x] Replace fixture-only planner path with provider-neutral `AgentProfile` selection
  - fixture remains fallback
  - `nim_http + can_plan` profile can plan mission objectives over hosted HTTP
- [x] Add active MCP session lifecycle (`agent_login`, heartbeat, activity, logout`) as primary agent truth path
- [ ] Collapse remaining duplicate runtime state in `ConductorRuntimeState` around persisted mission store
- [x] Add one scoped code-writing worker adapter in isolated workspace/worktree

### M9: Code-writing Worker
- [x] Add isolated workspace/worktree lifecycle for code-writing objectives
- [x] Add first real CLI worker profile (`local_cli`) with profile-backed subprocess dispatch
- [x] Wire session bundle + prompt handoff + active MCP launch config into CLI worker harness
- [x] Add provider-aware horse behavior for CLI profiles
  - chat-style profiles (`gemini_cli`, `codex_cli`, `claude_cli`) receive prompt over stdin
  - non-chat profiles keep bundle-first file/env handoff
- [x] Add compact profile env/token UX in `Debug`
  - provider-specific token hints
  - env JSON override editor
  - saved profile cards show env override keys + raw JSON on demand
- [x] Enforce objective scope before and after worker execution
- [x] Capture patch/diff artifact for each worker run
- [x] Capture touched files and validation artifacts through active MCP
  - touched files: reported through harness + active MCP
  - validation artifacts: submitted by harness verdict; backend merges patch/base metadata without rerun
- [x] Add active MCP context pull for workers
  - `agent_request_context` returns mission/objective facts, AGENTS rules, recent events, and scope-graph facts
  - harness writes runtime context snapshot into worker bundle before execution
- [x] Add HTTP planner/evaluator provider hook for hosted model APIs
  - workflow `provider=nim` uses NVIDIA NIM hosted chat endpoint
  - mission planner can use active `nim_http` profile with `can_plan`
- [x] Run objective-level validation command after worker completion
- [x] Add reviewer/watchdog gate before accepting the patch
- [x] Add accept/reject/apply strategy back into the active map
- [x] Handle conflicts and stale map revisions explicitly
- [x] Expose live session runtime state
  - `runtimeStatus`, `heartbeatAgeSec`, `eventAgeSec`, `leaseExpired` in mission view + MCP
- [x] Show code-writing worker state as mission robots with patch/review/gate status

### M10: Multi-worker Swarm / Troika
- [x] Add scheduler summary for multi-objective missions
  - multiline planner brief expands into multiple objectives
  - scheduler exposes `ready / blocked / active / stale / next objective`
  - `Run next code worker` uses scheduler-selected objective
- [x] Add scope conflict detection before spawning workers
  - conflicting later objective renders as blocked with scope wait reason
- [x] Add stale-session sweep control
  - `Sweep stale` requeues stale objectives
  - visible no-op state when nothing stale exists
- [ ] Add automatic serialization or worktree split for conflicting objectives
- [ ] Add reviewer profiles that inspect worker patches before watchdog gate
- [ ] Add escalation policy:
  - cheap worker failed -> stronger worker
  - repeated failure -> reviewer/planner intervention
  - scope drift -> block and ask user
- [ ] Add cost/time/token budget accounting per mission and per worker profile
- [ ] Add swarm-level event timeline and mission replay
- [ ] Add robot formation UI:
  - planner as command unit
  - workers as horses/robots
  - reviewer as outrider/inspector
  - watchdog as radar/advisor
- [ ] Add merge queue for accepted objective patches
- [ ] Add mission completion report with objectives, patches, validation, watchdog verdicts, and residual risks
- [ ] Add first real “feature from plan” run using at least two independent workers

## M8 user story 1: Plan a mission and watch worker robots report

As a user, I can give Sloposcope one feature/task on the current map, ask the conductor to plan it, and watch a small worker pool execute the plan as robots.

Acceptance criteria:
- User fills task title, brief, validation command, optional issue URL.
- User clicks `Plan mission`.
- A configured planner profile receives task + graph context + watchdog context.
- Planner returns deterministic JSON with objectives:
  - `id`
  - `title`
  - `prompt`
  - optional `scope.paths`
  - optional `expected_validation`
- Objectives are persisted in `WorkStore` / unified conductor store.
- `Runs` tab shows mission status and objective list.
- Fake worker pool can claim objectives and emit run events.
- Robot markers appear on the graph for claimed/running/done/failed worker runs.
- Watchdog can run after objective completion and mark the mission `on_track`, `drift`, `blocked`, or `insufficient_context`.
- No code is modified by this first slice.

Non-goals for M8 user story 1:
- No autonomous code writing yet.
- No automatic merging.
- No multi-mission scheduler.
- No semantic embeddings.
- No attempt to solve conflicting worker scopes automatically.

## M8 implementation plan

### Step 1: Runtime vocabulary and persistence
- Add `Mission` type:
  - `mission_id`
  - `repo_path`
  - `map_id` or `worktree_path`
  - `task_title`
  - `task_brief`
  - `validation_command`
  - `state`
- Decide source of truth:
  - preferred: `WorkStore` becomes persisted runtime backend for conductor work units/events
  - Tauri runtime becomes live cache/view over persisted store
- Add tests that create a mission, add objectives, reload store, and preserve state.

### Step 2: Planner adapter
- Add `AgentProfile`:
  - `profile_id`
  - `role` (`planner | worker | reviewer`)
  - `provider` (`openai | codex_cli | claude_cli | gemini_cli | local_cli | app_server`)
  - `model`
  - `token_env`
  - `command`
  - `mcp_servers`
  - `default_tools`
  - `budget`
- Add `AgentSession`:
  - `session_id`
  - `mission_id`
  - `objective_id`
  - `profile_id`
  - `workspace_path`
  - `state`
  - `started_at`
- Add `PlannerAdapter` trait separate from `RunnerAdapter`:
  - `plan_mission(context) -> Vec<WorkUnit>`
- Add provider-neutral subprocess/app-server launcher:
  - starts one agent process/session with token/model/profile config
  - preconfigures active MCP servers and CLI tools
  - nudges the agent with mission context
  - expects the agent to report through active MCP into the persisted event ledger
- OpenAI is only one predefined `AgentProfile`, not a special architecture path.
- Use structured JSON contract.
- Keep planner output small and bounded.
- Add deterministic parser tests with fixture model output.

### Step 3: MCP + Tauri commands
- Add commands/tools:
  - `plan_mission`
  - `get_mission`
  - `list_missions`
  - `claim_objective`
  - `report_objective_event`
  - `submit_objective_verdict`
- Keep names aligned between MCP and Tauri where possible.
- Do not use MCP as push event bus inside the app; use persisted event ledger as source of truth.

### Step 4: UI in Runs tab
- Add `Plan mission` action.
- Show mission summary.
- Show objective board:
  - pending
  - claimed
  - running
  - done
  - failed
- Show event ledger.
- Add optional `Export mission log` later; source of truth remains SQLite, markdown is generated/reporting only.

### Step 5: Robot overlay
- Add graph overlay input: `workerRuns` / `missionUnits`.
- Robot position priority:
  - explicit touched file from latest run event
  - objective scope path
  - cluster/sector from affected entities
  - fallback mission command center location
- Existing hammer robot remains file-touch indicator; mission robots are separate visual entities.

### Step 6: Dry-run worker pool
- Add fake/local worker adapter:
  - claims unit
  - emits `started`
  - emits `progress`
  - emits `completed`
- No code changes.
- This validates orchestration, UI, event ledger, and watchdog gate before real code-writing workers.

## M8 execution checklist

Build and ship the first runnable mission slice in this order:

1. Add persisted `Mission` record to the conductor/work store.
2. Add `MissionInput` parsing + validation.
3. Add `PlannerAdapter` and one fake/fixture planner implementation.
4. Save returned objectives as work units.
5. Add `AgentProfile` and `AgentSession` persistence.
6. Add active MCP session lifecycle:
   - login
   - activity
   - event report
   - touched files
   - verdict
   - logout
7. Add dry-run fake worker that consumes one objective and emits events.
8. Render mission state in `Runs`.
9. Render robot/session markers on the graph.
10. Run watchdog after objective completion and record verdict.

Definition of done for M8:
- one mission can be created from user input
- planner produces at least one objective
- one fake worker can execute it through active MCP events
- UI shows mission/objective/session state
- watchdog produces a gate verdict
- no code is changed

## Mission input contract

Minimum fields required to start a mission:
- `title`
- `brief`
- `validation_command`

Strongly recommended fields:
- `issue_key`
- `issue_url`
- `acceptance_criteria[]`
- `non_goals[]`
- `allowed_scope_paths[]`
- `forbidden_paths[]`
- `budget`
- `timebox_minutes`

Suggested JSON shape:

```json
{
  "title": "Stabilize settings UI and reduce operator noise",
  "brief": "Return the app to viewer-first behavior, hide enterprise debug controls, and keep watchdog/conductor internals available only in advanced mode.",
  "validation_command": "npm run -s build && cargo test --quiet --manifest-path src-tauri/Cargo.toml",
  "issue_key": "SLOPO-001",
  "issue_url": "https://example.invalid/issues/SLOPO-001",
  "acceptance_criteria": [
    "Graph remains the primary full-screen view",
    "Default settings surface contains only mission + run status",
    "Advanced controls remain reachable but hidden by default"
  ],
  "non_goals": [
    "Do not change graph rendering architecture",
    "Do not introduce autonomous code-writing workers in this slice"
  ],
  "allowed_scope_paths": [
    "src/",
    "src-tauri/src/",
    "ROADMAP.md"
  ],
  "forbidden_paths": [
    "vendor/",
    ".git/"
  ],
  "budget": {
    "max_worker_sessions": 3,
    "max_tokens_per_session": 120000
  },
  "timebox_minutes": 90
}
```

`MissionInput` should be normalized into:
- `MissionFacts`
- `BaseInstructionPack`
- `ObjectiveContract[]`
- `VerificationRecord[]`

## Later: code-writing swarm

M9 turns the M8 harness into actual code changes.

Worker prompt includes:
- mission context
- objective prompt
- allowed scope paths
- relevant graph context
- validation command
- active MCP login/reporting instructions

Worker requirements:
- run in isolated workspace/worktree
- report touched files
- report validation artifacts
- produce a patch/diff artifact
- never write directly into the active map unless explicitly running in single-map unsafe mode

Acceptance gate:
- reviewer verdict
- watchdog verdict
- validation command
- conflict/stale-map check

Parallel objectives:
- allowed only with non-overlapping scopes
- otherwise serialized or moved to separate worktrees/maps

## M10 swarm behavior

M10 is where the common harness becomes a real troika / swarm.

Conductor responsibilities:
- choose which objectives can run at the same time
- choose which agent profile gets each objective
- start/stop sessions
- watch cost/time/stuck sessions
- route failures to reviewer/planner/escalation
- maintain merge queue
- keep mission state understandable to the user

Scheduling rules:
- independent scopes can run concurrently
- overlapping file/symbol scopes require serialization or separate worktree maps
- high-risk objectives go to stronger profiles or require reviewer approval
- generated patches are never accepted only because a worker says it is done

UI rules:
- user sees formation/state, not raw process noise
- every robot has role, objective, scope, state, latest event, and artifacts
- mission can be replayed from the event ledger
- visual layer remains downstream of persisted mission/runtime state

## Agent harness model

The swarm should work like a common harness:
- Conductor owns the mission, objectives, scope, budget, and event ledger.
- Each worker is a horse in the harness: a separate process/session with a known profile, token/model, workspace, and active MCP/tool set.
- Agents can be OpenAI, Codex CLI, Claude CLI, Gemini CLI, a local model wrapper, or a custom app-server.
- The conductor does not care which vendor produced the run; it cares about normalized events, artifacts, touched files, validation, and verdict.

Required lifecycle:
1. `spawn_session(profile, mission, objective, workspace)`.
2. Agent logs into Sloposcope context (`mission_id`, `objective_id`, `agent_id`, `session_id`).
3. Agent receives MCP/tool config and initial nudge.
4. Agent reports `started/progress/touched_files/completed/failed`.
5. Conductor updates robots from the event ledger.
6. Watchdog/reviewer/tests gate acceptance.

## Active MCP harness

Use active MCP for agent coordination, not only passive graph queries.

Active MCP responsibilities:
- `agent_login(mission_id, objective_id, profile_id, session_id)` claims a live identity.
- `agent_activity_set(session_id, phase, label, detail)` drives robot state and UI timers.
- `agent_report_event(session_id, event_type, payload)` appends to the persisted event ledger.
- `agent_report_touched_files(session_id, paths)` updates robot placement and mission fog/activity.
- `agent_request_context(session_id, kind, target)` fetches graph/watchdog/task context on demand.
- `agent_submit_verdict(session_id, status, summary, artifacts)` closes the objective attempt.
- `agent_logout(session_id)` releases the live lease.

PTY / CLI responsibilities:
- Start the vendor CLI or local agent process.
- Inject token/model/profile env.
- Pass MCP server config.
- Send the initial nudge and follow-up nudges.
- Handle terminal-specific behavior:
  - prompt readiness detection
  - multiline input
  - interrupts
  - process exit
  - timeout / stuck-session handling
  - optional resize
- Treat stdout/stderr as diagnostics only. Authoritative state comes from active MCP events.

OpenAI/Codex-specific work belongs here:
- CLI invocation shape.
- PTY behavior.
- login/session nudging.
- model/token environment mapping.
- MCP config injection.
- how to recover when the CLI is waiting, blocked, or asking for input.

Ecky-like pattern to copy:
- explicit session login
- sticky agent/model identity
- structured activity updates
- final reply/event save
- no scraping terminal text as the source of truth

## Architecture rules

- One mission belongs to one map/worktree.
- Canonical graph is terrain for that map.
- `GraphStore` is the storage boundary for canonical graph persistence.
- `GraphViewService` owns `Atlas` / `Investigation` derivation.
- `SearchService` owns BM25 search index lifecycle and querying.
- `ClusterService` owns persisted clustering refresh and fallback behavior.
- `WorkStore` should become the persisted runtime backend for missions/objectives/events.
- `RunnerAdapter` launches workers.
- `PlannerAdapter` plans objectives.
- `AgentProfile` defines provider/model/token/command/MCP/tool setup.
- `AgentSession` is the runtime identity for one robot/worker process.
- Active MCP is the agent coordination surface.
- The persisted event ledger is the source of truth.
- PTY/stdout is never the source of truth.
- Markdown mission reports are generated artifacts, not source of truth.
- Historical full-revision graph persistence is out of scope for v1.
- Datalog-like reasoning is a future query-layer concern, not a storage dependency.

## Open work outside M8

Still open after current delivered slices:
- Global graph-search UI on the main canvas page; backend search exists, frontend still only has local visible-node filter
- Better clustering algorithms / quality upgrades beyond connected-component grouping in the first Julia sidecar
- `list_clusters`, richer `impact`, and raw query expansion on the MCP surface
- Optional embeddings / reranking / semantic search
- Visual simulation polish after runtime entities are real

## Resume point

If brain is cold, restart here:

1. The product model is `World -> Map -> Mission -> Objective -> Robot`.
2. Current code has graph/watchdog/search/clustering/MCP/work-unit foundations.
3. The next executable slice is M8 user story 1.
4. Do not start with autonomous code writing.
5. First build planner + persisted mission/objectives + fake worker robots + watchdog gate.
6. Only then add a scoped code-writing worker.
