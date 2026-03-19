# Slopodog (Watchdog) Implementation Plan

Status: Done (current implementation scope)
Owner: sloposcope
Last updated: 2026-03-10

## Current TODO Board (Trackable)

In progress now:
- (none)

Next queue:
- (none)

`WD-039` breakdown:
- [x] `WD-039A` Define `Conductor` state machine and transitions (claimed/running/retry/reconciled).
- [x] `WD-039B` Add typed repo-owned workflow contract (`WORKFLOW.md` front matter + prompt template).
- [x] `WD-039C` Implement deterministic per-task workspace manager with safety invariants.
- [x] `WD-039D` Add runner protocol adapter for coding-agent app-server sessions.
- [x] `WD-039E` Add retry/backoff/reconciliation loop with bounded concurrency.
- [x] `WD-039F` Add status surface (`/api/v1/status` + `/api/v1/refresh`) and structured run traces.

Done recently:
- [x] `WD-001` Backend watchdog data model and runtime commands.
- [x] `WD-002` `AGENTS.md` ingestion and repo config loading.
- [x] `WD-003` Bounded snapshot + deterministic evaluator + run traces.
- [x] `WD-004` Frontend watchdog panel with issue context, manual check, verdict and traces.
- [x] `WD-031` Alert dedupe by evidence fingerprint + cooldown window.
- [x] `WD-033` Watchdog UI extracted into `WatchdogPanel.svelte`.
- [x] `WD-032` Real validation evidence capture via app-run validation command.
- [x] `WD-034` Add `watchdog_eval_cases` fixture corpus and component evaluator tests.
- [x] `WD-035` Replay/e2e eval target + spam-rate report.
- [x] `WD-036` Instruction-compliance checks against parsed `AGENTS.md` rules.
- [x] `WD-037` Trigger policies (`before_push`, `on_large_change_batch`, `on_branch_switch_after_quiet_period`).
- [x] `WD-038` MCP endpoints for watchdog status/check/alerts/issue/evidence.
- [x] `WD-039A` Conductor state machine scaffold + transition tests.
- [x] `WD-039B` Typed `WORKFLOW.md` contract loader (front matter + prompt template) with parser tests.
- [x] `WD-039C` Deterministic per-task workspace manager with path-safety invariants.
- [x] `WD-039D` Runner protocol adapter for coding-agent app-server sessions with payload/event/session-id tests.
- [x] `WD-039E` Reconciliation tick with bounded concurrency, retry backoff, and lifecycle tests.
- [x] `WD-039F` Conductor status/refresh surface with structured run traces and task enqueue command.
- [x] `WD-040` Workflow-driven watchdog execution wired to trigger policies (`provider=openai` with deterministic fallback).
- [x] `WD-041` Conductor UI panel with queue/status controls and trace view.
- [x] `WD-042` Strict-mode diagnostics for provider failures (`providerStrictMode=true` blocks fallback and surfaces blocked verdict evidence).
- [x] `WD-043` Conductor workflow settings surface (`provider`, `providerModel`, `timeoutSeconds`, `traceRetention`, strict mode) with repo-owned `WORKFLOW.md` persistence.
- [x] `WD-044` Eval/quality hardening (`scripts/run_watchdog_eval_suite.sh` + CI workflow + generated threshold report artifact).
- [x] `WD-045` Provider diagnostics polish (explicit provider failure counters in Conductor runtime/status and panel).
- [x] `WD-046` Expand replay fixture corpus for branch-switch noise and strict-mode provider-failure scenarios.

## Goal

Add Slopodog (the watchdog layer) to `sloposcope` so it observes ongoing work and alerts when the work appears to drift away from the active task, violates shared repo instructions, or lacks validation evidence.

The watchdog is not an authoring agent. It does not write code, propose large rewrites by default, or become another autonomous worker inside the app.

## Product intent

`sloposcope` remains a workspace/change-graph product.
The watchdog is a supervisory layer on top of the existing graph, VCS, and working-copy state.

Primary user value:
- detect drift from the active task
- detect likely violations of shared team instructions
- detect missing validation after behavioral changes
- provide evidence-backed alerts
- later expose the same watchdog through MCP so external agents can query it

## Shared sources of truth

The watchdog should only use shared team/project context, not personal hidden preferences.

Allowed inputs:
- `AGENTS.md` in the repo
- shared repo config committed in the repo, e.g. `.sloposcope/watchdog.json`
- active Linear issue selected by the user
- current repo/VCS state already known by `sloposcope`
- recent validation evidence from the app (build/test results, if captured)

