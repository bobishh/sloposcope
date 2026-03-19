# Settings UI Plan

Status: planned
Last updated: 2026-03-19

## Problem

The current right panel has 4 tabs — Task / Watchdog / Conductor / Runs — with several issues:

1. **Watchdog config is not editable via UI.** Trigger mode, checks on/off, alert threshold,
   snapshot limits all live in `.sloposcope/watchdog.json` and can only be changed by
   hand-editing the file. The Watchdog tab shows read-only metadata and action buttons only.

2. **Conductor mixes config with operations.** "Workflow Settings" (saves to `WORKFLOW.md`)
   and "Queue" (enqueue / tick) share the same Conductor tab. These are different concerns.

3. **"Task" is a poor name.** The tab is the active issue context form — title, brief,
   validation command, acceptance criteria. It has nothing to do with generic tasks.

4. **Inconsistent persistence model.** Conductor settings → `WORKFLOW.md` (editable via UI),
   Watchdog settings → `watchdog.json` (no UI), Issue context → in-memory. No coherent story.

5. **Four tabs for what is conceptually two surfaces.** The user mostly needs:
   - "what am I working on right now" (issue + watchdog status)
   - "how is the automation configured" (watchdog config + conductor workflow)

---

## New Tab Structure

Three tabs replacing four:

```
[ Issue ]  [ Config ]  [ Runs ]
```

### Issue tab (replaces Task + inline Watchdog status)

The place to set up the current coding session. Contains:

- Issue context form (title, brief, validation command, acceptance criteria, non-goals,
  issue ID/URL/identifier/labels/priority) — same fields as current TaskPanel
- Watchdog status strip: last verdict badge + summary, confidence, issues list
- Watchdog action buttons: Run check, Build, Test, Reload
- Last validation hint (kind / ok|fail / ranAt)
- Graph evidence section (snapshot stats, changed files)
- Recent traces list

Rationale: the watchdog verdict is about the current issue. Showing it here keeps
"how is this task going" in one place. The Config tab holds the knobs you set once
and rarely touch again.

### Config tab (new — replaces Watchdog config-less panel + Conductor settings)

All durable settings. Two sections separated by a divider:

**Watchdog** section:
- Enabled toggle
- Trigger mode select: manual_only / before_push / on_large_change_batch /
  on_branch_switch_after_quiet_period
- Alert threshold select: info / warning / critical
- Require active issue checkbox
- Max files in snapshot (number, 1–100)
- Max diff lines per file (number, 10–1000)
- Dedupe window seconds (number, 0–3600)
- Checks group (checkboxes): issue_alignment, semantic_alignment,
  instruction_compliance, missing_validation, cost_risk
- Save button → writes `.sloposcope/watchdog.json`
- Load result hint (path, whether file was found / had parse error)

**Conductor** section:
- Provider select: coding_agent_app_server / openai
- Provider model text input
- Timeout sec (number, 10–3600)
- Trace retention (number, 1–200)
- Strict mode checkbox
- Save button → writes `WORKFLOW.md` front matter (existing logic)
- WORKFLOW.md load result hint

Rationale: both are "configure once, run many times" settings that live in files.
Grouping them together makes the persistence model obvious and keeps operations
(Issue, Runs) separate from configuration.

### Runs tab (replaces Conductor queue + RunsPanel)

Operational conductor surface. Contains:

- Status grid (total / running / retry / done / traces / last tick) — same as
  current Conductor meta-grid
- Issue runs list with Prepare launch / Dispatch buttons — from RunsPanel
- Queue section: Task ID, Max retries inputs + Enqueue button + Tick now button
- Raw event ingest section (from RunsPanel) — for manual runner event injection

Rationale: everything about "spinning up and tracking runs" lives here. The
workflow settings that drive those runs live in Config.

---

## Backend changes needed

### `save_watchdog_repo_config` Tauri command (new)

```rust
#[tauri::command]
fn save_watchdog_repo_config(
    state: tauri::State<AppState>,
    config: WatchdogRepoConfig,
) -> Result<(), String>
```

Implementation in `watchdog_config.rs`:

```rust
pub fn save_repo_config(repo: &Path, config: &WatchdogRepoConfig) -> Result<(), String> {
    let path = repo_config_path(repo);
    fs::create_dir_all(path.parent().unwrap())...;
    let json = serde_json::to_string_pretty(config)...;
    fs::write(&path, json)...;
    Ok(())
}
```

After save, re-read and return updated `WatchdogStateView` so the frontend
reflects the new config immediately (same pattern as `save_conductor_workflow_settings`).

No new types needed — `WatchdogRepoConfig` is already serializable.

---

## Frontend changes needed

### State additions in `App.svelte`

Watchdog config form fields (mirrors `WatchdogRepoConfig`):
```js
let watchdogConfigEnabled = $state(true);
let watchdogConfigTriggerMode = $state('manual_only');
let watchdogConfigAlertThreshold = $state('warning');
let watchdogConfigRequireActiveIssue = $state(true);
let watchdogConfigMaxFiles = $state('20');
let watchdogConfigMaxDiffLines = $state('200');
let watchdogConfigDedupeWindow = $state('300');
let watchdogConfigChecks = $state({
  issueAlignment: true,
  semanticAlignment: false,
  instructionCompliance: true,
  missingValidation: true,
  costRisk: true,
});
let watchdogConfigSaving = $state(false);
```

Populate from `watchdogState.config` when watchdog state loads (same pattern as
`applyConductorWorkflowLoad`).

### `saveWatchdogConfig()` function

```js
async function saveWatchdogConfig() {
  watchdogConfigSaving = true;
  try {
    await invokeTraced('save_watchdog_repo_config', {
      config: {
        enabled: watchdogConfigEnabled,
        triggerMode: watchdogConfigTriggerMode,
        alertThreshold: watchdogConfigAlertThreshold,
        requireActiveIssue: watchdogConfigRequireActiveIssue,
        maxFilesInSnapshot: Number(watchdogConfigMaxFiles),
        maxDiffLinesPerFile: Number(watchdogConfigMaxDiffLines),
        dedupeWindowSeconds: Number(watchdogConfigDedupeWindow),
        checks: watchdogConfigChecks,
      }
    });
    await refreshWatchdogState(activeProjectId, false);
  } finally {
    watchdogConfigSaving = false;
  }
}
```

### Component changes

- **Rename `TaskPanel.svelte` → `IssuePanel.svelte`** — add Watchdog status/traces
  sections pulled from current `WatchdogPanel.svelte`
- **Rename `WatchdogPanel.svelte` → `ConfigPanel.svelte`** — add Watchdog config
  form (new) + Conductor workflow form (moved from `ConductorPanel.svelte`)
- **Keep `RunsPanel.svelte`** as-is, add Conductor queue section (moved from
  `ConductorPanel.svelte`)
- **Delete `ConductorPanel.svelte`** — its contents split between ConfigPanel and RunsPanel

Tab buttons in `App.svelte`:
```svelte
<button ... onclick={() => activeSettingsTab = 'issue'}>Issue</button>
<button ... onclick={() => activeSettingsTab = 'config'}>Config</button>
<button ... onclick={() => activeSettingsTab = 'runs'}>Runs</button>
```

Default tab: `'issue'` (was `'task'`).

---

## File-by-file summary

| File | Change |
|------|--------|
| `src-tauri/src/watchdog_config.rs` | Add `save_repo_config()` |
| `src-tauri/src/lib.rs` | Add `save_watchdog_repo_config` command, register in invoke_handler |
| `src/App.svelte` | New watchdog config state + `saveWatchdogConfig()`, tab rename, rewire panels |
| `src/lib/TaskPanel.svelte` → `IssuePanel.svelte` | Absorb Watchdog status + traces |
| `src/lib/WatchdogPanel.svelte` → `ConfigPanel.svelte` | Add Watchdog config form, absorb Conductor workflow form |
| `src/lib/RunsPanel.svelte` | Absorb Conductor queue section |
| `src/lib/ConductorPanel.svelte` | Delete |

---

## What this does NOT change

- Persistence format: `watchdog.json` and `WORKFLOW.md` stay as-is
- Backend watchdog logic, conductor logic — untouched
- Watchdog action buttons (Run check, Build, Test) move to Issue tab but are
  otherwise unchanged
- All existing Tauri commands except the one new one
