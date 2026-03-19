<script>
  import { onMount, tick } from 'svelte';
  import { fade } from 'svelte/transition';
  import Sloposcope from './lib/Sloposcope.svelte';
  import MissionPanel from './lib/MissionPanel.svelte';
  import MissionRunsPanel from './lib/MissionRunsPanel.svelte';
  import AgentProfilesPanel from './lib/AgentProfilesPanel.svelte';
  import IssuePanel from './lib/IssuePanel.svelte';
  import ConfigPanel from './lib/ConfigPanel.svelte';
  import RunsPanel from './lib/RunsPanel.svelte';
  import { createBackendBridge } from './lib/backendBridge.js';
  import { getFortune } from './lib/fortunes.js';
  import * as projectSessions from './lib/projectSessionModel.js';
  import * as watchdogOps from './lib/watchdogConductorOrchestrator.js';
  const CHANGES_PAGE_SIZE = 20;
  const ALL_CHANGES_LIMIT = 50000;
  const DEBUG_LOGS = (() => {
    try {
      if (typeof window === 'undefined') return false;
      const qs = new URLSearchParams(window.location.search);
      if (qs.get('debug') === '1') return true;
      if (qs.get('debug') === '0') return false;
      const stored = localStorage.getItem('sloposcope-debug') ?? localStorage.getItem('eyeloss-debug');
      if (stored === '1') return true;
      if (stored === '0') return false;
      return import.meta.env.DEV;
    } catch (_e) {
      return import.meta.env.DEV;
    }
  })();

  function debugLog(...args) {
    if (DEBUG_LOGS) console.log(...args);
  }

  function normalizeTouchedPath(path) {
    if (!path) return '';
    return String(path)
      .replaceAll('\\', '/')
      .replace(/^\.\/+/, '')
      .replace(/^\/+/, '');
  }

  function seedHeatmapFromGraph(g, targetProjectId = activeProjectId) {
    const project = getProjectById(targetProjectId);
    if (!project) return;
    if (!g || !Array.isArray(g.nodes)) return;
    let changed = false;
    for (const node of g.nodes) {
      const status = String(node?.change_status || '');
      if (status !== 'added' && status !== 'modified') continue;
      const file = normalizeTouchedPath(node?.file || node?.id);
      if (!file) continue;
      if (!project.heatmapData.has(file)) {
        project.heatCounter = (project.heatCounter || 0) + 1;
        project.heatmapData.set(file, { seq: project.heatCounter, touchedAt: Date.now() });
        changed = true;
      }
    }
    if (changed && activeProjectId === targetProjectId) {
      heatCounter = project.heatCounter || 0;
      heatmapData = new Map(project.heatmapData);
    }
  }

  function traceNow() {
    try {
      return new Date().toISOString();
    } catch (_e) {
      return 'unknown-time';
    }
  }

  function summarizeResult(value) {
    if (Array.isArray(value)) return `array(len=${value.length})`;
    if (value && typeof value === 'object') {
      if (Array.isArray(value.nodes) && Array.isArray(value.edges)) {
        return `graph(nodes=${value.nodes.length}, edges=${value.edges.length})`;
      }
      const keys = Object.keys(value);
      return `object(keys=${keys.length})`;
    }
    return String(value);
  }

  async function invokeTraced(cmd, args, options = {}) {
    const warnAfterMs = Number(options.warnAfterMs ?? 5000);
    const started = performance.now();
    debugLog(`[FRONTEND][TRACE] ${traceNow()} -> ${cmd}`, args);
    const warnTimer = setTimeout(() => {
      const elapsed = Math.round(performance.now() - started);
      console.warn(`[FRONTEND][TRACE] ${traceNow()} still waiting for ${cmd} (${elapsed}ms elapsed)`, args);
    }, warnAfterMs);

    try {
      const result = await invoke(cmd, args);
      const elapsed = Math.round(performance.now() - started);
      debugLog(`[FRONTEND][TRACE] ${traceNow()} <- ${cmd} (${elapsed}ms) ${summarizeResult(result)}`);
      return result;
    } catch (error) {
      const elapsed = Math.round(performance.now() - started);
      console.error(`[FRONTEND][TRACE] ${traceNow()} !! ${cmd} failed after ${elapsed}ms`, error, args);
      throw error;
    } finally {
      clearTimeout(warnTimer);
    }
  }

  const MOCK_CHANGES = [
    { id: 'deadbeef', description: 'Working copy', timestamp: '2026-02-24 23:00' },
    { id: 'c0ffee11', description: 'Refactor parser pipeline', timestamp: '2026-02-24 22:30' },
    { id: 'b0ba0000', description: 'Fix timeline hover metadata', timestamp: '2026-02-24 22:00' },
    { id: 'fadedcab', description: 'Improve graph edge resolver', timestamp: '2026-02-24 21:30' },
    { id: 'decafbad', description: 'Add file watcher heat map', timestamp: '2026-02-24 21:00' },
    { id: 'bead1234', description: 'Tidy css variables', timestamp: '2026-02-24 20:30' },
    { id: '8badf00d', description: 'Add bookmarks API', timestamp: '2026-02-24 20:00' },
    { id: 'facefeed', description: 'Stabilize canvas zoom', timestamp: '2026-02-24 19:30' },
    { id: 'ab12cd34', description: 'Introduce code window', timestamp: '2026-02-24 19:00' },
    { id: '3412dcba', description: 'Bootstrap initial graph renderer', timestamp: '2026-02-24 18:30' },
  ];

  function createDefaultWatchdogState() {
    return {
      config: {
        enabled: true,
        triggerMode: 'manual_only',
        requireActiveIssue: true,
        maxFilesInSnapshot: 20,
        maxDiffLinesPerFile: 200,
        dedupeWindowSeconds: 300,
        alertThreshold: 'warning',
        checks: {
          issueAlignment: true,
          instructionCompliance: true,
          missingValidation: true,
          costRisk: true,
        },
      },
      configLoad: {
        path: '',
        exists: false,
        loaded: false,
        error: null,
      },
      agents: {
        load: {
          path: null,
          exists: false,
          loaded: false,
          error: null,
        },
        rules: [],
      },
      agentsPath: null,
      activeIssue: null,
      activeIssueContext: null,
      lastSnapshot: null,
      lastValidation: null,
      lastTraceId: null,
      lastVerdict: null,
      alerts: [],
    };
  }

  function createDefaultConductorStatus(traceLimit = 20) {
    return {
      generatedAt: new Date().toISOString(),
      config: {
        maxConcurrentRuns: 2,
        retryBaseSeconds: 15,
        retryMaxSeconds: 300,
      },
      providerFailures: {
        total: 0,
        openai: 0,
        codingAgentAppServer: 0,
        strictModeBlocked: 0,
        fallbackUsed: 0,
        lastFailureAt: null,
        lastFailureProvider: null,
        lastFailureMessage: null,
      },
      tasksTotal: 0,
      tasksClaimed: 0,
      tasksRunning: 0,
      tasksRetryScheduled: 0,
      tasksReconciled: 0,
      tracesTotal: 0,
      lastTickAt: null,
      recentTraces: [],
      issueRuns: [],
      _traceLimit: traceLimit,
    };
  }

  function createDefaultConductorWorkflowLoad() {
    return {
      path: '',
      exists: false,
      loaded: false,
      error: null,
      contract: null,
    };
  }

  function createDefaultMissionState() {
    return {
      missionId: '',
      title: '',
      brief: '',
      validationCommand: '',
      issueKey: '',
      issueUrl: '',
      plannerProfileId: '',
      plannerProfileLabel: '',
      plannerNotes: '',
      plannerPromptOverride: '',
      plannerSummary: 'viewer-first planner ready',
      plannerStatus: 'idle',
      plannerMode: 'viewer_first',
      summary: '',
      status: 'idle',
      viewerProfileId: '',
      viewerSessionId: '',
      reviewState: 'pending',
      applyState: 'not_applied',
      gateState: 'open',
      profiles: [],
      lastAction: '',
      lastActionAt: null,
      objectives: [],
      sessions: [],
      verifications: [],
      eventLedger: [],
      robotTargets: [],
      scheduler: {
        readyCount: 0,
        blockedCount: 0,
        activeCount: 0,
        staleSessionCount: 0,
        nextObjectiveId: null,
      },
      lastSavedAt: null,
      lastPlannedAt: null,
      lastCheckedAt: null,
      lastRunAt: null,
      lastCheckResult: null,
    };
  }

  function latestMissionSession(state) {
    const sessions = Array.isArray(state?.sessions) ? state.sessions : [];
    return sessions.length > 0 ? sessions[sessions.length - 1] : null;
  }

  function uniqueChanges(list) {
    return projectSessions.uniqueChanges(list);
  }
  const { invoke, listen } = createBackendBridge({
    debugLog,
    mockChanges: MOCK_CHANGES,
    createDefaultWatchdogState,
    createDefaultConductorStatus,
    createDefaultConductorWorkflowLoad,
  });

  let graph = $state({ nodes: [], edges: [] });
  let graphViewMode = $state('investigation');
  let graphMeta = $state(projectSessions.createDefaultGraphMeta('investigation'));
  let changes = $state([]);
  let changesLimit = $state(CHANGES_PAGE_SIZE);
  let hasMoreChanges = $state(true);
  let bookmarks = $state([]);
  let currentBranch = $state('');
  let currentRevision = $state('');
  let since = $state('@'); 
  let loading = $state(true); // Initial load
  let refreshing = $state(false); // Update state
  let graphBuildProgress = $state(null); // { filesProcessed, filesTotal } while bg rebuild runs
  let currentFortune = $state('');
  let repoPath = $state('');
  let repoActionMessage = $state('');
  let showDropdown = $state(false);
  let highlightedIndex = $state(-1);
  let loadGraphRequestSeq = 0;
  let inFlightLoadGraph = 0;
  let loadingMore = $state(false);

  // Heatmap state
  let heatmapData = $state(new Map());
  let heatCounter = 0;
  let fileContentVersions = $state(new Map());
  let contentVersion = $derived.by(() => {
    const repo = repoPath || '';
    const revision = currentRevision || '';
    const sinceValue = since || '';
    return `${repo}|${revision}|${sinceValue}`;
  });

  // Multi-project tabs (one active project; inactive projects sleep).
  let projectTabs = $state([]);
  let activeProjectId = $state(null);

  let watchdogState = $state(createDefaultWatchdogState());
  let watchdogTraces = $state([]);
  let watchdogLoading = $state(false);
  let watchdogRunning = $state(false);
  let watchdogError = $state('');
  let watchdogLoadRequestSeq = 0;

  let watchdogIssueId = $state('');
  let watchdogIssueTitle = $state('');
  let watchdogIssueIdentifier = $state('');
  let watchdogIssueUrl = $state('');
  let watchdogIssueSummary = $state('');
  let watchdogIssueLabels = $state('');
  let watchdogIssuePriority = $state('');
  let watchdogAcceptanceCriteria = $state('');
  let watchdogNonGoals = $state('');
  let watchdogValidationDetails = $state('');
  let conductorStatus = $state(createDefaultConductorStatus(20));
  let conductorLoading = $state(false);
  let conductorWorkflowLoading = $state(false);
  let conductorRunning = $state(false);
  let conductorSettingsSaving = $state(false);
  let conductorError = $state('');
  let conductorTaskId = $state('');
  let conductorMaxRetries = $state('2');
  let conductorTraceLimit = $state('20');
  let conductorWorkflowLoad = $state(createDefaultConductorWorkflowLoad());
  let conductorWorkflowProvider = $state('coding_agent_app_server');
  let conductorWorkflowProviderModel = $state('');
  let conductorWorkflowTimeout = $state('900');
  let conductorWorkflowTraceRetention = $state('30');
  let conductorWorkflowStrictMode = $state(false);
  let conductorLaunchResult = $state(null);
  let conductorLaunchLoading = $state(false);
  let conductorDispatchLoading = $state(false);
  let conductorIngestTaskId = $state('');
  let conductorIngestRawEvent = $state('');
  let conductorIngestLoading = $state(false);
  let clusterRefreshLoading = $state(false);
  let missionStateByProject = $state(new Map());
  let selectedCodeWorkerProfileByProject = $state(new Map());
  let missionState = $state(createDefaultMissionState());
  let missionLoading = $state(false);
  let missionSaving = $state(false);
  let missionPlanning = $state(false);
  let missionCheckRunning = $state(false);
  let missionFixtureRunning = $state(false);
  let missionCodeWorkerRunning = $state(false);
  let missionReviewRunning = $state(false);
  let missionReviewAction = $state('');
  let missionError = $state('');
  let missionLoadRequestSeq = 0;
  let missionTitle = $state('');
  let missionBrief = $state('');
  let missionValidationCommand = $state('');
  let missionIssueKey = $state('');
  let missionIssueUrl = $state('');
  let missionPlannerProfileId = $state('');
  let missionPlannerNotes = $state('');
  let missionPlannerPromptOverride = $state('');
  let selectedCodeWorkerProfileId = $state('');
  let missionProfileSaving = $state(false);
  let missionProfileDeletingId = $state('');
  let missionProfileError = $state('');
  let missionProfileLabel = $state('');
  let missionProfileProvider = $state('');
  let missionProfileModel = $state('');
  let missionProfileCommand = $state('');
  let missionProfileArgs = $state('');
  let missionProfileEnvJson = $state('{}');
  let missionProfileCapabilities = $state('');
  let missionProfileTransportKind = $state('pty');
  let missionProfileActive = $state(false);

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

  let activeMainView = $state('graph');
  let activeSettingsTab = $state('mission');
  let activeDebugTab = $state('issue');

  let watchdogStatusClass = $derived.by(() => {
    const status = String(watchdogState?.lastVerdict?.status || '');
    if (status === 'on_track') return 'eyeloss-watchdog__status--ok';
    if (status === 'drift') return 'eyeloss-watchdog__status--warn';
    if (status === 'blocked') return 'eyeloss-watchdog__status--blocked';
    return 'eyeloss-watchdog__status--unknown';
  });

  let missionProfileProviderHint = $derived.by(() => (
    watchdogOps.getAgentProfileProviderHint(missionProfileProvider)
  ));

  let missionProfileEnvError = $derived.by(() => (
    watchdogOps.validateJsonObjectInput(missionProfileEnvJson, 'Env JSON')
  ));

  function shortRepoLabel(path) {
    return projectSessions.shortRepoLabel(path);
  }

  function extractErrorMessage(error) {
    return watchdogOps.extractErrorMessage(error);
  }

  function toOptionalString(value) {
    return watchdogOps.toOptionalString(value);
  }

  function deriveTaskIdFromForm() {
    const explicit = String(watchdogIssueId || '').trim();
    if (explicit) return explicit;
    const identifier = String(watchdogIssueIdentifier || '').trim();
    if (identifier) {
      return identifier.toLowerCase().replaceAll(/[^a-z0-9]+/g, '-').replaceAll(/^-+|-+$/g, '');
    }
    const title = String(watchdogIssueTitle || '').trim().toLowerCase();
    const slug = title.replaceAll(/[^a-z0-9]+/g, '-').replaceAll(/^-+|-+$/g, '');
    return slug || '';
  }

  function applyWatchdogConfig(config) {
    if (!config) return;
    watchdogConfigEnabled = config.enabled ?? true;
    watchdogConfigTriggerMode = config.triggerMode ?? 'manual_only';
    watchdogConfigAlertThreshold = config.alertThreshold ?? 'warning';
    watchdogConfigRequireActiveIssue = config.requireActiveIssue ?? true;
    watchdogConfigMaxFiles = String(config.maxFilesInSnapshot ?? 20);
    watchdogConfigMaxDiffLines = String(config.maxDiffLinesPerFile ?? 200);
    watchdogConfigDedupeWindow = String(config.dedupeWindowSeconds ?? 300);
    watchdogConfigChecks = {
      issueAlignment: config.checks?.issueAlignment ?? true,
      semanticAlignment: config.checks?.semanticAlignment ?? false,
      instructionCompliance: config.checks?.instructionCompliance ?? true,
      missingValidation: config.checks?.missingValidation ?? true,
      costRisk: config.checks?.costRisk ?? true,
    };
  }

  function applyWatchdogView(view) {
    const derived = watchdogOps.deriveWatchdogFormValues(view, createDefaultWatchdogState);
    watchdogState = derived.state;
    watchdogIssueId = derived.form.issueId;
    watchdogIssueTitle = derived.form.issueTitle;
    watchdogIssueIdentifier = derived.form.issueIdentifier;
    watchdogIssueUrl = derived.form.issueUrl;
    watchdogIssueSummary = derived.form.issueSummary;
    watchdogIssueLabels = derived.form.issueLabels;
    watchdogIssuePriority = derived.form.issuePriority;
    watchdogAcceptanceCriteria = derived.form.acceptanceCriteria;
    watchdogNonGoals = derived.form.nonGoals;
    applyWatchdogConfig(derived.state?.config);
  }

  function formatWatchdogStatus(status) {
    return watchdogOps.formatWatchdogStatus(status);
  }

  function getMissionStateForProject(projectId) {
    return missionStateByProject.get(projectId) || createDefaultMissionState();
  }

  function getSelectedCodeWorkerProfileId(projectId = activeProjectId) {
    return selectedCodeWorkerProfileByProject.get(projectId) || '';
  }

  function setSelectedCodeWorkerProfileId(profileId, projectId = activeProjectId) {
    const normalized = String(profileId || '').trim();
    const next = new Map(selectedCodeWorkerProfileByProject);
    if (normalized) next.set(projectId, normalized);
    else next.delete(projectId);
    selectedCodeWorkerProfileByProject = next;
    if (projectId === activeProjectId) {
      selectedCodeWorkerProfileId = normalized;
    }
  }

  $effect(() => {
    if (!activeProjectId) return;
    const current = selectedCodeWorkerProfileByProject.get(activeProjectId) || '';
    if (current === selectedCodeWorkerProfileId) return;
    const next = new Map(selectedCodeWorkerProfileByProject);
    if (selectedCodeWorkerProfileId) next.set(activeProjectId, selectedCodeWorkerProfileId);
    else next.delete(activeProjectId);
    selectedCodeWorkerProfileByProject = next;
  });

  function storeMissionState(projectId, nextState) {
    const nextMap = new Map(missionStateByProject);
    nextMap.set(projectId, nextState);
    missionStateByProject = nextMap;
    if (projectId === activeProjectId) {
      missionState = nextState;
      missionTitle = nextState.title || '';
      missionBrief = nextState.brief || '';
      missionValidationCommand = nextState.validationCommand || '';
      missionIssueKey = nextState.issueKey || '';
      missionIssueUrl = nextState.issueUrl || '';
      missionPlannerProfileId = nextState.plannerProfileId || '';
      missionPlannerNotes = nextState.plannerNotes || '';
      missionPlannerPromptOverride = nextState.plannerPromptOverride || '';
    }
  }

  function syncMissionStateToProject(projectId = activeProjectId) {
    if (!projectId) return;
    storeMissionState(projectId, normalizeMissionView(missionState, missionState));
  }

  function missionInputFromForm() {
    return watchdogOps.buildMissionInputFromForm({
      title: missionTitle,
      brief: missionBrief,
      validationCommand: missionValidationCommand,
      issueKey: missionIssueKey,
      issueUrl: missionIssueUrl,
      plannerProfileId: missionPlannerProfileId,
      plannerNotes: missionPlannerNotes,
      plannerPromptOverride: missionPlannerPromptOverride,
    });
  }

  function missionSummaryFromInput(input) {
    const title = String(input?.title || '').trim();
    const brief = String(input?.brief || '').trim();
    if (title && brief) return `${title} - ${brief}`;
    return title || brief || 'Mission not planned yet.';
  }

  function appendMissionEvent(state, event) {
    const nextLedger = [...(state?.eventLedger || []), { at: traceNow(), ...event }].slice(-200);
    return {
      ...state,
      eventLedger: nextLedger,
    };
  }

  function normalizeMissionView(view, fallback = null) {
    const base = fallback || createDefaultMissionState();
    if (!view || typeof view !== 'object') return base;
    const latestSession = latestMissionSession(view) || latestMissionSession(base);
    return {
      ...base,
      ...view,
      title: String(view.title ?? base.title ?? ''),
      brief: String(view.brief ?? base.brief ?? ''),
      validationCommand: String(view.validationCommand ?? view.validation_command ?? base.validationCommand ?? ''),
      issueKey: String(view.issueKey ?? view.issue_key ?? base.issueKey ?? ''),
      issueUrl: String(view.issueUrl ?? view.issue_url ?? base.issueUrl ?? ''),
      plannerProfileId: String(view.plannerProfileId ?? view.planner_profile_id ?? base.plannerProfileId ?? ''),
      plannerProfileLabel: String(view.plannerProfileLabel ?? view.planner_profile_label ?? base.plannerProfileLabel ?? ''),
      plannerNotes: String(view.plannerNotes ?? view.planner_notes ?? base.plannerNotes ?? ''),
      plannerPromptOverride: String(view.plannerPromptOverride ?? view.planner_prompt_override ?? base.plannerPromptOverride ?? ''),
      plannerSummary: String(view.plannerSummary ?? view.planner_summary ?? base.plannerSummary ?? 'viewer-first planner ready'),
      plannerStatus: String(view.plannerStatus ?? view.planner_status ?? base.plannerStatus ?? 'idle'),
      plannerMode: String(view.plannerMode ?? view.planner_mode ?? base.plannerMode ?? 'viewer_first'),
      summary: String(view.summary ?? base.summary ?? ''),
      status: String(view.status ?? base.status ?? 'idle'),
      viewerProfileId: String(view.viewerProfileId ?? view.viewer_profile_id ?? latestSession?.profileId ?? base.viewerProfileId ?? ''),
      viewerSessionId: String(view.viewerSessionId ?? view.viewer_session_id ?? latestSession?.sessionId ?? base.viewerSessionId ?? ''),
      reviewState: String(view.reviewState ?? view.review_state ?? latestSession?.reviewState ?? base.reviewState ?? 'pending'),
      applyState: String(view.applyState ?? view.apply_state ?? latestSession?.applyState ?? base.applyState ?? 'not_applied'),
      gateState: String(view.gateState ?? view.gate_state ?? latestSession?.gateState ?? base.gateState ?? 'open'),
      profiles: Array.isArray(view.profiles) ? view.profiles : base.profiles,
      lastAction: String(view.lastAction ?? view.last_action ?? base.lastAction ?? ''),
      lastActionAt: view.lastActionAt ?? view.last_action_at ?? base.lastActionAt,
      objectives: Array.isArray(view.objectives) ? view.objectives : base.objectives,
      sessions: Array.isArray(view.sessions) ? view.sessions : base.sessions,
      verifications:
        Array.isArray(view.verifications)
          ? view.verifications
          : Array.isArray(view.verificationRecords)
            ? view.verificationRecords
            : base.verifications,
      eventLedger: Array.isArray(view.eventLedger) ? view.eventLedger : Array.isArray(view.event_ledger) ? view.event_ledger : base.eventLedger,
      robotTargets: Array.isArray(view.robotTargets) ? view.robotTargets : Array.isArray(view.robot_targets) ? view.robot_targets : base.robotTargets,
      scheduler:
        view.scheduler && typeof view.scheduler === 'object'
          ? {
              readyCount: Number(view.scheduler.readyCount ?? view.scheduler.ready_count ?? base.scheduler?.readyCount ?? 0),
              blockedCount: Number(view.scheduler.blockedCount ?? view.scheduler.blocked_count ?? base.scheduler?.blockedCount ?? 0),
              activeCount: Number(view.scheduler.activeCount ?? view.scheduler.active_count ?? base.scheduler?.activeCount ?? 0),
              staleSessionCount: Number(view.scheduler.staleSessionCount ?? view.scheduler.stale_session_count ?? base.scheduler?.staleSessionCount ?? 0),
              nextObjectiveId: view.scheduler.nextObjectiveId ?? view.scheduler.next_objective_id ?? base.scheduler?.nextObjectiveId ?? null,
            }
          : base.scheduler,
      lastSavedAt: view.lastSavedAt ?? view.last_saved_at ?? base.lastSavedAt,
      lastPlannedAt: view.lastPlannedAt ?? view.last_planned_at ?? base.lastPlannedAt,
      lastCheckedAt: view.lastCheckedAt ?? view.last_checked_at ?? base.lastCheckedAt,
      lastRunAt: view.lastRunAt ?? view.last_run_at ?? base.lastRunAt,
      lastCheckResult: view.lastCheckResult ?? view.last_check_result ?? base.lastCheckResult,
    };
  }

  function resolveMissionReviewSession(state, sessionId = '') {
    const sessions = Array.isArray(state?.sessions) ? state.sessions : [];
    if (sessionId) return sessions.find((entry) => entry.sessionId === sessionId) || null;
    return latestMissionSession(state);
  }

  function mutateMissionReviewState(current, sessionId, action, now = traceNow()) {
    const target = resolveMissionReviewSession(current, sessionId);
    if (!target) return null;

    const nextSessions = (current.sessions || []).map((entry) => {
      if (entry.sessionId !== target.sessionId) return entry;
      if (action === 'accept') {
        return {
          ...entry,
          reviewState: 'accepted',
          reviewedAt: now,
          reviewerId: 'viewer',
          gateState: 'ready',
          lastAction: 'accept',
          lastActionAt: now,
        };
      }
      if (action === 'reject') {
        return {
          ...entry,
          reviewState: 'rejected',
          reviewedAt: now,
          reviewerId: 'viewer',
          applyState: 'blocked',
          gateState: 'blocked',
          lastAction: 'reject',
          lastActionAt: now,
        };
      }
      if (action === 'apply') {
        return {
          ...entry,
          applyState: 'applied',
          appliedAt: now,
          lastAction: 'apply',
          lastActionAt: now,
        };
      }
      return {
        ...entry,
        gateState: 'ready',
        gatedAt: now,
        lastAction: 'gate',
        lastActionAt: now,
      };
    });

    const updatedSession = nextSessions.find((entry) => entry.sessionId === target.sessionId) || target;
    const reviewState = String(updatedSession.reviewState || current.reviewState || 'pending');
    const applyState = String(updatedSession.applyState || current.applyState || 'not_applied');
    const gateState = String(updatedSession.gateState || current.gateState || 'open');
    const status =
      action === 'reject'
        ? 'rejected'
        : action === 'apply'
          ? 'applied'
          : action === 'gate'
            ? 'accepted'
            : 'accepted';

    return normalizeMissionView(
      {
        ...current,
        status,
        viewerProfileId: updatedSession.profileId || current.viewerProfileId || '',
        viewerSessionId: updatedSession.sessionId || current.viewerSessionId || target.sessionId,
        reviewState,
        applyState,
        gateState,
        lastAction: action,
        lastActionAt: now,
        sessions: nextSessions,
        mission: {
          ...(current.mission || {}),
          status,
          viewerProfileId: updatedSession.profileId || current.viewerProfileId || '',
          viewerSessionId: updatedSession.sessionId || current.viewerSessionId || target.sessionId,
          reviewState,
          applyState,
          gateState,
          lastAction: action,
          lastActionAt: now,
        },
      },
      current,
    );
  }

  async function refreshMissionState(targetProjectId = activeProjectId) {
    if (!targetProjectId || targetProjectId !== activeProjectId) return;
    const requestId = ++missionLoadRequestSeq;
    missionLoading = true;
    missionError = '';
    try {
      const [view, profileView] = await Promise.all([
        watchdogOps.fetchMissionState(invokeTraced),
        watchdogOps.fetchAgentProfiles(invokeTraced),
      ]);
      if (requestId !== missionLoadRequestSeq || targetProjectId !== activeProjectId) return;
      const next = normalizeMissionView(
        { ...view, profiles: Array.isArray(profileView?.profiles) ? profileView.profiles : view?.profiles || [] },
        getMissionStateForProject(targetProjectId),
      );
      storeMissionState(targetProjectId, next);
      const selectedProfileId = getSelectedCodeWorkerProfileId(targetProjectId);
      if (selectedProfileId && !next.profiles.some((entry) => entry?.id === selectedProfileId)) {
        setSelectedCodeWorkerProfileId('', targetProjectId);
      }
    } catch (error) {
      if (requestId === missionLoadRequestSeq && targetProjectId === activeProjectId) {
        missionError = extractErrorMessage(error) || 'Failed to load mission state.';
      }
    } finally {
      if (requestId === missionLoadRequestSeq && targetProjectId === activeProjectId) {
        missionLoading = false;
      }
    }
  }

  async function saveMissionInput() {
    if (missionSaving) return;
    missionSaving = true;
    missionError = '';
    const input = missionInputFromForm();
    const now = traceNow();
    const current = getMissionStateForProject(activeProjectId);
    const plannerProfile = (current.profiles || []).find((entry) => entry?.id === input.plannerProfileId) || null;
    const plannerMode = input.plannerProfileId ? 'profile' : 'viewer_first';
    const plannerSummary = input.plannerProfileId
      ? `Planner profile ready: ${plannerProfile?.label || input.plannerProfileId}`
      : 'viewer-first planner ready';
    const nextState = normalizeMissionView(
      {
        ...current,
        ...input,
        plannerProfileLabel: plannerProfile?.label || '',
        summary: current.summary || missionSummaryFromInput(input),
        status: current.status === 'planned' ? 'planned' : 'ready',
        plannerSummary,
        plannerStatus: current.plannerStatus === 'planned' ? 'planned' : 'ready',
        plannerMode,
        lastSavedAt: now,
      },
      current,
    );
    try {
      const view = await invokeTraced('save_mission_input', { missionInput: input });
      const next = normalizeMissionView(view, nextState);
      storeMissionState(activeProjectId, next);
    } catch (error) {
      missionError = extractErrorMessage(error) || 'Failed to save mission input.';
      storeMissionState(activeProjectId, appendMissionEvent(nextState, {
        kind: 'save',
        summary: 'Mission input saved locally.',
      }));
    } finally {
      missionSaving = false;
    }
  }

  async function planMission() {
    if (missionPlanning) return;
    missionPlanning = true;
    missionError = '';
    const input = missionInputFromForm();
    if (!input.plannerProfileId && (input.plannerNotes || input.plannerPromptOverride)) {
      missionError = 'Select planner profile for planner notes or override prompt.';
      missionPlanning = false;
      return;
    }
    const now = traceNow();
    const current = getMissionStateForProject(activeProjectId);
    const plannerProfile = (current.profiles || []).find((entry) => entry?.id === input.plannerProfileId) || null;
    const plannerMode = input.plannerProfileId ? 'profile' : 'viewer_first';
    const plannerSummary = input.plannerProfileId
      ? `Mission planned with ${plannerProfile?.label || input.plannerProfileId}.`
      : 'Mission planned with default planner prompt.';
    const localPlan = normalizeMissionView(
      {
        ...current,
        ...input,
        summary: missionSummaryFromInput(input),
        status: 'planned',
        plannerProfileId: input.plannerProfileId || plannerProfile?.id || '',
        plannerProfileLabel: plannerProfile?.label || '',
        plannerNotes: input.plannerNotes,
        plannerPromptOverride: input.plannerPromptOverride,
        plannerSummary,
        plannerStatus: 'planned',
        plannerMode,
        viewerProfileId: '',
        viewerSessionId: '',
        reviewState: 'pending',
        applyState: 'not_applied',
        gateState: 'open',
        lastAction: 'plan',
        lastActionAt: now,
        objectives: [
          {
            id: 'objective-1',
            title: input.title || 'Mission objective',
            prompt: input.brief || input.title || 'Plan the mission slice.',
            expectedValidation: input.validationCommand || '',
            scope: {
              paths: ['src/'],
            },
            state: 'pending',
          },
        ],
        lastPlannedAt: now,
      },
      current,
    );
    try {
      const view = await invokeTraced('plan_mission', {
        missionInput: input,
      });
      const next = normalizeMissionView(view, localPlan);
      storeMissionState(activeProjectId, next);
    } catch (error) {
      missionError = extractErrorMessage(error) || 'Failed to plan mission.';
      storeMissionState(activeProjectId, appendMissionEvent(localPlan, {
        kind: 'plan',
        summary: `Planned ${localPlan.objectives.length} objective(s) locally.`,
      }));
    } finally {
      missionPlanning = false;
    }
  }

  async function runMissionCheck() {
    if (missionCheckRunning) return;
    missionCheckRunning = true;
    missionError = '';
    const current = getMissionStateForProject(activeProjectId);
    try {
      await runWatchdogCheck();
      const status = String(watchdogState?.lastVerdict?.status || 'unknown');
      const next = normalizeMissionView(
        {
          ...current,
          lastCheckedAt: traceNow(),
          lastCheckResult: {
            status,
            summary: watchdogState?.lastVerdict?.summary || '',
            confidence: watchdogState?.lastVerdict?.confidence ?? null,
          },
        },
        current,
      );
      storeMissionState(activeProjectId, appendMissionEvent(next, {
        kind: 'check',
        summary: `Mission check ${status}.`,
      }));
    } catch (error) {
      missionError = extractErrorMessage(error) || 'Mission check failed.';
    } finally {
      missionCheckRunning = false;
    }
  }

  async function runFixtureWorker() {
    if (missionFixtureRunning) return;
    missionFixtureRunning = true;
    missionError = '';
    const current = getMissionStateForProject(activeProjectId);
    const requestedObjectiveId = String(current.scheduler?.nextObjectiveId || '').trim();
    const objective =
      current.objectives?.find((entry) => entry.id === requestedObjectiveId)
      || current.objectives?.find((entry) => entry.state === 'pending')
      || current.objectives?.[0]
      || null;
    const sessionId = `fixture-${Date.now()}`;
    const now = traceNow();
    const localNext = normalizeMissionView(
      {
        ...current,
        status: 'running',
        lastRunAt: now,
        sessions: [
          ...(current.sessions || []),
          {
            sessionId,
            objectiveId: objective?.id || null,
            objectiveTitle: objective?.title || null,
            profileId: 'fixture_worker',
            workerKind: 'fixture_worker',
            state: 'completed',
            result: 'passed',
            startedAt: now,
            finishedAt: now,
            verificationId: `verification-${(current.verifications?.length || 0) + 1}`,
            latestEvent: objective ? 'completed' : 'idle',
            command: current.validationCommand || null,
            reviewState: 'pending',
            applyState: 'not_applied',
            gateState: 'open',
          },
        ],
        verifications: [
          ...(current.verifications || []),
          {
            verificationId: `verification-${(current.verifications?.length || 0) + 1}`,
            sessionId,
            objectiveId: objective?.id || null,
            objectiveTitle: objective?.title || null,
            profileId: 'fixture_worker',
            kind: 'fixture_worker',
            status: 'passed',
            summary: objective ? `Fixture worker completed ${objective.id}.` : 'Fixture worker ran.',
            checkedAt: now,
            command: current.validationCommand || null,
            reviewState: 'pending',
            applyState: 'not_applied',
            gateState: 'open',
          },
        ],
        objectives: (current.objectives || []).map((entry) => (
          entry.id === objective?.id
            ? { ...entry, state: 'done', latestSessionId: sessionId }
            : entry
        )),
        viewerProfileId: 'fixture_worker',
        viewerSessionId: sessionId,
        reviewState: 'pending',
        applyState: 'not_applied',
        gateState: 'open',
        lastAction: 'run_fixture_worker',
        lastActionAt: now,
      },
      current,
    );
    try {
      const view = await invokeTraced('run_fixture_worker', {
        missionId: current.missionId || activeProjectId,
        objectiveId: objective?.id || null,
      });
      const next = normalizeMissionView(view, localNext);
      storeMissionState(activeProjectId, next);
    } catch (error) {
      missionError = extractErrorMessage(error) || 'Fixture worker failed.';
      storeMissionState(activeProjectId, appendMissionEvent(localNext, {
        kind: 'fixture_worker',
        summary: objective ? `Fixture worker completed ${objective.id} locally.` : 'Fixture worker ran locally.',
      }));
    } finally {
      missionFixtureRunning = false;
    }
  }

  async function runCodeWorker() {
    if (missionCodeWorkerRunning) return;
    missionCodeWorkerRunning = true;
    missionError = '';
    const current = getMissionStateForProject(activeProjectId);
    const requestedObjectiveId = String(current.scheduler?.nextObjectiveId || '').trim();
    const objective =
      current.objectives?.find((entry) => entry.id === requestedObjectiveId)
      || current.objectives?.find((entry) => entry.state === 'pending')
      || current.objectives?.[0]
      || null;
    const selectedProfileId = getSelectedCodeWorkerProfileId(activeProjectId);
    const selectedProfile = current.profiles?.find((entry) => entry?.id === selectedProfileId) || null;
    const sessionId = `code-session-${(current.sessions?.length || 0) + 1}`;
    const verificationId = `verification-${(current.verifications?.length || 0) + 1}`;
    const now = traceNow();
    const patchPath = `${repoPath || '/path/to/despair'}/.sloposcope/worktrees/${current.missionId || 'mission'}/${objective?.id || 'objective-1'}/artifacts/${objective?.id || 'objective-1'}.patch`;
    const workspacePath = `${repoPath || '/path/to/despair'}/.sloposcope/worktrees/${current.missionId || 'mission'}/${objective?.id || 'objective-1'}`;
    const localNext = normalizeMissionView(
      {
        ...current,
        status: 'running',
        lastRunAt: now,
        sessions: [
          ...(current.sessions || []),
          {
            sessionId,
            objectiveId: objective?.id || null,
            objectiveTitle: objective?.title || null,
            profileId: selectedProfile?.id || 'local-code-worker',
            workerKind: 'code_worker',
            state: 'completed',
            startedAt: now,
            updatedAt: now,
            verificationId,
            workspacePath,
            patchPath,
            patchSummary: objective ? `Patch artifact for ${objective.id}` : 'Patch artifact',
            validationStatus: 'not_run',
            command: current.validationCommand || null,
            reviewState: 'pending',
            applyState: 'not_applied',
            gateState: 'open',
          },
        ],
        verifications: [
          ...(current.verifications || []),
          {
            verificationId,
            sessionId,
            objectiveId: objective?.id || null,
            objectiveTitle: objective?.title || null,
            profileId: selectedProfile?.id || 'local-code-worker',
            kind: 'code_worker',
            status: 'ready_for_review',
            summary: objective ? `Code worker completed ${objective.id}.` : 'Code worker ran.',
            checkedAt: now,
            command: current.validationCommand || null,
            workspacePath,
            patchPath,
            patchSummary: objective ? `Patch artifact for ${objective.id}` : 'Patch artifact',
            validationStatus: 'not_run',
            reviewState: 'pending',
            applyState: 'not_applied',
            gateState: 'open',
          },
        ],
        objectives: (current.objectives || []).map((entry) => (
          entry.id === objective?.id
            ? { ...entry, state: 'done', latestSessionId: sessionId }
            : entry
        )),
        viewerProfileId: selectedProfile?.id || 'local-code-worker',
        viewerSessionId: sessionId,
        reviewState: 'pending',
        applyState: 'not_applied',
        gateState: 'open',
        lastAction: 'run_code_worker',
        lastActionAt: now,
      },
      current,
    );
    try {
      const view = await invokeTraced('run_code_worker', {
        missionId: current.missionId || activeProjectId,
        objectiveId: objective?.id || null,
        profileId: selectedProfile?.id || null,
      });
      const next = normalizeMissionView(view, localNext);
      storeMissionState(activeProjectId, next);
    } catch (error) {
      missionError = extractErrorMessage(error) || 'Code worker failed.';
      storeMissionState(activeProjectId, appendMissionEvent(localNext, {
        kind: 'code_worker',
        summary: objective ? `Code worker completed ${objective.id} locally.` : 'Code worker ran locally.',
      }));
    } finally {
      missionCodeWorkerRunning = false;
    }
  }

  async function runNextCodeWorker() {
    if (missionCodeWorkerRunning) return;
    missionCodeWorkerRunning = true;
    missionError = '';
    try {
      const current = getMissionStateForProject(activeProjectId);
      const view = await watchdogOps.runNextCodeWorker(
        invokeTraced,
        current.missionId || activeProjectId,
        getSelectedCodeWorkerProfileId(activeProjectId) || null,
      );
      storeMissionState(activeProjectId, normalizeMissionView(view, current));
    } catch (error) {
      missionError = extractErrorMessage(error) || 'Next code worker failed.';
    } finally {
      missionCodeWorkerRunning = false;
    }
  }

  async function sweepStaleSessions() {
    if (missionLoading || missionCodeWorkerRunning || missionFixtureRunning || missionReviewRunning) return;
    missionLoading = true;
    missionError = '';
    try {
      const current = getMissionStateForProject(activeProjectId);
      const view = await watchdogOps.sweepStaleSessions(invokeTraced, current.missionId || activeProjectId);
      storeMissionState(activeProjectId, normalizeMissionView(view, current));
    } catch (error) {
      missionError = extractErrorMessage(error) || 'Stale sweep failed.';
    } finally {
      missionLoading = false;
    }
  }

  async function runMissionReviewAction(action, sessionId = '') {
    if (missionReviewRunning) return;
    missionReviewRunning = true;
    missionReviewAction = action;
    missionError = '';
    const current = getMissionStateForProject(activeProjectId);
    const now = traceNow();
    const targetSession = resolveMissionReviewSession(current, sessionId);
    if (!targetSession) {
      missionError = 'No mission run available.';
      missionReviewRunning = false;
      missionReviewAction = '';
      return;
    }
    if (action === 'apply' && String(targetSession.reviewState || current.reviewState || 'pending') !== 'accepted') {
      missionError = 'Mission run must be accepted before apply.';
      missionReviewRunning = false;
      missionReviewAction = '';
      return;
    }
    if (action === 'apply' && String(targetSession.gateState || current.gateState || 'open') !== 'ready') {
      missionError = 'Mission run gate must be ready before apply.';
      missionReviewRunning = false;
      missionReviewAction = '';
      return;
    }
    if (action === 'gate' && String(targetSession.reviewState || current.reviewState || 'pending') !== 'accepted') {
      missionError = 'Mission run must be accepted before gate.';
      missionReviewRunning = false;
      missionReviewAction = '';
      return;
    }

    const localNext = mutateMissionReviewState(current, targetSession.sessionId, action, now);
    if (!localNext) {
      missionError = 'No mission run available.';
      missionReviewRunning = false;
      missionReviewAction = '';
      return;
    }

    try {
      const command =
        action === 'accept'
          ? 'accept_worker_patch'
          : action === 'reject'
            ? 'reject_worker_patch'
            : action === 'apply'
              ? 'apply_worker_patch'
              : 'evaluate_worker_patch_gate';
      const view = await invokeTraced(command, {
        missionId: current.missionId || activeProjectId,
        objectiveId: targetSession.objectiveId,
      });
      const next = normalizeMissionView(view, localNext);
      storeMissionState(activeProjectId, next);
    } catch (error) {
      missionError = extractErrorMessage(error) || `Failed to ${action} mission run.`;
      storeMissionState(activeProjectId, appendMissionEvent(localNext, {
        kind: action,
        summary:
          action === 'accept'
            ? `Accepted ${targetSession.sessionId} locally.`
            : action === 'reject'
              ? `Rejected ${targetSession.sessionId} locally.`
              : action === 'apply'
                ? `Applied ${targetSession.sessionId} locally.`
                : `Gated ${targetSession.sessionId} locally.`,
      }));
    } finally {
      missionReviewRunning = false;
      missionReviewAction = '';
    }
  }

  function clearMissionProfileForm() {
    missionProfileError = '';
    missionProfileLabel = '';
    missionProfileProvider = '';
    missionProfileModel = '';
    missionProfileCommand = '';
    missionProfileArgs = '';
    missionProfileEnvJson = '{}';
    missionProfileCapabilities = '';
    missionProfileTransportKind = 'pty';
    missionProfileActive = false;
  }

  function applyMissionProfileTemplate(templateId) {
    const presets = {
      nim: {
        label: 'NIM planner',
        provider: 'nim_http',
        model: 'meta/llama3-8b-instruct',
        command: '',
        args: '',
        env: {},
        capabilities: 'can_plan\ncan_review',
        transportKind: 'http',
        active: true,
      },
      opencode_nim: {
        label: 'OpenCode on NIM',
        provider: 'opencode_nim',
        model: 'openai/gpt-oss-20b',
        command: 'opencode',
        args: '',
        env: {},
        capabilities: 'can_plan\ncan_edit\ncan_run_tools\ncan_review',
        transportKind: 'pty',
        active: false,
      },
      pi_nim: {
        label: 'PI on NIM',
        provider: 'pi_cli',
        model: 'openai/meta/llama3-8b-instruct',
        command: 'pi',
        args: '',
        env: {},
        capabilities: 'can_plan\ncan_edit\ncan_run_tools\ncan_review',
        transportKind: 'pty',
        active: true,
      },
      gemini: {
        label: 'Gemini horse',
        provider: 'gemini_cli',
        model: 'gemini-2.5-pro',
        command: 'gemini',
        args: '--sandbox\n--yolo',
        env: {},
        capabilities: 'can_edit\ncan_run_tools',
        transportKind: 'pty',
        active: false,
      },
      codex: {
        label: 'Codex horse',
        provider: 'codex_cli',
        model: 'gpt-5-codex',
        command: 'codex',
        args: '--sandbox\nworkspace-write',
        env: {},
        capabilities: 'can_edit\ncan_run_tools',
        transportKind: 'pty',
        active: false,
      },
      claude: {
        label: 'Claude horse',
        provider: 'claude_cli',
        model: 'claude-sonnet-4',
        command: 'claude',
        args: '--dangerously-skip-permissions',
        env: {},
        capabilities: 'can_edit\ncan_run_tools',
        transportKind: 'pty',
        active: false,
      },
      local: {
        label: 'Local shell horse',
        provider: 'local_cli',
        model: 'shell',
        command: '/bin/sh',
        args: '-lc\necho ok',
        env: {},
        capabilities: 'can_edit',
        transportKind: 'pty',
        active: false,
      },
    };
    const preset = presets[String(templateId || '').trim()];
    if (!preset) return;
    missionProfileError = '';
    missionProfileLabel = preset.label;
    missionProfileProvider = preset.provider;
    missionProfileModel = preset.model;
    missionProfileCommand = preset.command;
    missionProfileArgs = preset.args;
    missionProfileEnvJson = watchdogOps.formatJsonObjectInput(preset.env);
    missionProfileCapabilities = preset.capabilities;
    missionProfileTransportKind = preset.transportKind;
    missionProfileActive = preset.active;
  }

  async function saveMissionProfile() {
    if (missionProfileSaving) return;
    if (missionProfileEnvError) {
      missionProfileError = missionProfileEnvError;
      return;
    }
    missionProfileSaving = true;
    missionProfileError = '';
    const profile = watchdogOps.buildAgentProfileInputFromForm({
      label: missionProfileLabel,
      provider: missionProfileProvider,
      model: missionProfileModel,
      command: missionProfileCommand,
      args: missionProfileArgs,
      envJson: missionProfileEnvJson,
      capabilities: missionProfileCapabilities,
      transportKind: missionProfileTransportKind,
      active: missionProfileActive,
    });
    try {
      const response = await watchdogOps.saveAgentProfile(invokeTraced, profile);
      const profiles = Array.isArray(response?.profiles) ? response.profiles : [];
      const next = normalizeMissionView(
        {
          ...getMissionStateForProject(activeProjectId),
          profiles,
        },
        getMissionStateForProject(activeProjectId),
      );
      storeMissionState(activeProjectId, next);
      const savedProfile = profiles.find((entry) => entry?.label === profile.label) || null;
      if (savedProfile?.id) setSelectedCodeWorkerProfileId(savedProfile.id, activeProjectId);
      clearMissionProfileForm();
    } catch (error) {
      missionProfileError = extractErrorMessage(error) || 'Failed to save profile.';
    } finally {
      missionProfileSaving = false;
    }
  }

  async function deleteMissionProfile(profileId) {
    if (missionProfileDeletingId) return;
    missionProfileDeletingId = String(profileId || '').trim();
    missionProfileError = '';
    try {
      const response = await watchdogOps.deleteAgentProfile(invokeTraced, missionProfileDeletingId);
      const profiles = Array.isArray(response?.profiles) ? response.profiles : [];
      const next = normalizeMissionView(
        {
          ...getMissionStateForProject(activeProjectId),
          profiles,
        },
        getMissionStateForProject(activeProjectId),
      );
      storeMissionState(activeProjectId, next);
      if (getSelectedCodeWorkerProfileId(activeProjectId) === missionProfileDeletingId) {
        setSelectedCodeWorkerProfileId('', activeProjectId);
      }
    } catch (error) {
      missionProfileError = extractErrorMessage(error) || 'Failed to delete profile.';
    } finally {
      missionProfileDeletingId = '';
    }
  }

  async function refreshWatchdogState(targetProjectId = activeProjectId, includeTraces = true) {
    if (!targetProjectId || targetProjectId !== activeProjectId) return;
    const requestId = ++watchdogLoadRequestSeq;
    watchdogLoading = true;
    watchdogError = '';
    try {
      const { stateView, traces } = await watchdogOps.fetchWatchdogState(invokeTraced, includeTraces);
      if (requestId !== watchdogLoadRequestSeq || targetProjectId !== activeProjectId) return;
      applyWatchdogView(stateView);
      if (includeTraces && requestId === watchdogLoadRequestSeq && targetProjectId === activeProjectId) {
        watchdogTraces = traces;
      }
    } catch (error) {
      if (requestId === watchdogLoadRequestSeq && targetProjectId === activeProjectId) {
        watchdogError = extractErrorMessage(error) || 'Failed to load watchdog state.';
      }
    } finally {
      if (requestId === watchdogLoadRequestSeq && targetProjectId === activeProjectId) {
        watchdogLoading = false;
      }
    }
  }

  async function saveWatchdogIssueContext() {
    const issueId = deriveTaskIdFromForm();
    const issueTitle = String(watchdogIssueTitle || '').trim();
    const issueSummary = String(watchdogIssueSummary || '').trim();
    if (!issueTitle || !issueSummary) {
      watchdogError = 'Task title and brief are required.';
      return;
    }

    watchdogLoading = true;
    watchdogError = '';
    try {
      watchdogIssueId = issueId;
      const issueContext = watchdogOps.buildWatchdogIssueContextFromForm({
        issueId,
        issueTitle: watchdogIssueTitle,
        issueIdentifier: watchdogIssueIdentifier,
        issueUrl: watchdogIssueUrl,
        issueSummary: watchdogIssueSummary,
        acceptanceCriteria: watchdogAcceptanceCriteria,
        nonGoals: watchdogNonGoals,
        issueLabels: watchdogIssueLabels,
        issuePriority: watchdogIssuePriority,
      });
      const stateView = await watchdogOps.saveWatchdogIssueContext(invokeTraced, issueContext);
      applyWatchdogView(stateView);
    } catch (error) {
      watchdogError = extractErrorMessage(error) || 'Failed to save issue context.';
    } finally {
      watchdogLoading = false;
    }
  }

  async function clearWatchdogIssueContext() {
    watchdogLoading = true;
    watchdogError = '';
    try {
      const stateView = await watchdogOps.clearWatchdogIssueContext(invokeTraced);
      applyWatchdogView(stateView);
    } catch (error) {
      watchdogError = extractErrorMessage(error) || 'Failed to clear issue context.';
    } finally {
      watchdogLoading = false;
    }
  }

  async function recordWatchdogValidation(kind, success) {
    watchdogLoading = true;
    watchdogError = '';
    try {
      const details = toOptionalString(watchdogValidationDetails);
      const stateView = await watchdogOps.recordWatchdogValidation(invokeTraced, {
        kind,
        success,
        ranAt: new Date().toISOString(),
        details,
      });
      applyWatchdogView(stateView);
    } catch (error) {
      watchdogError = extractErrorMessage(error) || 'Failed to record validation.';
    } finally {
      watchdogLoading = false;
    }
  }

  async function runWatchdogValidationCommand(kind) {
    const command = toOptionalString(watchdogValidationDetails);
    if (!command) {
      await recordWatchdogValidation(kind, true);
      return;
    }

    watchdogLoading = true;
    watchdogError = '';
    try {
      const stateView = await watchdogOps.runWatchdogValidationCommand(
        invokeTraced,
        kind,
        command,
      );
      applyWatchdogView(stateView);
    } catch (error) {
      watchdogError = extractErrorMessage(error) || 'Failed to run validation command.';
    } finally {
      watchdogLoading = false;
    }
  }

  async function runWatchdogCheck() {
    if (watchdogRunning) return;
    watchdogRunning = true;
    watchdogError = '';
    try {
      await watchdogOps.triggerWatchdogCheck(invokeTraced);
      await refreshWatchdogState(activeProjectId, true);
    } catch (error) {
      watchdogError = extractErrorMessage(error) || 'Watchdog check failed.';
    } finally {
      watchdogRunning = false;
    }
  }

  async function saveWatchdogConfig() {
    if (watchdogConfigSaving) return;
    watchdogConfigSaving = true;
    watchdogError = '';
    try {
      const stateView = await invokeTraced('save_watchdog_repo_config', {
        config: {
          enabled: watchdogConfigEnabled,
          triggerMode: watchdogConfigTriggerMode,
          alertThreshold: watchdogConfigAlertThreshold,
          requireActiveIssue: watchdogConfigRequireActiveIssue,
          maxFilesInSnapshot: Number(watchdogConfigMaxFiles),
          maxDiffLinesPerFile: Number(watchdogConfigMaxDiffLines),
          dedupeWindowSeconds: Number(watchdogConfigDedupeWindow),
          checks: watchdogConfigChecks,
        },
      });
      applyWatchdogView(stateView);
    } catch (error) {
      watchdogError = extractErrorMessage(error) || 'Failed to save watchdog config.';
    } finally {
      watchdogConfigSaving = false;
    }
  }

  function applyConductorStatus(view) {
    conductorStatus = view || createDefaultConductorStatus(Number(conductorTraceLimit || 20));
  }

  function applyConductorWorkflowLoad(view) {
    const derived = watchdogOps.deriveConductorWorkflowValues(
      view,
      createDefaultConductorWorkflowLoad,
    );
    conductorWorkflowLoad = derived.load;
    conductorWorkflowProvider = derived.form.provider;
    conductorWorkflowProviderModel = derived.form.providerModel;
    conductorWorkflowTimeout = derived.form.timeoutSeconds;
    conductorWorkflowTraceRetention = derived.form.traceRetention;
    conductorWorkflowStrictMode = derived.form.strictMode;
  }

  function parseConductorNumber(value, fallback, min, max) {
    return watchdogOps.parseConductorNumber(value, fallback, min, max);
  }

  async function refreshConductorStatus(targetProjectId = activeProjectId, runTick = false) {
    if (!targetProjectId || targetProjectId !== activeProjectId) return;
    conductorLoading = true;
    conductorError = '';
    try {
      const traceLimit = parseConductorNumber(conductorTraceLimit, 20, 1, 100);
      const status = await watchdogOps.fetchConductorStatus(invokeTraced, traceLimit, runTick);
      applyConductorStatus(status);
    } catch (error) {
      conductorError = extractErrorMessage(error) || 'Failed to refresh conductor status.';
    } finally {
      conductorLoading = false;
    }
  }

  async function refreshConductorWorkflow(targetProjectId = activeProjectId) {
    if (!targetProjectId || targetProjectId !== activeProjectId) return;
    conductorWorkflowLoading = true;
    conductorError = '';
    try {
      const load = await watchdogOps.fetchConductorWorkflow(invokeTraced);
      if (targetProjectId !== activeProjectId) return;
      applyConductorWorkflowLoad(load);
    } catch (error) {
      if (targetProjectId === activeProjectId) {
        conductorError = extractErrorMessage(error) || 'Failed to load workflow settings.';
      }
    } finally {
      if (targetProjectId === activeProjectId) {
        conductorWorkflowLoading = false;
      }
    }
  }

  async function reloadConductorPanel(targetProjectId = activeProjectId, runTick = false) {
    await Promise.all([
      refreshConductorStatus(targetProjectId, runTick),
      refreshConductorWorkflow(targetProjectId),
    ]);
  }

  async function saveConductorWorkflowSettings() {
    if (conductorSettingsSaving) return;
    conductorSettingsSaving = true;
    conductorError = '';
    try {
      const settings = {
        provider: String(conductorWorkflowProvider || 'coding_agent_app_server')
          .trim()
          .toLowerCase(),
        providerModel: String(conductorWorkflowProviderModel || ''),
        providerStrictMode: Boolean(conductorWorkflowStrictMode),
        timeoutSeconds: parseConductorNumber(conductorWorkflowTimeout, 900, 10, 3600),
        traceRetention: parseConductorNumber(conductorWorkflowTraceRetention, 30, 1, 200),
      };
      const load = await watchdogOps.saveConductorWorkflowSettings(invokeTraced, settings);
      applyConductorWorkflowLoad(load);
      await refreshConductorStatus(activeProjectId, false);
    } catch (error) {
      conductorError = extractErrorMessage(error) || 'Failed to save workflow settings.';
    } finally {
      conductorSettingsSaving = false;
    }
  }

  async function enqueueConductorTask() {
    const taskId = String(conductorTaskId || '').trim();
    if (!taskId) {
      conductorError = 'Task id is required.';
      return;
    }
    if (conductorRunning) return;
    conductorRunning = true;
    conductorError = '';
    try {
      const maxRetries = parseConductorNumber(conductorMaxRetries, 2, 0, 10);
      await watchdogOps.enqueueConductorTask(invokeTraced, taskId, maxRetries);
      conductorTaskId = '';
      await reloadConductorPanel(activeProjectId, true);
    } catch (error) {
      conductorError = extractErrorMessage(error) || 'Failed to enqueue task.';
    } finally {
      conductorRunning = false;
    }
  }

  async function tickConductorNow() {
    if (conductorRunning) return;
    conductorRunning = true;
    conductorError = '';
    try {
      await reloadConductorPanel(activeProjectId, true);
    } catch (error) {
      conductorError = extractErrorMessage(error) || 'Failed to run conductor tick.';
    } finally {
      conductorRunning = false;
    }
  }

  async function prepareConductorLaunch(taskId) {
    if (conductorLaunchLoading) return;
    conductorLaunchLoading = true;
    conductorLaunchResult = null;
    conductorError = '';
    try {
      conductorLaunchResult = await watchdogOps.prepareConductorLaunch(invokeTraced, taskId);
      await refreshConductorStatus(activeProjectId, false);
    } catch (error) {
      conductorError = extractErrorMessage(error) || 'Failed to prepare launch.';
    } finally {
      conductorLaunchLoading = false;
    }
  }

  async function dispatchConductorLaunch(taskId) {
    if (conductorDispatchLoading) return;
    conductorDispatchLoading = true;
    conductorLaunchResult = null;
    conductorError = '';
    try {
      conductorLaunchResult = await watchdogOps.dispatchConductorLaunch(invokeTraced, taskId);
      await refreshConductorStatus(activeProjectId, false);
    } catch (error) {
      conductorError = extractErrorMessage(error) || 'Dispatch failed.';
    } finally {
      conductorDispatchLoading = false;
    }
  }

  async function ingestConductorRunnerEvent() {
    const taskId = String(conductorIngestTaskId || '').trim();
    const rawEvent = String(conductorIngestRawEvent || '').trim();
    if (!taskId || !rawEvent) {
      conductorError = 'Task ID and event JSON are required.';
      return;
    }
    if (conductorIngestLoading) return;
    conductorIngestLoading = true;
    conductorError = '';
    try {
      await watchdogOps.ingestConductorRunnerEvent(invokeTraced, taskId, rawEvent);
      conductorIngestRawEvent = '';
      await reloadConductorPanel(activeProjectId, true);
    } catch (error) {
      conductorError = extractErrorMessage(error) || 'Failed to ingest event.';
    } finally {
      conductorIngestLoading = false;
    }
  }

  async function refreshGraphClusters() {
    if (clusterRefreshLoading) return;
    clusterRefreshLoading = true;
    watchdogError = '';
    try {
      await invokeTraced('refresh_graph_clusters', {}, { warnAfterMs: 30000 });
    } catch (error) {
      watchdogError = extractErrorMessage(error) || 'Failed to refresh graph clusters.';
    } finally {
      clusterRefreshLoading = false;
    }
  }

  function setRepoActionMessage(message) {
    repoActionMessage = message ? String(message) : '';
  }

  function createProjectSession(path) {
    return projectSessions.createProjectSession(path, CHANGES_PAGE_SIZE);
  }

  function getProjectById(id) {
    return projectSessions.getProjectById(projectTabs, id);
  }

  function getProjectByPath(path) {
    return projectSessions.getProjectByPath(projectTabs, path);
  }

  function getActiveProject() {
    return getProjectById(activeProjectId);
  }

  function upsertProject(path) {
    const result = projectSessions.upsertProject(projectTabs, path, CHANGES_PAGE_SIZE);
    projectTabs = result.projectTabs;
    return result.project;
  }

  function applyProjectToGlobals(project) {
    if (!project) return;
    const globals = projectSessions.projectToGlobals(project);
    repoPath = globals.repoPath;
    graph = globals.graph;
    graphViewMode = globals.graphViewMode;
    graphMeta = globals.graphMeta;
    changes = globals.changes;
    changesLimit = globals.changesLimit;
    hasMoreChanges = globals.hasMoreChanges;
    bookmarks = globals.bookmarks;
    currentBranch = globals.currentBranch;
    currentRevision = globals.currentRevision;
    since = globals.since;
    heatCounter = globals.heatCounter;
    heatmapData = globals.heatmapData;
    fileContentVersions = globals.fileContentVersions;
    loadingMore = globals.loadingMore;
  }

  function syncGlobalsToActiveProject() {
    const project = getActiveProject();
    if (!project) return;
    projectSessions.applyProjectGlobals(project, {
      repoPath,
      graph,
      graphViewMode,
      graphMeta,
      changes,
      changesLimit,
      hasMoreChanges,
      bookmarks,
      currentBranch,
      currentRevision,
      since,
      heatCounter,
      heatmapData,
      fileContentVersions,
      loadingMore,
    });
  }

  function refreshFortune() {
    try {
      updateFortune();
    } catch (_e) {
      // no-op
    }
    if (!currentFortune || currentFortune.trim() === '') {
      currentFortune = getFortune([]);
    }
  }

  function updateFortune() {
    const committers = changes.map(c => c.description.split(' ')[0]).filter(n => n && n.length > 2);
    currentFortune = getFortune(committers);
  }

  function getShortName(id) {
    if (!id) return '';
    
    if (id.includes('/')) {
      const parts = id.split('/');
      return parts[parts.length - 1]; // Return full filename: "config.exs"
    }
    
    if (id.includes('.')) {
      const parts = id.split('.');
      const last = parts[parts.length - 1];
      
      // Known file extensions
      const knownExts = ['ex', 'exs', 'svelte', 'js', 'ts', 'jsx', 'tsx', 'rs', 'py', 'rb', 'go', 'java', 'cpp', 'h', 'php', 'cs', 'json', 'md'];
      if (knownExts.includes(last.toLowerCase())) {
        return id; 
      }
      return last;
    }
    
    return id;
  }

  let filteredItems = $derived.by(() => {
    const q = (since || '').toLowerCase();
    
    const matchedBookmarks = bookmarks
      .filter(b => b.name.toLowerCase().includes(q))
      .map(b => ({ id: b.name, description: 'Bookmark', type: 'bookmark' }));
      
    const matchedChanges = changes
      .filter(c => c.id.toLowerCase().includes(q) || c.description.toLowerCase().includes(q))
      .map(c => ({ ...c, type: 'commit' }));

    return [...matchedBookmarks, ...matchedChanges];
  });

  async function loadTimelineChanges(targetProjectId = activeProjectId) {
    const project = getProjectById(targetProjectId);
    if (!project) return;
    const c = await invokeTraced('get_changes', { limit: ALL_CHANGES_LIMIT }, { warnAfterMs: 8000 });
    const unique = uniqueChanges(c);
    debugLog(`[FRONTEND][TRACE] timeline fetched raw=${c?.length || 0}, unique=${unique.length}`);
    project.changes = unique;
    project.changesLimit = unique.length;
    project.hasMoreChanges = false; // We paginate locally after prefetching full history.

    if (activeProjectId === targetProjectId) {
      changes = unique;
      changesLimit = unique.length;
      hasMoreChanges = false;
      refreshFortune();
    }
  }

  function buildGraphViewRequest(project) {
    const mode = String(project?.graphViewMode || 'investigation');
    const trimmedSince = String(project?.since || '@').trim() || '@';
    const seed =
      mode === 'atlas'
        ? null
        : trimmedSince !== '@'
          ? { kind: 'commit', revset: trimmedSince }
          : { kind: 'working_copy' };
    return {
      mode,
      seed,
      depth: 1,
      minConfidence: 0,
      aggregate: 'file',
    };
  }

  async function setGraphViewMode(mode, targetProjectId = activeProjectId) {
    const project = getProjectById(targetProjectId);
    if (!project) return;
    project.graphViewMode = mode === 'atlas' ? 'atlas' : 'investigation';
    if (activeProjectId === targetProjectId) {
      graphViewMode = project.graphViewMode;
    }
    await loadGraph(false, targetProjectId);
  }

  async function loadGraph(isInitial = false, targetProjectId = activeProjectId) {
    const project = getProjectById(targetProjectId);
    if (!project) return;
    const requestId = ++loadGraphRequestSeq;
    const requestStartedAt = performance.now();
    const previousBranch = project.currentBranch || '';
    const request = buildGraphViewRequest(project);
    inFlightLoadGraph += 1;
    debugLog(
      `[FRONTEND][BRANCH][req:${requestId}] loadGraph start initial=${isInitial} mode=${request.mode} branch='${previousBranch}' inFlight=${inFlightLoadGraph}`
    );

    if (activeProjectId === targetProjectId) {
      // Always use the subtle overlay — the full preloader is only shown on
      // first mount (loading starts true). Background builds show progress
      // in the controls bar instead of blocking the whole UI.
      refreshing = true;
      refreshFortune();
    }

    // Safety timeout to clear loader if backend hangs
    const safetyTimeout = setTimeout(async () => {
      if (activeProjectId === targetProjectId) {
        await tick();
        loading = false;
        refreshing = false;
      }
    }, 10000);

    try {
      const [view, b, curr] = await Promise.all([
        invokeTraced('get_graph_view', { request }, { warnAfterMs: 8000 }),
        invokeTraced('get_bookmarks', {}, { warnAfterMs: 8000 }),
        invokeTraced('get_current_branch', {}, { warnAfterMs: 8000 }),
      ]);
      const g = view.graph;

      const elapsedMs = Math.round(performance.now() - requestStartedAt);
      debugLog(
        `[FRONTEND][BRANCH][req:${requestId}] loadGraph done in ${elapsedMs}ms mode=${view.mode} nodes=${g.nodes.length} edges=${g.edges.length} bookmarks=${b.length} branch='${previousBranch}'->'${curr}'`
      );
      project.graph = g;
      project.graphMeta = view;
      project.graphViewMode = view.mode || request.mode;
      project.bookmarks = b;
      project.currentBranch = curr;
      seedHeatmapFromGraph(g, targetProjectId);
      if (activeProjectId === targetProjectId) {
        graph = g;
        graphMeta = view;
        graphViewMode = project.graphViewMode;
        bookmarks = b;
        currentBranch = curr;
      }
    } catch (e) {
      console.error(`[FRONTEND][BRANCH][req:${requestId}] loadGraph failed`, e);
    } finally {
      clearTimeout(safetyTimeout);
      if (activeProjectId === targetProjectId) {
        await tick();
        loading = false;
        refreshing = false;
      }
      inFlightLoadGraph = Math.max(0, inFlightLoadGraph - 1);
      debugLog(`[FRONTEND][BRANCH][req:${requestId}] loadGraph finish inFlight=${inFlightLoadGraph}`);
    }
  }

  async function loadMoreChanges(targetProjectId = activeProjectId) {
    const project = getProjectById(targetProjectId);
    if (!project) return 0;
    if (project.loadingMore || project.changes.length === 0 || !project.hasMoreChanges) return 0;
    if (activeProjectId === targetProjectId) {
      refreshFortune();
      loadingMore = true;
    }
    project.loadingMore = true;
    let loadedCount = 0;
    try {
      const targetLimit = Math.max(project.changes.length, project.changesLimit) + CHANGES_PAGE_SIZE;
      const expanded = await invokeTraced('get_changes', { limit: targetLimit }, { warnAfterMs: 8000 });
      const expandedUnique = uniqueChanges(expanded);
      const existingIds = new Set(project.changes.map(c => c.id));
      const tail = expandedUnique.filter(c => !existingIds.has(c.id));

      if (tail.length > 0) {
        project.changes = [...project.changes, ...tail];
        loadedCount = tail.length;
      }

      project.changesLimit = Math.max(project.changesLimit, expandedUnique.length);
      if (expandedUnique.length < targetLimit || loadedCount === 0) {
        project.hasMoreChanges = false;
      }
      if (activeProjectId === targetProjectId) {
        changes = project.changes;
        changesLimit = project.changesLimit;
        hasMoreChanges = project.hasMoreChanges;
      }
    } catch (e) {
      console.error('[FRONTEND] Error loading more changes:', e);
    } finally {
      project.loadingMore = false;
      if (activeProjectId === targetProjectId) loadingMore = false;
    }
    return loadedCount;
  }

  function setSince(revset, event) {
    const previousSince = since;
    const previousBranch = currentBranch;
    const isMultiToggle = Boolean(event && (event.shiftKey || event.metaKey || event.ctrlKey));
    const isBookmark = bookmarks.some((b) => b.name === revset || b.id === revset);
    debugLog(
      `[FRONTEND][BRANCH] setSince input='${revset}' prev='${previousSince}' branch='${previousBranch}' multi=${isMultiToggle} bookmarkMatch=${isBookmark}`
    );
    if (event && (event.shiftKey || event.metaKey || event.ctrlKey)) {
      // Toggle logic for multi-select
      const current = (since || '').split(' | ').filter(s => s && s !== '');
      if (current.includes(revset)) {
        since = current.filter(s => s !== revset).join(' | ');
      } else {
        since = [...current, revset].join(' | ');
      }
    } else {
      since = revset;
    }
    
    if (!since || since === '') since = '@';
    const active = getActiveProject();
    if (active) {
      active.since = since;
    }
    debugLog(`[FRONTEND][BRANCH] setSince resolved='${since}' (from '${previousSince}') -> triggering loadGraph`);
    loadGraph(false, activeProjectId);
  }

  async function activateProjectTab(
    projectId,
    { skipBackendSet = false, isInitial = false, waitForRefresh = false } = {}
  ) {
    const next = getProjectById(projectId);
    if (!next) return;

    if (activeProjectId !== projectId) {
      syncGlobalsToActiveProject();
      syncMissionStateToProject(activeProjectId);
      activeProjectId = projectId;
      applyProjectToGlobals(next);
      const nextMissionState = getMissionStateForProject(projectId);
      missionState = nextMissionState;
      missionTitle = nextMissionState.title || '';
      missionBrief = nextMissionState.brief || '';
      missionValidationCommand = nextMissionState.validationCommand || '';
      missionIssueKey = nextMissionState.issueKey || '';
      missionIssueUrl = nextMissionState.issueUrl || '';
      missionPlannerProfileId = nextMissionState.plannerProfileId || '';
      missionPlannerNotes = nextMissionState.plannerNotes || '';
      missionPlannerPromptOverride = nextMissionState.plannerPromptOverride || '';
      selectedCodeWorkerProfileId = getSelectedCodeWorkerProfileId(projectId);
    }

    showDropdown = false;
    highlightedIndex = -1;

    // Clear any stale build progress from a previous repo and show refresh overlay.
    if (activeProjectId === projectId) {
      graphBuildProgress = null;
      refreshing = true;
    }

    if (!skipBackendSet) {
      try {
        const switchedPath = await invokeTraced(
          'set_repo_path',
          { path: next.path },
          { warnAfterMs: 10000 }
        );
        next.path = switchedPath;
        next.label = shortRepoLabel(switchedPath);
        if (activeProjectId === projectId) repoPath = switchedPath;
      } catch (e) {
        refreshing = false;
        console.error('[FRONTEND] Failed to activate project tab:', e);
        return;
      }
    }

    const refreshPromise = Promise.all([
      loadTimelineChanges(projectId),
      loadGraph(isInitial, projectId),
      refreshWatchdogState(projectId, true),
      reloadConductorPanel(projectId, false),
      refreshMissionState(projectId),
    ]);

    if (waitForRefresh) {
      await refreshPromise;
    } else {
      refreshPromise.catch((e) => console.error('[FRONTEND] Project refresh failed:', e));
    }
  }

  function closeProjectTab(projectId, event) {
    event?.stopPropagation?.();
    if (projectTabs.length <= 1) return;
    const idx = projectTabs.findIndex((p) => p.id === projectId);
    if (idx < 0) return;

    const wasActive = activeProjectId === projectId;
    const remaining = projectTabs.filter((p) => p.id !== projectId);
    projectTabs = remaining;

    if (!wasActive) return;
    const next = remaining[Math.max(0, idx - 1)] || remaining[0];
    if (!next) return;
    activateProjectTab(next.id).catch((e) =>
      console.error('[FRONTEND] Failed to activate fallback project tab:', e)
    );
  }

  function isEditableTarget(target) {
    if (!(target instanceof HTMLElement)) return false;
    return (
      target.tagName === 'INPUT' ||
      target.tagName === 'TEXTAREA' ||
      target.tagName === 'SELECT' ||
      target.isContentEditable
    );
  }

  function activateRelativeProjectTab(step) {
    const total = projectTabs.length;
    if (total <= 1) return;
    const currentIndex = projectTabs.findIndex((project) => project.id === activeProjectId);
    if (currentIndex < 0) return;
    const nextIndex = (currentIndex + step + total) % total;
    const nextProject = projectTabs[nextIndex];
    if (!nextProject || nextProject.id === activeProjectId) return;
    activateProjectTab(nextProject.id).catch((e) =>
      console.error('[FRONTEND] Failed to activate project tab from hotkey:', e)
    );
  }

  function activateProjectTabByNumber(numberKey) {
    const total = projectTabs.length;
    if (total === 0) return;
    const index = numberKey === 9 ? total - 1 : Math.min(numberKey - 1, total - 1);
    const targetProject = projectTabs[index];
    if (!targetProject || targetProject.id === activeProjectId) return;
    activateProjectTab(targetProject.id).catch((e) =>
      console.error('[FRONTEND] Failed to activate numbered project tab:', e)
    );
  }

  function handleAppHotkeys(event) {
    if (!(event.metaKey || event.ctrlKey)) return;
    if (event.altKey) return;
    if (isEditableTarget(event.target)) return;
    const key = String(event.key || '').toLowerCase();

    if (key === 'n') {
      event.preventDefault();
      selectRepo();
      return;
    }

    if (key === 'o') {
      event.preventDefault();
      changeCurrentRepo();
      return;
    }

    if (event.key === ']' || event.code === 'BracketRight') {
      event.preventDefault();
      activateRelativeProjectTab(1);
      return;
    }

    if (event.key === '[' || event.code === 'BracketLeft') {
      event.preventDefault();
      activateRelativeProjectTab(-1);
      return;
    }

    const digit = Number(event.key);
    if (Number.isInteger(digit) && digit >= 1 && digit <= 9) {
      event.preventDefault();
      activateProjectTabByNumber(digit);
    }
  }

  async function selectRepo() {
    try {
      const newPath = await invokeTraced('pick_folder', {}, { warnAfterMs: 60000 });
      debugLog(`[FRONTEND][TRACE] repo selected: ${newPath}`);
      setRepoActionMessage('');
      const project = upsertProject(newPath);
      project.path = newPath;
      project.label = shortRepoLabel(newPath);
      // Switch to new tab immediately — activateProjectTab shows the splash
      // and calls set_repo_path + loadGraph internally.
      await activateProjectTab(project.id, {
        skipBackendSet: false,
        isInitial: true,
        waitForRefresh: true,
      });
    } catch (e) {
      const message = extractErrorMessage(e);
      const lowered = message.toLowerCase();
      if (lowered.includes('no folder selected') || lowered.includes('dialog cancelled')) {
        debugLog('Dialog cancelled', e);
        return;
      }
      setRepoActionMessage(message || 'Failed to open selected directory.');
      console.error('[FRONTEND] selectRepo failed:', e);
    }
  }

  async function changeCurrentRepo() {
    const current = getActiveProject();
    if (!current) return;
    try {
      const newPath = await invokeTraced('pick_folder', {}, { warnAfterMs: 60000 });
      setRepoActionMessage('');
      debugLog(`[FRONTEND][TRACE] change current repo -> ${newPath}`);

      // Already open in another tab — close current, switch to existing
      const duplicate = getProjectByPath(newPath);
      if (duplicate && duplicate.id !== current.id) {
        projectTabs = projectTabs.filter((p) => p.id !== current.id);
        await activateProjectTab(duplicate.id, {
          skipBackendSet: false,
          isInitial: true,
          waitForRefresh: true,
        });
        return;
      }

      // Same path — just reload
      if (newPath === current.path) {
        await activateProjectTab(current.id, {
          skipBackendSet: false,
          isInitial: true,
          waitForRefresh: true,
        });
        return;
      }

      // Replace current tab with new project; activateProjectTab shows splash + builds
      const replacement = createProjectSession(newPath);
      const index = projectTabs.findIndex((p) => p.id === current.id);
      if (index >= 0) {
        const nextTabs = [...projectTabs];
        nextTabs[index] = replacement;
        projectTabs = nextTabs;
      } else {
        projectTabs = [replacement];
      }
      await activateProjectTab(replacement.id, {
        skipBackendSet: false,
        isInitial: true,
        waitForRefresh: true,
      });
    } catch (e) {
      const message = extractErrorMessage(e);
      const lowered = message.toLowerCase();
      if (lowered.includes('no folder selected') || lowered.includes('dialog cancelled')) {
        debugLog('Dialog cancelled', e);
        return;
      }
      setRepoActionMessage(message || 'Failed to change directory.');
      console.error('[FRONTEND] changeCurrentRepo failed:', e);
    }
  }

  let theme = $state(typeof localStorage !== 'undefined' ? (localStorage.getItem('codegraph-theme') || 'midnight') : 'midnight');

  function toggleTheme() {
    theme = theme === 'midnight' ? 'victorian' : 'midnight';
    if (typeof localStorage !== 'undefined') localStorage.setItem('codegraph-theme', theme);
    applyTheme();
  }

  function applyTheme() {
    document.documentElement.setAttribute('data-theme', theme);
  }

  async function getFileSource(file) {
    return invokeTraced('get_file_source', { file }, { warnAfterMs: 6000 });
  }

  async function getFileDiff(file) {
    return invokeTraced('get_file_diff', { file, since: since === '@' ? null : since }, { warnAfterMs: 6000 });
  }

  async function saveFile(file, content) {
    return invokeTraced('save_file', { file, content }, { warnAfterMs: 6000 });
  }

  onMount(async () => {
    const mountStart = performance.now();
    debugLog(`[FRONTEND][TRACE] ${traceNow()} onMount start`);
    applyTheme();
    debugLog(`[FRONTEND][TRACE] ${traceNow()} theme applied`);
    refreshFortune();
    debugLog(`[FRONTEND][TRACE] ${traceNow()} fortune refreshed`);
    const initialRepoPath = await invokeTraced('get_repo_path', {}, { warnAfterMs: 8000 });
    const initialProject = upsertProject(initialRepoPath);
    activeProjectId = initialProject.id;
    applyProjectToGlobals(initialProject);
    debugLog(`[FRONTEND][TRACE] ${traceNow()} repo path resolved: ${initialRepoPath}`);
    await Promise.all([
      loadTimelineChanges(initialProject.id),
      loadGraph(true, initialProject.id),
      refreshWatchdogState(initialProject.id, true),
      reloadConductorPanel(initialProject.id, false),
      refreshMissionState(initialProject.id),
    ]);
    debugLog(`[FRONTEND][TRACE] ${traceNow()} initial data loaded in ${Math.round(performance.now() - mountStart)}ms`);

    listen('graph-build-progress', (event) => {
      const { filesProcessed, filesTotal } = event?.payload || {};
      if (typeof filesTotal === 'number' && filesTotal > 0) {
        graphBuildProgress = { filesProcessed: filesProcessed ?? 0, filesTotal };
      }
    }).catch((e) => {
      console.error('[FRONTEND] Failed to register graph-build-progress listener:', e);
    });

    listen('graph-updated', (event) => {
      graphBuildProgress = null;
      const startedAt = performance.now();
      const activeProject = getActiveProject();
      if (!activeProject) return;
      const previousBranch = activeProject.currentBranch || '';
      const previousRevision = activeProject.currentRevision || '';
      const payload = event?.payload || {};
      const payloadBranch = payload.current_branch || '';
      const payloadRevision = payload.current_revision || '';
      const payloadSince = typeof payload.since === 'string' ? payload.since : '';
      const payloadSinceReset = Boolean(payload.since_reset);
      const payloadNodes = payload.graph?.nodes?.length || 0;
      const payloadEdges = payload.graph?.edges?.length || 0;
      const payloadBookmarks = Array.isArray(payload.bookmarks) ? payload.bookmarks.length : 0;
      debugLog(
        `[FRONTEND][BRANCH][event] graph-updated recv branch='${previousBranch}'->'${payloadBranch || previousBranch}' rev='${previousRevision || '-'}'->'${payloadRevision || previousRevision || '-'}' since='${activeProject.since}'->'${payloadSince || activeProject.since}' reset=${payloadSinceReset} nodes=${payloadNodes} edges=${payloadEdges} bookmarks=${payloadBookmarks}`
      );
      activeProject.graph = event.payload.graph;
      if (payload.graph_view) {
        activeProject.graphMeta = payload.graph_view;
        activeProject.graphViewMode = payload.graph_view.mode || activeProject.graphViewMode || 'investigation';
      }
      seedHeatmapFromGraph(event.payload.graph, activeProject.id);
      if (event.payload.current_branch) {
        activeProject.currentBranch = event.payload.current_branch;
      }
      if (event.payload.current_revision) {
        activeProject.currentRevision = event.payload.current_revision;
      }
      if (Array.isArray(event.payload.bookmarks)) {
        activeProject.bookmarks = event.payload.bookmarks;
      }
      const branchChanged = Boolean(payloadBranch) && payloadBranch !== previousBranch;
      const revisionChanged = Boolean(payloadRevision) && payloadRevision !== previousRevision;
      if ((payloadSinceReset || branchChanged) && payloadSince && activeProject.since !== payloadSince) {
        activeProject.since = payloadSince;
      }
      applyProjectToGlobals(activeProject);
      if (branchChanged || revisionChanged) {
        loadTimelineChanges(activeProject.id)
          .then(() => {
            debugLog(
              `[FRONTEND][BRANCH][event] timeline refreshed in ${Math.round(performance.now() - startedAt)}ms (branchChanged=${branchChanged} revisionChanged=${revisionChanged})`
            );
          })
          .catch((e) => console.error('[FRONTEND] Failed to refresh timeline changes:', e));
        refreshWatchdogState(activeProject.id, false).catch((e) =>
          console.error('[FRONTEND] Failed to refresh watchdog state on branch switch:', e)
        );
        reloadConductorPanel(activeProject.id, false).catch((e) =>
          console.error('[FRONTEND] Failed to refresh conductor state on branch switch:', e)
        );
      } else {
        debugLog('[FRONTEND][BRANCH][event] timeline refresh skipped (no branch/revision change)');
      }
    }).catch((e) => {
      console.error('[FRONTEND] Failed to register graph-updated listener:', e);
    });

    listen('file-touched', (event) => {
      const activeProject = getActiveProject();
      if (!activeProject) return;
      const path = normalizeTouchedPath(event.payload);
      if (!path) return;
      debugLog(`[FRONTEND] File touched: ${path}`);

      activeProject.heatCounter = (activeProject.heatCounter || 0) + 1;
      activeProject.heatmapData.set(path, {
        seq: activeProject.heatCounter,
        touchedAt: Date.now(),
      });
      activeProject.fileContentVersions.set(
        path,
        Number(activeProject.fileContentVersions.get(path) || 0) + 1
      );
      if (activeProject.heatmapData.size > 100) {
        let oldestKey = null;
        let oldestSeq = Infinity;
        for (const [key, value] of activeProject.heatmapData.entries()) {
          const seq = typeof value === 'number' ? value : Number(value?.seq ?? 0);
          if (seq < oldestSeq) {
            oldestSeq = seq;
            oldestKey = key;
          }
        }
        if (oldestKey) activeProject.heatmapData.delete(oldestKey);
      }

      if (activeProjectId === activeProject.id) {
        heatCounter = activeProject.heatCounter;
        // Force reactivity in Svelte 5 for Map
        heatmapData = new Map(activeProject.heatmapData);
        fileContentVersions = new Map(activeProject.fileContentVersions);
      }
    }).catch((e) => {
      console.error('[FRONTEND] Failed to register file-touched listener:', e);
    });
    debugLog(`[FRONTEND][TRACE] ${traceNow()} listeners registered`);
  });