Not allowed as policy source:
- per-user engineering preferences
- hidden local rule files outside the repo
- ad hoc path constraints invented by the watchdog

Allowed local-only config:
- API keys / provider credentials
- provider endpoint/base URL
- local UI preferences such as whether alerts are collapsed

## Non-goals

Not part of the initial watchdog feature:
- code generation
- code editing by the model
- autonomous planning/execution loops
- repo-wide embeddings/indexing
- full conversational agent inside `sloposcope`
- private user-specific rule systems

## Core user stories

1. As a user, I can tell `sloposcope` which Linear issue is active.
2. As a user, I can ask the watchdog whether the recent work is still aligned with the issue.
3. As a user, I get alerts when the watchdog sees likely drift or missing validation.
4. As a user, I can inspect why an alert fired and what evidence caused it.
5. As an external agent, I can later query the watchdog over MCP for current status and recent alerts.

## Inputs the watchdog evaluates

### 1. Objective
Derived from active Linear issue:
- issue title
- issue description
- acceptance criteria, if extractable
- explicit non-goals, if present
- labels/priority when useful

### 2. Shared instructions
Derived from `AGENTS.md` and optional shared repo config:
- workflow constraints
- editing constraints
- UI/style constraints
- validation expectations
- anti-overengineering guidance

### 3. Evidence
Derived from current app state:
- changed files in working copy
- diff summaries for changed files
- open windows / selected nodes, when relevant
- current branch/revision/since
- recent branch switches
- recent build/test runs, if present

## Output model

The watchdog emits structured verdicts and alerts.

### Guard verdict

```json
{
  "status": "on_track | drift | blocked | insufficient_context",
  "confidence": 0.0,
  "summary": "short human-readable verdict",
  "issues": [
    {
      "kind": "drift | instruction_violation | missing_validation | cost_risk | unclear_mapping",
      "severity": "info | warning | critical",
      "title": "short label",
      "why": "why the watchdog thinks this is happening",
      "evidence": ["file path or observation", "other observation"],
      "recommended_next_step": "one concrete next action"
    }
  ]
}
```

### Alert

```json
{
  "id": "uuid-or-stable-hash",
  "kind": "drift | instruction_violation | missing_validation | cost_risk | unclear_mapping",
  "severity": "info | warning | critical",
  "title": "short label",
  "message": "short message shown in UI",
  "evidence": ["..."],
  "created_at": "ISO-8601",
  "verdict_id": "..."
}
```

## Evaluation model

The watchdog should be staged, not monolithic.

### Stage 1: deterministic evidence collection
Build a normalized snapshot from app state and shared sources:
- current issue
- parsed repo instructions
- changed files
- diff summaries
- validation signals

### Stage 2: watchdog assessment
Run a focused evaluation over the normalized snapshot.
The implementation may be rule-only at first, then optionally add LLM-based judgment on top of the same snapshot.

Important constraint:
- all final alerts must be explainable with evidence visible in the app

## Execution principles

The implementation follows a strict build/eval loop:
- context engineering is a first-class deliverable, not prompt afterthought
- component-level eval and end-to-end eval are required before tuning alerts
- every watchdog run should produce a traceable record with inputs, verdict, and evidence
- guardrails are layered: deterministic checks first, semantic evaluator second
- autonomy remains constrained to watchdog-only actions; no code-authoring behavior

## Guardrail stack

Layer 1: deterministic guardrails
- missing active issue when required
- missing validation evidence after behavioral changes
- oversized change batch beyond configured bounds
- repeated alert dedupe and cooldown rules

Layer 2: semantic drift evaluator
- issue-to-change alignment check
- instruction compliance check against `AGENTS.md`
- cost/overengineering risk signal

Layer 3: user confirmation boundary
- watchdog surfaces alerts and evidence
- user decides whether to act; watchdog does not mutate code

## Evaluation protocol

Component eval:
- fixed input snapshots with expected verdict labels
- failure budget on false positive / false negative drift alerts

End-to-end eval:
- replay recent real traces from working-copy sessions
- verify alert quality and spam rate under normal workflow

Required artifacts:
- `watchdog_eval_cases` corpus (JSON snapshots + expected outcomes)
- trace log schema for watchdog runs
- regression report before changing thresholds or evaluator prompts

## Implementation phases

## Phase 0: groundwork and data model

Status: [x] Done

Deliverables:
- [x] shared watchdog data model in backend
- [x] canonical `WatchdogSnapshot`
- [x] canonical `WatchdogVerdict`
- [x] canonical `WatchdogAlert`
- [x] shared repo config file format defined

Proposed files:
- `src-tauri/src/watchdog.rs`
- `src-tauri/src/watchdog_config.rs`
- `src-tauri/src/watchdog_types.rs`

Tasks:
- [x] define Rust structs for issue/rules/evidence/verdict/alert
- [x] decide where active issue selection is stored in app state
- [x] decide minimal persisted UI state for watchdog panel
- [x] define `.sloposcope/watchdog.json` schema

Acceptance:
- [x] project compiles with empty/inactive watchdog state
- [x] types are serializable to frontend

## Phase 1: shared input ingestion

Status: [x] Done

Deliverables:
- [x] `AGENTS.md` reader
- [x] shared repo watchdog config reader
- [x] Linear issue selection + fetch path

Tasks:
- [x] read nearest repo `AGENTS.md`
- [x] include loaded repo watchdog config in evaluator snapshot
- [x] integrate active issue selection into app state
- [x] normalize issue fields into compact watchdog issue context

Acceptance:
- [x] watchdog can build an input context with no LLM involved
- [x] missing `AGENTS.md` or missing issue degrades gracefully

## Phase 2: evidence collection from current app state

Status: [x] Done

Deliverables:
- [x] working-copy evidence snapshot builder
- [x] diff summarization path for changed files
- [x] validation evidence record format
- [x] watchdog run trace schema
- [x] bounded snapshot contract version

Tasks:
- [x] collect changed files from current repo state
- [x] collect bounded diff summaries for changed files
- [x] capture branch/revision/since state
- [x] capture recent build/test results if available
- [x] store last-known validation result in app state
- [x] define a versioned snapshot payload for evaluator inputs
- [x] emit trace entries for every watchdog check

Acceptance:
- [x] snapshot stays bounded in size
- [x] snapshot is cheap to build for normal working-copy usage
- [x] each check is reproducible from trace payload

## Phase 3: first evaluation engine

Status: [x] Done

Deliverables:
- [x] initial watchdog evaluator
- [x] alert generation logic
- [x] manual "Check focus" command
- [x] deterministic guardrail rule set
- [x] semantic evaluator interface (LLM optional, pluggable)

Tasks:
- [x] implement first evaluator over snapshot
- [x] emit structured verdict
- [x] derive alerts from verdict issues
- [x] add backend command `check_watchdog_focus`
- [x] implement deterministic checks as first-pass filter
- [x] run semantic evaluator only when needed by policy

Initial checks for v1:
- [x] current changes map poorly to active issue summary
- [x] no validation evidence after behavior-affecting changes
- [x] work appears broader than requested issue scope
- [x] issue mapping is unclear because no active issue is set
- [x] likely instruction conflict with `AGENTS.md`

Acceptance:
- [x] manual check works end-to-end from UI
- [x] verdict explains itself with concrete evidence
- [x] deterministic-only mode works without model provider

## Phase 3.5: eval harness and calibration

Status: [x] Done

Deliverables:
- [x] component eval suite for watchdog verdicts
- [x] e2e replay eval for alert behavior
- [x] threshold tuning report

Tasks:
- [x] add fixture corpus for `watchdog_eval_cases`
- [x] add command/test target to run watchdog component evals
- [x] replay representative real traces and measure alert spam rate
- [x] tune thresholds/cooldowns using measured results

Acceptance:
- [x] baseline eval suite runs in CI
- [x] threshold changes require updated eval report

## Phase 4: UI integration

Status: [x] Done

Deliverables:
- [x] watchdog panel in frontend
- [x] active issue display
- [x] alert list
- [x] verdict inspector

Tasks:
- [x] add watchdog state to `src/App.svelte`
- [x] add small watchdog panel with current status
- [x] show severity-colored alerts
- [x] show evidence for each alert
- [x] add manual `Check focus` action

Proposed frontend files:
- `src/lib/WatchdogPanel.svelte`
- `src/lib/WatchdogAlertList.svelte`

Acceptance:
- [x] user can see whether watchdog is idle/on-track/warning/critical
- [x] user can inspect alert reasons without reading logs

## Phase 5: trigger policy and alerting behavior

Status: [x] Done

Deliverables:
- [x] configurable trigger policy
- [x] alert throttling / dedupe
- [x] major-change auto-check path