</script>

<svelte:window onkeydown={handleAppHotkeys} />

<div class="eyeloss-page">
  <div class="eyeloss-project-tabs">
    {#each projectTabs as project}
      <button
        type="button"
        class="eyeloss-project-tab"
        class:eyeloss-project-tab--active={project.id === activeProjectId}
        title={project.path}
        onclick={() => activateProjectTab(project.id)}
      >
        <span class="eyeloss-project-tab__name">{project.label}</span>
        {#if project.currentBranch}
          <span class="eyeloss-project-tab__branch">{project.currentBranch}</span>
        {/if}
        {#if projectTabs.length > 1}
          <span
            class="eyeloss-project-tab__close"
            role="button"
            tabindex="0"
            onclick={(e) => closeProjectTab(project.id, e)}
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') closeProjectTab(project.id, e);
            }}
          >
            x
          </span>
        {/if}
      </button>
    {/each}
    <button
      type="button"
      class="eyeloss-project-tab eyeloss-project-tab--new"
      onclick={selectRepo}
      title="Open another repo in a new tab"
      aria-label="Open new project tab"
    >
      +
    </button>
  </div>

  <div class="eyeloss-controls">
    <div class="eyeloss-controls__left">
      <div class="eyeloss-controls__row">
        <span class="eyeloss-controls__repo">{repoPath}</span>
        <button
          type="button"
          class="btn btn-ghost"
          onclick={changeCurrentRepo}
          style="font-size: 0.6rem; padding: 2px 6px;"
          title="Change repo in current tab"
        >
          Change
        </button>
      </div>
      <div class="eyeloss-controls__row">
        <span class="eyeloss-controls__branch">branch: {currentBranch}</span>
      </div>
      {#if currentBranch === 'NO_VCS'}
        <div class="eyeloss-controls__row eyeloss-controls__warning">
          no vcs detected: file-view mode only, init git/jj for history
        </div>
      {/if}
      {#if (refreshing && !loading) || graphBuildProgress}
        <div class="eyeloss-controls__row eyeloss-controls__loading">
          {#if graphBuildProgress && graphBuildProgress.filesTotal > 0}
            building… {graphBuildProgress.filesProcessed}/{graphBuildProgress.filesTotal} files
          {:else}
            building graph…
          {/if}
        </div>
      {:else if repoActionMessage}
        <div class="eyeloss-controls__row eyeloss-controls__error">{repoActionMessage}</div>
      {/if}
    </div>

    <div class="eyeloss-controls__center">
      <form 
        onsubmit={(e) => { e.preventDefault(); setSince(since); showDropdown = false; }}
        style="position: relative;"
      >
        <div class="eyeloss-controls__graph-mode">
          <button
            type="button"
            class="btn"
            class:btn-primary={graphViewMode === 'investigation'}
            onclick={() => setGraphViewMode('investigation')}
          >
            Investigation
          </button>
          <button
            type="button"
            class="btn"
            class:btn-primary={graphViewMode === 'atlas'}
            onclick={() => setGraphViewMode('atlas')}
          >
            Atlas
          </button>
        </div>
        <label for="since-input">Since</label>
        <div style="position: relative; display: inline-block;">
          <input
            id="since-input"
            name="since"
            type="text"
            bind:value={since}
            onfocus={() => showDropdown = true}
            onblur={() => setTimeout(() => showDropdown = false, 200)}
            onkeydown={(e) => {
              if (e.key === 'ArrowDown') {
                e.preventDefault();
                highlightedIndex = Math.min(highlightedIndex + 1, filteredItems.length - 1);
              } else if (e.key === 'ArrowUp') {
                e.preventDefault();
                highlightedIndex = Math.max(highlightedIndex - 1, 0);
              } else if (e.key === 'Enter' && highlightedIndex >= 0) {
                e.preventDefault();
                setSince(filteredItems[highlightedIndex].id);
                showDropdown = false;
              } else if (e.key === 'Escape') {
                showDropdown = false;
              }
            }}
            placeholder="@ | @- | ancestors(@, 5)"
            class="input-mono"
            autocomplete="off"
          />
          {#if showDropdown && filteredItems.length > 0}
            <div class="eyeloss-dropdown">
              {#each filteredItems as item, i}
                <button
                  type="button"
                  class="eyeloss-dropdown__item"
                  class:eyeloss-dropdown__item--highlighted={i === highlightedIndex}
                  onclick={() => { setSince(item.id); showDropdown = false; }}
                >
                  <span class="eyeloss-dropdown__id">
                    {getShortName(item.id)} 
                    {#if item.type === 'bookmark'}<small style="opacity: 0.5; font-weight: normal; margin-left: 4px;">(bookmark)</small>{/if}
                  </span>
                  <span class="eyeloss-dropdown__desc">{item.description}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
        <button type="submit" class="btn btn-primary">Apply</button>
        {#if since}
          <button type="button" class="btn btn-ghost" onclick={() => { setSince(null); }}>Clear</button>
        {/if}
        {#if graphMeta?.approximateHistorical}
          <div class="eyeloss-controls__hint">approx historical view</div>
        {/if}
      </form>
    </div>

    <div class="eyeloss-controls__right">
      <button
        type="button"
        class="btn eyeloss-controls__view-btn"
        onclick={() => {
          activeMainView = activeMainView === 'graph' ? 'mission' : 'graph';
        }}
        title={activeMainView === 'graph' ? 'Open mission panel' : 'Back to graph'}
      >
        {activeMainView === 'graph' ? 'Mission' : 'Graph'}
      </button>
      <button type="button" class="btn theme-toggle" onclick={toggleTheme} title="Toggle theme">
        Theme: {theme === 'midnight' ? 'dark' : 'light'}
      </button>
    </div>
  </div>

  {#if activeMainView === 'graph'}
    <Sloposcope {graph}
      {graphMeta}
      {since}
      {changes}
      {bookmarks}
      {contentVersion}
      {fileContentVersions}
      {getFileDiff}
      {getFileSource}
      {saveFile}
      {theme}
      {heatmapData}
      {loadingMore}
      {hasMoreChanges}
      {missionState}
      onSelectSince={setSince}
      onLoadMoreChanges={loadMoreChanges}
      onProjectChange={selectRepo}
    />
  {:else}
    <section class="eyeloss-control-page">
      <div class="eyeloss-control-page__inner">
        <div class="eyeloss-control-page__header">
          <span>Mission</span>
          <button
            type="button"
            class="btn"
            onclick={() => {
              activeMainView = 'graph';
            }}
          >
            Back to graph
          </button>
        </div>

        <div class="eyeloss-primary-tabs" role="tablist" aria-label="Mission sections">
          <button type="button" class="btn eyeloss-primary-tab" class:btn-primary={activeSettingsTab === 'mission'} onclick={() => activeSettingsTab = 'mission'}>Mission</button>
          <button type="button" class="btn eyeloss-primary-tab" class:btn-primary={activeSettingsTab === 'runs'} onclick={() => activeSettingsTab = 'runs'}>Runs</button>
          <button type="button" class="btn eyeloss-primary-tab" class:btn-primary={activeSettingsTab === 'debug'} onclick={() => activeSettingsTab = 'debug'}>Debug</button>
        </div>

        {#if activeSettingsTab === 'mission'}
          <MissionPanel
            {missionState}
            {missionLoading}
            {missionSaving}
            {missionPlanning}
            {missionCheckRunning}
            {missionFixtureRunning}
            {missionError}
            bind:missionTitle
            bind:missionBrief
            bind:missionValidationCommand
            bind:missionIssueKey
            bind:missionIssueUrl
            bind:missionPlannerProfileId
            bind:missionPlannerNotes
            bind:missionPlannerPromptOverride
            onSaveMission={saveMissionInput}
            onPlanMission={planMission}
            onRunCheck={runMissionCheck}
            onReload={() => refreshMissionState(activeProjectId)}
          />
        {:else if activeSettingsTab === 'runs'}
          <MissionRunsPanel
            {missionState}
            {missionLoading}
            {missionError}
            {missionFixtureRunning}
            {missionCodeWorkerRunning}
            {missionReviewRunning}
            {missionReviewAction}
            bind:selectedCodeWorkerProfileId
            onRunCodeWorker={runCodeWorker}
            onRunNextCodeWorker={runNextCodeWorker}
            onRunFixtureWorker={runFixtureWorker}
            onSweepStaleSessions={sweepStaleSessions}
            onAcceptRun={(sessionId) => runMissionReviewAction('accept', sessionId)}
            onRejectRun={(sessionId) => runMissionReviewAction('reject', sessionId)}
            onApplyRun={(sessionId) => runMissionReviewAction('apply', sessionId)}
            onGateRun={(sessionId) => runMissionReviewAction('gate', sessionId)}
            onReload={() => refreshMissionState(activeProjectId)}
          />
        {:else}
          <div class="eyeloss-debug-stack">
            <section class="eyeloss-control-card">
              <header class="eyeloss-control-card__header">
                <div>
                  <div class="eyeloss-control-card__eyebrow">Debug</div>
                  <h2 class="eyeloss-control-card__title">Enterprise Controls</h2>
                </div>
              </header>

              <div class="eyeloss-settings-tabs eyeloss-settings-tabs--debug" role="tablist" aria-label="Debug sections">
                <button type="button" class="btn eyeloss-settings-tab" class:btn-primary={activeDebugTab === 'issue'} onclick={() => activeDebugTab = 'issue'}>Issue</button>
                <button type="button" class="btn eyeloss-settings-tab" class:btn-primary={activeDebugTab === 'config'} onclick={() => activeDebugTab = 'config'}>Config</button>
                <button type="button" class="btn eyeloss-settings-tab" class:btn-primary={activeDebugTab === 'profiles'} onclick={() => activeDebugTab = 'profiles'}>Profiles</button>
                <button type="button" class="btn eyeloss-settings-tab" class:btn-primary={activeDebugTab === 'runs'} onclick={() => activeDebugTab = 'runs'}>Runs</button>
              </div>

              {#if activeDebugTab === 'issue'}
                <IssuePanel
                  {watchdogState}
                  {watchdogTraces}
                  {watchdogStatusClass}
                  {watchdogLoading}
                  {watchdogRunning}
                  {watchdogError}
                  {clusterRefreshLoading}
                  {graphMeta}
                  {formatWatchdogStatus}
                  bind:watchdogIssueId
                  bind:watchdogIssueTitle
                  bind:watchdogIssueIdentifier
                  bind:watchdogIssueUrl
                  bind:watchdogIssueSummary
                  bind:watchdogIssueLabels
                  bind:watchdogIssuePriority
                  bind:watchdogAcceptanceCriteria
                  bind:watchdogNonGoals
                  bind:watchdogValidationDetails
                  onSaveIssueContext={saveWatchdogIssueContext}
                  onClearIssueContext={clearWatchdogIssueContext}
                  onRunCheck={runWatchdogCheck}
                  onRunValidationCommand={runWatchdogValidationCommand}
                  onRefreshClusters={refreshGraphClusters}
                  onReload={() => refreshWatchdogState(activeProjectId, true)}
                />
              {:else if activeDebugTab === 'config'}
                <ConfigPanel
                  {watchdogState}
                  {watchdogError}
                  {watchdogConfigSaving}
                  {conductorWorkflowLoad}
                  {conductorLoading}
                  {conductorWorkflowLoading}
                  {conductorSettingsSaving}
                  {conductorError}
                  bind:watchdogConfigEnabled
                  bind:watchdogConfigTriggerMode
                  bind:watchdogConfigAlertThreshold
                  bind:watchdogConfigRequireActiveIssue
                  bind:watchdogConfigMaxFiles
                  bind:watchdogConfigMaxDiffLines
                  bind:watchdogConfigDedupeWindow
                  bind:watchdogConfigChecks
                  bind:conductorWorkflowProvider
                  bind:conductorWorkflowProviderModel
                  bind:conductorWorkflowTimeout
                  bind:conductorWorkflowTraceRetention
                  bind:conductorWorkflowStrictMode
                  onSaveWatchdogConfig={saveWatchdogConfig}
                  onSaveSettings={saveConductorWorkflowSettings}
                  onRefresh={() => reloadConductorPanel(activeProjectId, false)}
                />
              {:else if activeDebugTab === 'profiles'}
                <AgentProfilesPanel
                  profiles={missionState?.profiles || []}
                  profileSaving={missionProfileSaving}
                  profileDeletingId={missionProfileDeletingId}
                  profileError={missionProfileError}
                  bind:selectedCodeWorkerProfileId
                  bind:profileLabel={missionProfileLabel}
                  bind:profileProvider={missionProfileProvider}
                  bind:profileModel={missionProfileModel}
                  bind:profileCommand={missionProfileCommand}
                  bind:profileArgs={missionProfileArgs}
                  bind:profileEnvJson={missionProfileEnvJson}
                  bind:profileCapabilities={missionProfileCapabilities}
                  bind:profileTransportKind={missionProfileTransportKind}
                  bind:profileActive={missionProfileActive}
                  profileEnvError={missionProfileEnvError}
                  profileProviderHint={missionProfileProviderHint}
                  onApplyTemplate={applyMissionProfileTemplate}
                  onSaveProfile={saveMissionProfile}
                  onDeleteProfile={deleteMissionProfile}
                  onUseProfile={(profileId) => setSelectedCodeWorkerProfileId(profileId)}
                  onClearForm={clearMissionProfileForm}
                  onReload={() => refreshMissionState(activeProjectId)}
                />
              {:else}
                <RunsPanel
                  {conductorStatus}
                  {conductorLoading}
                  {conductorWorkflowLoading}
                  {conductorRunning}
                  {conductorError}
                  {conductorLaunchResult}
                  {conductorLaunchLoading}
                  {conductorDispatchLoading}
                  {conductorIngestLoading}
                  bind:conductorTaskId
                  bind:conductorMaxRetries
                  bind:conductorTraceLimit
                  bind:conductorIngestTaskId
                  bind:conductorIngestRawEvent
                  onEnqueue={enqueueConductorTask}
                  onTick={tickConductorNow}
                  onRefresh={() => reloadConductorPanel(activeProjectId, false)}
                  onLaunch={prepareConductorLaunch}
                  onDispatch={dispatchConductorLaunch}
                  onIngestEvent={ingestConductorRunnerEvent}
                />
              {/if}
            </section>
          </div>
        {/if}
      </div>
    </section>
  {/if}

  {#if loading || refreshing || loadingMore}
    <div class={loading ? 'eyeloss-splash' : 'eyeloss-refresh-overlay'} transition:fade={{ duration: 120 }}>
      <div class="eyeloss-splash__fortune">{currentFortune}</div>
    </div>
  {/if}
</div>