Tasks:
- [x] add triggers such as manual, before-push, on-large-change-batch
- [x] dedupe repeated alerts with same evidence hash
- [x] avoid checking on every noisy file event
- [x] track last check time and last verdict hash
- [x] gate auto-triggers behind measured alert quality from eval suite

Shared config examples:
- `manual_only`
- `before_push`
- `on_large_change_batch`
- `on_branch_switch_after_quiet_period`

Acceptance:
- [x] watchdog does not spam on every watcher event
- [x] repeated identical conditions do not flood the UI

## Phase 6: MCP exposure

Status: [x] Done

Deliverables:
- [x] MCP surface for external agents
- [x] queryable watchdog status and alerts

Proposed MCP operations:
- [x] `watchdog.get_status`
- [x] `watchdog.check_focus`
- [x] `watchdog.get_alerts`
- [x] `watchdog.get_issue_context`
- [x] `watchdog.get_evidence_snapshot`

Acceptance:
- [x] external agent can ask whether work is drifting
- [x] external agent can inspect recent alerts and evidence

## Minimal file layout proposal

Backend:
- `src-tauri/src/watchdog.rs`
- `src-tauri/src/watchdog_config.rs`
- `src-tauri/src/watchdog_types.rs`
- `src-tauri/src/linear_context.rs` or reuse existing Linear access path if added elsewhere

Frontend:
- `src/lib/WatchdogPanel.svelte`
- `src/lib/WatchdogAlertList.svelte`
- `src/lib/watchdog.js`
- integrate state/wiring in `src/App.svelte`

Repo config:
- `.sloposcope/watchdog.json`

## Suggested repo config schema

```json
{
  "enabled": true,
  "trigger_mode": "manual_only",
  "require_active_issue": true,
  "max_files_in_snapshot": 20,
  "max_diff_lines_per_file": 200,
  "dedupe_window_seconds": 300,
  "alert_threshold": "warning",
  "checks": {
    "issue_alignment": true,
    "instruction_compliance": true,
    "missing_validation": true,
    "cost_risk": true
  }
}
```

## Trackable milestone checklist

### Milestone A: inputs
- [x] active issue can be selected
- [x] `AGENTS.md` can be read
- [x] `.sloposcope/watchdog.json` can be read

### Milestone B: evidence
- [x] current changed files can be summarized
- [x] recent validation evidence is available
- [x] snapshot size is bounded

### Milestone C: evaluation
- [x] manual watchdog check works
- [x] structured verdict is returned to frontend
- [x] alerts are deduped

### Milestone D: UI
- [x] panel exists
- [x] alerts are visible
- [x] evidence is inspectable

### Milestone E: MCP
- [x] watchdog status is queryable
- [x] watchdog check can be triggered externally
- [x] recent alerts can be queried externally

## Risks

- active issue selection may be underspecified if no Linear context is chosen
- diff snapshots can become too large if not bounded aggressively
- noisy triggers can make the feature annoying if dedupe is weak
- verdict quality will be poor if evidence shown to the evaluator is sloppy or oversized
- MCP exposure must not bypass existing app security assumptions

## Open questions

- how should the active Linear issue be chosen in the UI?
- do we want manual-only as the default trigger mode?
- where should validation results come from first: app-run commands, external hooks, or user-marked checks?
- should watchdog verdicts be persisted across app restarts, or only alerts?

## First implementation slice

Recommended first slice:
- [x] repo config file support
- [x] `AGENTS.md` ingestion
- [x] active issue selection placeholder
- [x] bounded evidence snapshot builder
- [x] manual `Check focus`
- [x] basic alert panel

This slice is enough to validate the product direction before adding MCP or more automation.

## Symphony Mapping Notes (2026-03-09)

Reference:
- `openai/symphony` spec (`SPEC.md`, draft v1)

Directly reusable for sloposcope:
- strict orchestrator ownership of scheduling state
- bounded concurrency and per-state limits
- deterministic per-issue workspace paths + path safety invariants
- retry queue with exponential backoff and reconciliation on each poll tick
- typed repo-owned workflow contract (`WORKFLOW.md`)
- structured runtime events + operator-visible status endpoint

Needs adaptation for sloposcope:
- tracker integration may be broader than Linear
- we keep watchdog as supervisory layer, not autonomous code author by default
- UI remains sloposcope-first (graph + evidence windows), orchestration is backend capability

Non-goal for first spike:
- multi-tenant control plane or complex distributed scheduler
