function cloneJson(value) {
  return value == null ? value : JSON.parse(JSON.stringify(value));
}

const MOCK_CURRENT_REPO_PATH_KEY = 'sloposcope.mockCurrentRepoPath';
const MOCK_MISSION_STATE_BY_REPO_KEY = 'sloposcope.mockMissionStateByRepo';

function canUseLocalStorage() {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined';
}

function readMockStorage(key, fallback) {
  if (!canUseLocalStorage()) return fallback;
  try {
    const raw = window.localStorage.getItem(key);
    if (!raw) return fallback;
    return JSON.parse(raw);
  } catch (_error) {
    return fallback;
  }
}

function writeMockStorage(key, value) {
  if (!canUseLocalStorage()) return;
  try {
    window.localStorage.setItem(key, JSON.stringify(value));
  } catch (_error) {
    // ignore storage failures in mock mode
  }
}

function createMockGraph() {
  return {
    nodes: [
      {
        id: 'MockModule',
        type: 'module',
        file: 'mock.ex',
        line_count: 42,
        change_status: 'neighbor',
        functions: [],
      },
      {
        id: 'MockHelper',
        type: 'module',
        file: 'mock_helper.ex',
        line_count: 12,
        change_status: 'modified',
        functions: [],
      },
    ],
    edges: [{ source: 'MockHelper', target: 'MockModule', type: 'call' }],
  };
}

function createMockGraphView(request = {}) {
  const mode = String(request?.mode || 'investigation');
  const seed = request?.seed || { kind: 'working_copy' };
  const graph = createMockGraph();
  return {
    graph,
    mode,
    aggregate: String(request?.aggregate || 'file'),
    approximateHistorical: seed?.kind === 'commit',
    seedLabel: seed?.kind === 'commit' ? `commit:${seed?.revset || '@'}` : 'working_copy',
    nodeProvenance:
      mode === 'investigation'
        ? [
            { nodeId: 'MockHelper', reason: 'changed file' },
            { nodeId: 'MockModule', reason: '1-hop dependency' },
          ]
        : [],
    canonicalEntityCount: 2,
    canonicalEdgeCount: 1,
    clusteringMode: 'heuristic',
    clusterAssignments: [],
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

function normalizeMissionInput(input = {}) {
  return {
    title: String(input?.title || '').trim(),
    brief: String(input?.brief || '').trim(),
    validationCommand: String(input?.validationCommand || input?.validation_command || '').trim(),
    issueKey: String(input?.issueKey || input?.issue_key || '').trim(),
    issueUrl: String(input?.issueUrl || input?.issue_url || '').trim(),
    plannerProfileId: String(input?.plannerProfileId || input?.planner_profile_id || '').trim(),
    plannerNotes: String(input?.plannerNotes || input?.planner_notes || '').trim(),
    plannerPromptOverride: String(input?.plannerPromptOverride || input?.planner_prompt_override || '').trim(),
  };
}

function deriveMissionId(input = {}, fallbackId = '') {
  const explicit = String(input?.missionId || input?.mission_id || '').trim();
  if (explicit) return explicit;
  const issueKey = String(input?.issueKey || input?.issue_key || '').trim();
  if (issueKey) return issueKey.toLowerCase().replaceAll(/[^a-z0-9]+/g, '-').replaceAll(/^-+|-+$/g, '');
  const title = String(input?.title || '').trim().toLowerCase();
  const slug = title.replaceAll(/[^a-z0-9]+/g, '-').replaceAll(/^-+|-+$/g, '');
  return slug || fallbackId || 'mission';
}

function missionSummaryFromInput(input = {}) {
  const title = String(input?.title || '').trim();
  const brief = String(input?.brief || '').trim();
  if (title && brief) return `${title} - ${brief}`;
  return title || brief || 'Mission not planned yet.';
}

function normalizeScopePath(path) {
  return String(path || '')
    .trim()
    .replaceAll('\\', '/')
    .replace(/^\.\/+/, '')
    .replace(/^\/+/, '');
}

function parseObjectiveScopeTag(line) {
  const trimmed = String(line || '').trim();
  if (!trimmed.startsWith('[scope:')) return { scopePaths: [], prompt: trimmed };
  const endIndex = trimmed.indexOf(']');
  if (endIndex < 0) return { scopePaths: [], prompt: trimmed };
  const scopePaths = trimmed
    .slice(7, endIndex)
    .split(',')
    .map(normalizeScopePath)
    .filter(Boolean);
  return {
    scopePaths,
    prompt: trimmed.slice(endIndex + 1).trim(),
  };
}

function planMissionObjectives(missionInput = {}) {
  const lines = String(missionInput?.brief || '')
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean);
  const sourceLines = lines.length ? lines : [String(missionInput?.title || '').trim()].filter(Boolean);
  const planned = [];
  for (let index = 0; index < sourceLines.length; index += 1) {
    const parsed = parseObjectiveScopeTag(sourceLines[index]);
    let prompt = parsed.prompt;
    if (!prompt && index + 1 < sourceLines.length) {
      prompt = String(sourceLines[index + 1] || '').trim();
      if (prompt) index += 1;
    }
    prompt = prompt || `Complete mission slice ${planned.length + 1}.`;
    planned.push({
      id: `objective-${planned.length + 1}`,
      title: prompt,
      prompt,
      expectedValidation: String(missionInput?.validationCommand || '').trim(),
      scope: { paths: parsed.scopePaths },
      state: 'pending',
      latestSessionId: null,
    });
  }
  return planned;
}

function scopePathsOverlap(left = [], right = []) {
  const normalizedLeft = left.map(normalizeScopePath).filter(Boolean);
  const normalizedRight = right.map(normalizeScopePath).filter(Boolean);
  if (!normalizedLeft.length || !normalizedRight.length) return false;
  return normalizedLeft.some((lhs) => normalizedRight.some((rhs) =>
    lhs === rhs || lhs.startsWith(`${rhs}/`) || rhs.startsWith(`${lhs}/`)
  ));
}

function latestSessionForObjective(state, objectiveId) {
  const sessions = Array.isArray(state?.sessions) ? [...state.sessions].reverse() : [];
  return sessions.find((entry) => entry?.objectiveId === objectiveId) || null;
}

function profileHasCapability(profile, capability) {
  const wanted = String(capability || '').trim().toLowerCase();
  if (!wanted) return false;
  const capabilities = Array.isArray(profile?.capabilities) ? profile.capabilities : [];
  return capabilities.some((entry) => String(entry || '').trim().toLowerCase() === wanted);
}

function findPlannerProfile(state = {}) {
  const profiles = Array.isArray(state?.profiles) ? state.profiles : [];
  return (
    profiles.find((profile) => profile?.active && profileHasCapability(profile, 'can_plan'))
    || profiles.find((profile) => profileHasCapability(profile, 'can_plan'))
    || null
  );
}

function findPlannerProfileById(state = {}, profileId = '') {
  const normalized = String(profileId || '').trim();
  if (!normalized) return null;
  const profiles = Array.isArray(state?.profiles) ? state.profiles : [];
  return profiles.find((profile) => profile?.id === normalized) || null;
}

function deriveScheduler(state = {}) {
  const objectives = Array.isArray(state?.objectives) ? state.objectives : [];
  const summary = {
    readyCount: 0,
    blockedCount: 0,
    activeCount: 0,
    staleSessionCount: 0,
    nextObjectiveId: null,
  };
  const byObjectiveId = new Map();
  const reservedScopes = [];

  for (const objective of objectives) {
    const scopePaths = Array.isArray(objective?.scope?.paths) ? objective.scope.paths : [];
    const session = latestSessionForObjective(state, objective?.id);
    const runtimeStatus = String(session?.runtimeStatus || session?.state || '').trim().toLowerCase();
    let schedulerState = 'ready';
    let schedulerReason = '';

    if (objective?.state === 'done') {
      schedulerState = 'done';
    } else if (objective?.state === 'failed') {
      schedulerState = 'failed';
    } else if (objective?.state === 'claimed') {
      if (runtimeStatus === 'stuck') {
        schedulerState = 'blocked_stale';
        schedulerReason = 'stale claimed session blocks objective';
        summary.staleSessionCount += 1;
        summary.blockedCount += 1;
      } else {
        schedulerState = 'active';
        summary.activeCount += 1;
        if (scopePaths.length) reservedScopes.push(scopePaths);
      }
    } else {
      if (runtimeStatus === 'stuck') {
        schedulerState = 'blocked_stale';
        schedulerReason = 'stale session for objective needs sweep';
        summary.staleSessionCount += 1;
        summary.blockedCount += 1;
      } else if (scopePaths.length && reservedScopes.some((reserved) => scopePathsOverlap(scopePaths, reserved))) {
        schedulerState = 'blocked_scope';
        schedulerReason = `waiting for scope ${scopePaths[0]}`;
        summary.blockedCount += 1;
      } else {
        schedulerState = 'ready';
        summary.readyCount += 1;
        if (!summary.nextObjectiveId) summary.nextObjectiveId = objective?.id || null;
        if (scopePaths.length) reservedScopes.push(scopePaths);
      }
    }

    byObjectiveId.set(objective?.id, { schedulerState, schedulerReason });
  }

  return { summary, byObjectiveId };
}

function createMissionEvent(kind, summary, payload = null) {
  return {
    at: new Date().toISOString(),
    kind,
    summary,
    payload,
  };
}

function withComputedMissionState(state = {}) {
  const scheduler = deriveScheduler(state);
  const objectives = (Array.isArray(state?.objectives) ? state.objectives : []).map((objective) => {
    const schedule = scheduler.byObjectiveId.get(objective?.id) || null;
    return {
      ...objective,
      schedulerState: schedule?.schedulerState || 'ready',
      schedulerReason: schedule?.schedulerReason || '',
    };
  });
  const sessions = (Array.isArray(state?.sessions) ? state.sessions : []).map((session) => {
    const schedule = scheduler.byObjectiveId.get(session?.objectiveId) || null;
    return {
      ...session,
      schedulerState: schedule?.schedulerState || '',
      schedulerReason: schedule?.schedulerReason || '',
    };
  });
  return {
    ...state,
    objectives,
    sessions,
    scheduler: scheduler.summary,
  };
}

function deriveProfileId(input = {}, fallbackId = '') {
  const explicit = String(input?.id || '').trim();
  if (explicit) return explicit.toLowerCase().replaceAll(/[^a-z0-9]+/g, '-').replaceAll(/^-+|-+$/g, '');
  const label = String(input?.label || '').trim().toLowerCase();
  const slug = label.replaceAll(/[^a-z0-9]+/g, '-').replaceAll(/^-+|-+$/g, '');
  return slug || fallbackId || 'agent-profile';
}

function cloneMissionState(state, { omitLatestEvent = false } = {}) {
  const computed = withComputedMissionState(state);
  const eventLedger = Array.isArray(state?.eventLedger) ? state.eventLedger : [];
  const visibleEvents = omitLatestEvent && eventLedger.length > 0 ? eventLedger.slice(0, -1) : eventLedger;
  const mission = {
    missionId: String(computed?.missionId || ''),
    title: String(computed?.title || ''),
    brief: String(computed?.brief || ''),
    validationCommand: String(computed?.validationCommand || ''),
    issueKey: String(computed?.issueKey || ''),
    issueUrl: String(computed?.issueUrl || ''),
    plannerProfileId: String(computed?.plannerProfileId || ''),
    plannerProfileLabel: String(computed?.plannerProfileLabel || ''),
    plannerNotes: String(computed?.plannerNotes || ''),
    plannerPromptOverride: String(computed?.plannerPromptOverride || ''),
    plannerSummary: String(computed?.plannerSummary || 'viewer-first planner ready'),
    plannerStatus: String(computed?.plannerStatus || 'idle'),
    plannerMode: String(computed?.plannerMode || 'viewer_first'),
    summary: String(computed?.summary || ''),
    status: String(computed?.status || 'idle'),
    viewerProfileId: String(computed?.viewerProfileId || ''),
    viewerSessionId: String(computed?.viewerSessionId || ''),
    reviewState: String(computed?.reviewState || 'pending'),
    applyState: String(computed?.applyState || 'not_applied'),
    gateState: String(computed?.gateState || 'open'),
    profiles: cloneJson(computed?.profiles || []),
    scheduler: cloneJson(computed?.scheduler || {}),
    lastAction: String(computed?.lastAction || ''),
    lastActionAt: computed?.lastActionAt || null,
    lastSavedAt: computed?.lastSavedAt || null,
    lastPlannedAt: computed?.lastPlannedAt || null,
    lastCheckedAt: computed?.lastCheckedAt || null,
    lastRunAt: computed?.lastRunAt || null,
    lastCheckResult: cloneJson(computed?.lastCheckResult),
  };

  return {
    mission,
    ...mission,
    objectives: cloneJson(computed?.objectives || []),
    sessions: cloneJson(computed?.sessions || []),
    verifications: cloneJson(computed?.verifications || state?.verifications || []),
    events: cloneJson(visibleEvents),
    eventLedger: cloneJson(visibleEvents),
    robotTargets: cloneJson(computed?.robotTargets || state?.robotTargets || []),
    scheduler: cloneJson(computed?.scheduler || {}),
  };
}

export function createBackendBridge({
  debugLog,
  mockChanges,
  createDefaultWatchdogState,
  createDefaultConductorStatus,
  createDefaultConductorWorkflowLoad,
  createDefaultMissionState: createDefaultMissionStateFromHost,
}) {
  let mockWatchdogState = createDefaultWatchdogState();
  let mockWatchdogTraces = [];
  let mockConductorTasks = [];
  let mockConductorTraces = [];
  let mockConductorStatus = createDefaultConductorStatus(20);
  const createMissionState = createDefaultMissionStateFromHost || createDefaultMissionState;
  let mockCurrentRepoPath = readMockStorage(MOCK_CURRENT_REPO_PATH_KEY, '/path/to/despair') || '/path/to/despair';
  const mockMissionStatesByRepo = new Map(
    Object.entries(readMockStorage(MOCK_MISSION_STATE_BY_REPO_KEY, {}) || {}).map(([repoPath, state]) => [
      repoPath,
      cloneJson(state) || createMissionState(),
    ]),
  );
  let mockConductorWorkflowLoad = {
    ...createDefaultConductorWorkflowLoad(),
    path: 'WORKFLOW.md',
    exists: true,
    loaded: true,
    contract: {
      frontMatter: {
        schemaVersion: 1,
        workflowId: 'watchdog_focus',
        provider: 'openai',
        providerModel: 'gpt-4.1-mini',
        providerStrictMode: false,
        traceRetention: 30,
        title: 'Sloposcope Watchdog Focus Check',
        summary: 'Evaluate working-copy drift against active issue and shared repo rules.',
        maxRetries: 2,
        timeoutSeconds: 900,
        requiredChecks: ['cargo_test', 'ui_build'],
      },
      promptTemplate: 'You are a watchdog evaluator.',
    },
  };

  function persistMockMissionStates() {
    const serialized = {};
    for (const [repoPath, state] of mockMissionStatesByRepo.entries()) {
      serialized[repoPath] = state;
    }
    writeMockStorage(MOCK_MISSION_STATE_BY_REPO_KEY, serialized);
  }

  function ensureMissionState(repoPath = mockCurrentRepoPath) {
    const normalizedRepoPath = String(repoPath || '/path/to/despair');
    if (!mockMissionStatesByRepo.has(normalizedRepoPath)) {
      mockMissionStatesByRepo.set(normalizedRepoPath, createMissionState());
      persistMockMissionStates();
    }
    return mockMissionStatesByRepo.get(normalizedRepoPath);
  }

  function replaceMissionState(repoPath, nextState) {
    const normalizedRepoPath = String(repoPath || '/path/to/despair');
    const cloned = cloneJson(nextState || createMissionState());
    mockMissionStatesByRepo.set(normalizedRepoPath, cloned);
    persistMockMissionStates();
    return cloned;
  }

  function mutateMissionState(repoPath, updater) {
    const normalizedRepoPath = String(repoPath || '/path/to/despair');
    const current = ensureMissionState(normalizedRepoPath);
    const next = updater(cloneJson(current) || createMissionState());
    return replaceMissionState(normalizedRepoPath, next);
  }

  function pushMockConductorTrace(event, taskId = null, detail = null) {
    mockConductorTraces = [
      ...mockConductorTraces,
      {
        at: new Date().toISOString(),
        event,
        taskId,
        detail,
      },
    ].slice(-500);
  }

  function rebuildMockConductorStatus(traceLimit = 20) {
    const normalizedLimit = Math.max(1, Math.min(100, Number(traceLimit || 20)));
    const status = {
      generatedAt: new Date().toISOString(),
      config: {
        maxConcurrentRuns: 2,
        retryBaseSeconds: 15,
        retryMaxSeconds: 300,
      },
      providerFailures:
        mockConductorStatus.providerFailures
        || createDefaultConductorStatus(normalizedLimit).providerFailures,
      tasksTotal: mockConductorTasks.length,
      tasksClaimed: mockConductorTasks.filter((task) => task.state === 'claimed').length,
      tasksRunning: mockConductorTasks.filter((task) => task.state === 'running').length,
      tasksRetryScheduled: mockConductorTasks.filter((task) => task.state === 'retry_scheduled').length,
      tasksReconciled: mockConductorTasks.filter((task) => task.state === 'reconciled').length,
      tracesTotal: mockConductorTraces.length,
      lastTickAt: mockConductorStatus.lastTickAt || null,
      recentTraces: [...mockConductorTraces].slice(-normalizedLimit).reverse(),
      issueRuns: mockConductorTasks.map((task) => ({
        taskId: task.taskId,
        state: task.state,
        runAttempt: Number(task.runAttempt || 0),
        retryCount: Number(task.retryCount || 0),
        activeSessionId: task.activeSessionId || null,
        lastError: task.lastError || null,
      })),
      _traceLimit: normalizedLimit,
    };
    mockConductorStatus = status;
    return status;
  }

  const invoke = async (cmd, args) => {
    if (typeof window !== 'undefined' && window.__TAURI_INTERNALS__) {
      const { invoke: tauriInvoke } = await import('@tauri-apps/api/core');
      debugLog(`[FRONTEND] Invoking ${cmd}`, args);
      return tauriInvoke(cmd, args);
    }
    if (cmd === 'check_watchdog_before_push') cmd = 'check_watchdog_focus';
    debugLog('[MOCK] invoke', cmd, args);

    if (cmd === 'get_graph_view') {
      return createMockGraphView(args?.request);
    }

    if (cmd === 'get_graph') {
      return createMockGraphView({
        mode: 'investigation',
        seed: args?.since ? { kind: 'commit', revset: args.since } : { kind: 'working_copy' },
        aggregate: 'file',
      }).graph;
    }

    if (cmd === 'get_changes') {
      const limit = Math.max(1, Number(args?.limit || 20));
      const beforeId = args?.before_id;
      if (!beforeId) return mockChanges.slice(0, limit);
      const cursorIndex = mockChanges.findIndex((c) => c.id === beforeId);
      if (cursorIndex < 0) return [];
      return mockChanges.slice(cursorIndex + 1, cursorIndex + 1 + limit);
    }

    if (cmd === 'get_bookmarks') return [{ name: 'main', id: 'c0ffee11' }];
    if (cmd === 'get_current_branch') return 'main';
    if (cmd === 'get_repo_path') {
      ensureMissionState(mockCurrentRepoPath);
      return mockCurrentRepoPath;
    }
    if (cmd === 'select_repo' || cmd === 'pick_folder') return '/new/path/to/despair';
    if (cmd === 'set_repo_path') {
      mockCurrentRepoPath = String(args?.path || '/path/to/despair');
      writeMockStorage(MOCK_CURRENT_REPO_PATH_KEY, mockCurrentRepoPath);
      ensureMissionState(mockCurrentRepoPath);
      return mockCurrentRepoPath;
    }

    if (cmd === 'get_watchdog_state') return cloneJson(mockWatchdogState);
    if (cmd === 'get_mission_state') return cloneJson(cloneMissionState(ensureMissionState()));
    if (cmd === 'list_agent_profiles') {
      const state = ensureMissionState();
      return { profiles: cloneJson(state?.profiles || []) };
    }

    if (cmd === 'save_agent_profile') {
      const rawProfile = cloneJson(args?.profile || {});
      const now = new Date().toISOString();
      const next = mutateMissionState(mockCurrentRepoPath, (state) => {
        const currentProfiles = Array.isArray(state.profiles) ? [...state.profiles] : [];
        const profileId = deriveProfileId(rawProfile, `agent-profile-${currentProfiles.length + 1}`);
        const existingIndex = currentProfiles.findIndex((entry) => entry?.id === profileId);
        const existing = existingIndex >= 0 ? currentProfiles[existingIndex] : null;
        const profile = {
          id: profileId,
          label: String(rawProfile?.label || '').trim(),
          provider: String(rawProfile?.provider || '').trim(),
          model: String(rawProfile?.model || '').trim() || null,
          command: String(rawProfile?.command || '').trim(),
          args: Array.isArray(rawProfile?.args) ? rawProfile.args : [],
          env:
            rawProfile?.env && typeof rawProfile.env === 'object' && !Array.isArray(rawProfile.env)
              ? rawProfile.env
              : {},
          capabilities: Array.isArray(rawProfile?.capabilities) ? rawProfile.capabilities : [],
          transportKind: String(rawProfile?.transportKind || rawProfile?.transport_kind || existing?.transportKind || 'pty'),
          active: Boolean(rawProfile?.active),
          createdAt: existing?.createdAt || now,
          updatedAt: now,
        };
        if (existingIndex >= 0) currentProfiles.splice(existingIndex, 1, profile);
        else currentProfiles.push(profile);
        return {
          ...state,
          profiles: currentProfiles,
        };
      });
      next.eventLedger = [...(next.eventLedger || []), createMissionEvent('profile_save', `Saved profile ${deriveProfileId(rawProfile)}.`, {
        profileId: deriveProfileId(rawProfile),
      })];
      next.events = next.eventLedger;
      replaceMissionState(mockCurrentRepoPath, next);
      return { profiles: cloneJson(next.profiles || []) };
    }

    if (cmd === 'delete_agent_profile') {
      const profileId = String(args?.profileId || '').trim();
      const next = mutateMissionState(mockCurrentRepoPath, (state) => ({
        ...state,
        profiles: (state.profiles || []).filter((entry) => entry?.id !== profileId),
      }));
      next.eventLedger = [...(next.eventLedger || []), createMissionEvent('profile_delete', `Deleted profile ${profileId}.`, {
        profileId,
      })];
      next.events = next.eventLedger;
      replaceMissionState(mockCurrentRepoPath, next);
      return { profiles: cloneJson(next.profiles || []) };
    }

    if (cmd === 'save_mission_input') {
      const missionInput = normalizeMissionInput(args?.missionInput || args?.mission || args || {});
      const now = new Date().toISOString();
      const next = mutateMissionState(mockCurrentRepoPath, (state) => {
        const missionId = deriveMissionId(missionInput, state.missionId);
        const plannerProfile = findPlannerProfileById(state, missionInput.plannerProfileId);
        const plannerMode = missionInput.plannerProfileId ? 'profile' : 'viewer_first';
        const plannerSummary = missionInput.plannerProfileId
          ? `Planner profile ready: ${plannerProfile?.label || missionInput.plannerProfileId}`
          : 'viewer-first planner ready';
        return {
          ...state,
          missionId,
          title: missionInput.title,
          brief: missionInput.brief,
          validationCommand: missionInput.validationCommand,
          issueKey: missionInput.issueKey,
          issueUrl: missionInput.issueUrl,
          plannerProfileId: missionInput.plannerProfileId,
          plannerProfileLabel: plannerProfile?.label || '',
          plannerNotes: missionInput.plannerNotes,
          plannerPromptOverride: missionInput.plannerPromptOverride,
          plannerSummary,
          plannerStatus: state.plannerStatus === 'planned' ? 'planned' : 'ready',
          plannerMode,
          summary: missionSummaryFromInput(missionInput),
          status: state.status === 'planned' ? 'planned' : 'ready',
          lastSavedAt: now,
          mission: {
            ...(state.mission || {}),
            missionId,
            title: missionInput.title,
            brief: missionInput.brief,
            validationCommand: missionInput.validationCommand,
            issueKey: missionInput.issueKey,
            issueUrl: missionInput.issueUrl,
            plannerProfileId: missionInput.plannerProfileId,
            plannerProfileLabel: plannerProfile?.label || '',
            plannerNotes: missionInput.plannerNotes,
            plannerPromptOverride: missionInput.plannerPromptOverride,
            plannerSummary,
            plannerStatus: state.plannerStatus === 'planned' ? 'planned' : 'ready',
            plannerMode,
            summary: missionSummaryFromInput(missionInput),
            status: state.status === 'planned' ? 'planned' : 'ready',
            lastSavedAt: now,
          },
        };
      });
      next.eventLedger = [...(next.eventLedger || []), createMissionEvent('save', 'Mission input saved.', { missionInput })];
      next.events = next.eventLedger;
      replaceMissionState(mockCurrentRepoPath, next);
      return cloneJson(cloneMissionState(next, { omitLatestEvent: true }));
    }

    if (cmd === 'plan_mission') {
      const missionInput = normalizeMissionInput(args?.missionInput || args?.mission || args || {});
      if (!missionInput.plannerProfileId && (missionInput.plannerNotes || missionInput.plannerPromptOverride)) {
        throw new Error('Select planner profile for planner notes or override prompt.');
      }
      const now = new Date().toISOString();
      const next = mutateMissionState(mockCurrentRepoPath, (state) => {
        const missionId = deriveMissionId(missionInput, state.missionId);
        const plannerProfile = findPlannerProfileById(state, missionInput.plannerProfileId) || findPlannerProfile(state);
        const objectives = planMissionObjectives(missionInput);
        const plannerMode = missionInput.plannerProfileId ? 'profile' : 'viewer_first';
        const plannerSummary = missionInput.plannerProfileId
          ? `Mission planned with ${plannerProfile?.label || missionInput.plannerProfileId}.`
          : 'Mission planned with default planner prompt.';
        return {
          ...state,
          missionId,
          title: missionInput.title,
          brief: missionInput.brief,
          validationCommand: missionInput.validationCommand,
          issueKey: missionInput.issueKey,
          issueUrl: missionInput.issueUrl,
          plannerProfileId: plannerProfile?.id || missionInput.plannerProfileId || '',
          plannerProfileLabel: plannerProfile?.label || '',
          plannerNotes: missionInput.plannerNotes,
          plannerPromptOverride: missionInput.plannerPromptOverride,
          plannerSummary,
          plannerStatus: 'planned',
          plannerMode,
          summary: missionSummaryFromInput(missionInput),
          status: 'planned',
          viewerProfileId: '',
          viewerSessionId: '',
          reviewState: 'pending',
          applyState: 'not_applied',
          gateState: 'open',
          lastAction: 'plan',
          lastActionAt: now,
          viewerProfileId: plannerProfile?.id || '',
          objectives,
          lastPlannedAt: now,
          mission: {
            ...(state.mission || {}),
            missionId,
            title: missionInput.title,
            brief: missionInput.brief,
            validationCommand: missionInput.validationCommand,
            issueKey: missionInput.issueKey,
            issueUrl: missionInput.issueUrl,
            plannerProfileId: plannerProfile?.id || missionInput.plannerProfileId || '',
            plannerNotes: missionInput.plannerNotes,
            plannerPromptOverride: missionInput.plannerPromptOverride,
            plannerSummary,
            plannerStatus: 'planned',
            plannerMode,
            summary: missionSummaryFromInput(missionInput),
            status: 'planned',
            viewerProfileId: '',
            viewerSessionId: '',
            reviewState: 'pending',
            applyState: 'not_applied',
            gateState: 'open',
            lastAction: 'plan',
            lastActionAt: now,
            lastPlannedAt: now,
          },
        };
      });
      const plannerProfile = findPlannerProfile(next);
      const plannerLabel = plannerProfile?.provider || 'fixture';
      next.eventLedger = [...(next.eventLedger || []), createMissionEvent('plan', `Planned ${next.objectives.length || 0} objective(s) with ${plannerLabel}.`, {
        objectiveIds: next.objectives.map((objective) => objective.id),
        planner: plannerLabel,
      })];
      next.events = next.eventLedger;
      replaceMissionState(mockCurrentRepoPath, next);
      return cloneJson(cloneMissionState(next));
    }

    if (cmd === 'run_fixture_worker') {
      const now = new Date().toISOString();
      const requestedObjectiveId = String(args?.objectiveId || '').trim();
      const next = mutateMissionState(mockCurrentRepoPath, (state) => {
        const sessionId = `fixture-session-${(state.sessions?.length || 0) + 1}`;
        const verificationId = `verification-${(state.verifications?.length || 0) + 1}`;
        const objectives = Array.isArray(state.objectives)
          ? state.objectives.map((objective, index) => {
              const matchesRequested = requestedObjectiveId ? objective?.id === requestedObjectiveId : index === 0;
              if (!matchesRequested) return objective;
              return {
                ...objective,
                state: 'done',
                latestSessionId: sessionId,
              };
            })
          : [];
        const objective = objectives.find((entry) => entry?.latestSessionId === sessionId) || null;
        const verification = {
          verificationId,
          sessionId,
          objectiveId: objective?.id || null,
          objectiveTitle: objective?.title || null,
          profileId: 'fixture_worker',
          kind: 'fixture_worker',
          status: 'passed',
          summary: objective ? `Fixture worker completed ${objective.id}.` : 'Fixture worker ran.',
          checkedAt: now,
          command: state.validationCommand || null,
          reviewState: 'pending',
          applyState: 'not_applied',
          gateState: 'open',
        };
        const session = {
          sessionId,
          objectiveId: objective?.id || null,
          objectiveTitle: objective?.title || null,
          profileId: 'fixture_worker',
          workerKind: 'fixture_worker',
          state: 'completed',
          result: 'passed',
          startedAt: now,
          finishedAt: now,
          verificationId,
          latestEvent: objective ? 'completed' : 'idle',
          command: state.validationCommand || null,
          reviewState: 'pending',
          applyState: 'not_applied',
          gateState: 'open',
        };
        return {
          ...state,
          status: 'running',
          viewerProfileId: session.profileId,
          viewerSessionId: sessionId,
          reviewState: 'pending',
          applyState: 'not_applied',
          gateState: 'open',
          lastAction: 'run_fixture_worker',
          lastActionAt: now,
          objectives,
          sessions: [...(state.sessions || []), session],
          verifications: [...(state.verifications || []), verification],
          lastRunAt: now,
          mission: {
            ...(state.mission || {}),
            status: 'running',
            lastRunAt: now,
          },
        };
      });
      const objectiveId = next.sessions?.[next.sessions.length - 1]?.objectiveId || null;
      const objective = objectiveId
        ? next.objectives?.find((entry) => entry?.id === objectiveId) || null
        : null;
      next.eventLedger = [...(next.eventLedger || []), createMissionEvent('fixture_worker', objective ? `Fixture worker completed ${objective.id}.` : 'Fixture worker ran.', {
        objectiveId: objective?.id || null,
        sessionId: next.sessions?.[next.sessions.length - 1]?.sessionId || null,
        verificationId: next.verifications?.[next.verifications.length - 1]?.verificationId || null,
      })];
      next.events = next.eventLedger;
      replaceMissionState(mockCurrentRepoPath, next);
      return cloneJson(cloneMissionState(next, { omitLatestEvent: true }));
    }

    if (cmd === 'run_code_worker') {
      const now = new Date().toISOString();
      const requestedObjectiveId = String(args?.objectiveId || '').trim();
      const requestedProfileId = String(args?.profileId || '').trim();
      const next = mutateMissionState(mockCurrentRepoPath, (state) => {
        const scheduler = deriveScheduler(state).summary;
        const resolvedObjectiveId = requestedObjectiveId || String(scheduler.nextObjectiveId || '').trim();
        const selectedProfile = requestedProfileId
          ? (state.profiles || []).find((entry) => entry?.id === requestedProfileId) || null
          : null;
        const profileId = selectedProfile?.id || 'local-code-worker';
        const sessionId = `code-session-${(state.sessions?.length || 0) + 1}`;
        const verificationId = `verification-${(state.verifications?.length || 0) + 1}`;
        const objectives = Array.isArray(state.objectives)
          ? state.objectives.map((objective, index) => {
              const matchesRequested = resolvedObjectiveId ? objective?.id === resolvedObjectiveId : index === 0;
              if (!matchesRequested) return objective;
              return {
                ...objective,
                state: 'done',
                latestSessionId: sessionId,
              };
            })
          : [];
        const objective = objectives.find((entry) => entry?.latestSessionId === sessionId) || null;
        const workspacePath = `${mockCurrentRepoPath}/.sloposcope/worktrees/${state.missionId || 'mission'}/${objective?.id || 'objective-1'}`;
        const verification = {
          verificationId,
          sessionId,
          objectiveId: objective?.id || null,
          objectiveTitle: objective?.title || null,
          profileId,
          kind: 'code_worker',
          status: 'passed',
          summary: objective ? `Code worker completed ${objective.id}.` : 'Code worker ran.',
          checkedAt: now,
          command: state.validationCommand || null,
          workspacePath,
          patchSummary: objective ? `Patch artifact for ${objective.id}` : 'Patch artifact',
          patchArtifact: `diff --git a/src/mock_worker.js b/src/mock_worker.js\n+export const objective = '${objective?.id || 'objective-1'}';\n`,
          targetFile: 'src/mock_worker.js',
          validationStatus: 'passed',
          reviewState: 'pending',
          applyState: 'not_applied',
          gateState: 'open',
          validation: {
            command: state.validationCommand || null,
            success: true,
            exitCode: 0,
            stdout: 'mock validation ok',
            stderr: '',
          },
        };
        const session = {
          sessionId,
          objectiveId: objective?.id || null,
          objectiveTitle: objective?.title || null,
          profileId,
          workerKind: 'code_worker',
          state: 'completed',
          result: 'passed',
          startedAt: now,
          updatedAt: now,
          verificationId,
          latestEvent: objective ? 'completed' : 'idle',
          command: state.validationCommand || null,
          workspacePath,
          patchSummary: objective ? `Patch artifact for ${objective.id}` : 'Patch artifact',
          patchArtifact: verification.patchArtifact,
          validationStatus: 'passed',
          validation: verification.validation,
          reviewState: 'pending',
          applyState: 'not_applied',
          gateState: 'open',
        };
        return {
          ...state,
          status: 'running',
          viewerProfileId: profileId,
          viewerSessionId: sessionId,
          reviewState: 'pending',
          applyState: 'not_applied',
          gateState: 'open',
          lastAction: 'run_code_worker',
          lastActionAt: now,
          objectives,
          sessions: [...(state.sessions || []), session],
          verifications: [...(state.verifications || []), verification],
          lastRunAt: now,
          mission: {
            ...(state.mission || {}),
            status: 'running',
            lastRunAt: now,
          },
        };
      });
      const objectiveId = next.sessions?.[next.sessions.length - 1]?.objectiveId || null;
      const objective = objectiveId
        ? next.objectives?.find((entry) => entry?.id === objectiveId) || null
        : null;
      next.eventLedger = [...(next.eventLedger || []), createMissionEvent('code_worker', objective ? `Code worker completed ${objective.id}.` : 'Code worker ran.', {
        objectiveId: objective?.id || null,
        sessionId: next.sessions?.[next.sessions.length - 1]?.sessionId || null,
        verificationId: next.verifications?.[next.verifications.length - 1]?.verificationId || null,
      })];
      next.events = next.eventLedger;
      replaceMissionState(mockCurrentRepoPath, next);
      return cloneJson(cloneMissionState(next, { omitLatestEvent: true }));
    }

    if (cmd === 'run_next_code_worker') {
      const requestedProfileId = String(args?.profileId || '').trim();
      const current = ensureMissionState();
      const scheduler = deriveScheduler(current).summary;
      if (!scheduler.nextObjectiveId) {
        return cloneJson(cloneMissionState(ensureMissionState(), { omitLatestEvent: true }));
      }
      return invoke('run_code_worker', {
        missionId: current.missionId || deriveMissionId(current, 'mission'),
        objectiveId: scheduler.nextObjectiveId,
        profileId: requestedProfileId || null,
      });
    }

    if (cmd === 'sweep_stale_sessions') {
      const now = new Date().toISOString();
      const next = mutateMissionState(mockCurrentRepoPath, (state) => {
        let swept = 0;
        const sessions = (state.sessions || []).map((session) => {
          if (String(session?.runtimeStatus || session?.state || '').toLowerCase() !== 'stuck') {
            return session;
          }
          swept += 1;
          return {
            ...session,
            state: 'failed',
            runtimeStatus: 'failed',
            leaseExpired: false,
            lastError: 'stale session swept',
            updatedAt: now,
          };
        });
        const staleObjectiveIds = new Set(
          sessions
            .filter((session) => String(session?.lastError || '').includes('stale session swept'))
            .map((session) => session?.objectiveId)
            .filter(Boolean),
        );
        const objectives = (state.objectives || []).map((objective) => (
          staleObjectiveIds.has(objective?.id) && objective?.state === 'claimed'
            ? { ...objective, state: 'pending' }
            : objective
        ));
        const eventLedger = [
          ...(state.eventLedger || []),
          createMissionEvent(
            swept > 0 ? 'stale_swept' : 'stale_sweep_noop',
            swept > 0 ? `Swept ${swept} stale session(s).` : 'No stale sessions to sweep.',
            { swept },
          ),
        ];
        return {
          ...state,
          objectives,
          sessions,
          eventLedger,
          events: eventLedger,
          lastAction: swept > 0 ? 'stale_swept' : 'stale_sweep_noop',
          lastActionAt: now,
        };
      });
      replaceMissionState(mockCurrentRepoPath, next);
      return cloneJson(cloneMissionState(next));
    }

    if (cmd === 'accept_worker_patch' || cmd === 'reject_worker_patch' || cmd === 'apply_worker_patch' || cmd === 'evaluate_worker_patch_gate') {
      const now = new Date().toISOString();
      const objectiveId = String(args?.objectiveId || '').trim();
      const action =
        cmd === 'accept_worker_patch'
          ? 'accept'
          : cmd === 'reject_worker_patch'
            ? 'reject'
            : cmd === 'apply_worker_patch'
              ? 'apply'
              : 'gate';
      const next = mutateMissionState(mockCurrentRepoPath, (state) => {
        const currentSession = objectiveId
          ? latestMissionSession({ sessions: (state.sessions || []).filter((entry) => entry.objectiveId === objectiveId) })
          : latestMissionSession(state);
        if (!currentSession) {
          throw new Error('No mission run available.');
        }
        const targetSessionId = currentSession.sessionId;
        const nextSessions = (state.sessions || []).map((entry) => {
          if (entry.sessionId !== targetSessionId) return entry;
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
              lastAction: 'reject',
              lastActionAt: now,
              applyState: 'blocked',
              gateState: 'blocked',
            };
          }
          if (action === 'apply') {
            if (String(entry.reviewState || state.reviewState || 'pending') !== 'accepted') {
              throw new Error('Mission run must be accepted before apply.');
            }
            if (String(entry.gateState || state.gateState || 'open') !== 'ready') {
              throw new Error('Mission run gate must be ready before apply.');
            }
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
        const updatedSession = nextSessions.find((entry) => entry.sessionId === targetSessionId) || currentSession;
        const reviewState = String(updatedSession.reviewState || state.reviewState || 'pending');
        const applyState = String(updatedSession.applyState || state.applyState || 'not_applied');
        const gateState = String(updatedSession.gateState || state.gateState || 'open');
        const status =
          action === 'reject'
            ? 'rejected'
            : action === 'apply'
              ? 'applied'
              : action === 'gate'
                ? 'accepted'
                : 'accepted';
        return {
          ...state,
          status,
          viewerProfileId: updatedSession.profileId || state.viewerProfileId || '',
          viewerSessionId: updatedSession.sessionId || state.viewerSessionId || targetSessionId,
          reviewState,
          applyState,
          gateState,
          lastAction: action,
          lastActionAt: now,
          sessions: nextSessions,
          mission: {
            ...(state.mission || {}),
            status,
            viewerProfileId: updatedSession.profileId || state.viewerProfileId || '',
            viewerSessionId: updatedSession.sessionId || state.viewerSessionId || targetSessionId,
            reviewState,
            applyState,
            gateState,
            lastAction: action,
            lastActionAt: now,
          },
        };
      });
      const verb = action === 'accept' ? 'Accepted' : action === 'reject' ? 'Rejected' : action === 'apply' ? 'Applied' : 'Gated';
      next.eventLedger = [...(next.eventLedger || []), createMissionEvent(action, `${verb} ${next.viewerSessionId || 'mission run'}.`, {
        sessionId: next.viewerSessionId || null,
      })];
      next.events = next.eventLedger;
      replaceMissionState(mockCurrentRepoPath, next);
      return cloneJson(cloneMissionState(next, { omitLatestEvent: true }));
    }

    if (cmd === 'set_watchdog_active_issue') {
      mockWatchdogState.activeIssue = args?.issue ?? null;
      mockWatchdogState.activeIssueContext = null;
      return cloneJson(mockWatchdogState);
    }

    if (cmd === 'set_watchdog_active_issue_context') {
      const context = args?.issueContext ?? null;
      mockWatchdogState.activeIssueContext = context;
      mockWatchdogState.activeIssue = context?.issue ?? null;
      return cloneJson(mockWatchdogState);
    }

    if (cmd === 'record_watchdog_validation') {
      mockWatchdogState.lastValidation = args?.validation ?? null;
      return cloneJson(mockWatchdogState);
    }

    if (cmd === 'run_watchdog_validation_command') {
      const kind = String(args?.kind || 'validation');
      const command = String(args?.command || '').trim();
      if (!command) throw new Error('Validation command is empty');
      const success = !command.toLowerCase().includes('fail');
      mockWatchdogState.lastValidation = {
        kind,
        success,
        ranAt: new Date().toISOString(),
        details: `command=\`${command}\``,
      };
      return cloneJson(mockWatchdogState);
    }

    if (cmd === 'check_watchdog_focus') {
      const issues = [];
      if (!mockWatchdogState.activeIssueContext) {
        issues.push({
          kind: 'unclear_mapping',
          severity: 'warning',
          title: 'Active issue is not set',
          why: 'Set active issue context before running focus checks.',
          evidence: ['issueContext=None'],
          recommendedNextStep: 'Set issue context first.',
        });
      }
      if (!mockWatchdogState.lastValidation) {
        issues.push({
          kind: 'missing_validation',
          severity: 'warning',
          title: 'No validation record',
          why: 'Record a test/build run for current work.',
          evidence: ['lastValidation=None'],
          recommendedNextStep: 'Record validation.',
        });
      }
      const status = issues.length === 0 ? 'on_track' : 'drift';
      const createdAt = new Date().toISOString();
      const trace = {
        id: `wd-mock-${Date.now()}`,
        createdAt,
        snapshot: {
          schemaVersion: 1,
          createdAt,
          branch: 'main',
          revision: 'mock-rev',
          since: '@',
          issueContext: mockWatchdogState.activeIssueContext,
          agentsRulesCount: 0,
          agentsRules: [],
          changedFilesTotal: 1,
          changedFiles: [
            {
              path: 'src/App.svelte',
              status: 'modified',
              diffExcerpt: ['+mock change'],
              diffLineCount: 1,
            },
          ],
          validation: mockWatchdogState.lastValidation,
        },
        verdict: {
          status,
          confidence: issues.length === 0 ? 0.85 : 0.62,
          summary:
            issues.length === 0
              ? 'Current changes appear on-track.'
              : 'Potential drift detected; review issues.',
          issues,
        },
      };
      mockWatchdogState.lastTraceId = trace.id;
      mockWatchdogState.lastSnapshot = trace.snapshot;
      mockWatchdogState.lastVerdict = trace.verdict;
      mockWatchdogState.alerts = issues.map((issue, index) => ({
        id: `${trace.id}-${index}`,
        kind: issue.kind,
        severity: issue.severity,
        title: issue.title,
        message: issue.why,
        evidence: issue.evidence,
        createdAt,
        verdictId: trace.id,
      }));
      mockWatchdogTraces = [
        trace,
        ...mockWatchdogTraces.filter((entry) => entry.id !== trace.id),
      ].slice(0, 20);
      return cloneJson(trace);
    }

    if (cmd === 'get_watchdog_traces') {
      const limit = Math.max(1, Number(args?.limit || 20));
      return cloneJson(mockWatchdogTraces.slice(0, limit));
    }

    if (cmd === 'conductor_get_status') {
      const traceLimit = Math.max(1, Math.min(100, Number(args?.traceLimit || 20)));
      return cloneJson(rebuildMockConductorStatus(traceLimit));
    }

    if (cmd === 'get_conductor_workflow_contract') {
      return cloneJson(mockConductorWorkflowLoad);
    }

    if (cmd === 'save_conductor_workflow_settings') {
      const settings = args?.settings || {};
      const contract = mockConductorWorkflowLoad?.contract || {
        frontMatter: {
          schemaVersion: 1,
          workflowId: 'watchdog_focus',
          provider: 'coding_agent_app_server',
          providerModel: null,
          providerStrictMode: false,
          traceRetention: 30,
          title: 'Sloposcope Watchdog Focus Check',
          summary: null,
          maxRetries: 2,
          timeoutSeconds: 900,
          requiredChecks: [],
        },
        promptTemplate: 'You are a watchdog evaluator.',
      };
      const front = contract.frontMatter || {};

      const provider = String(settings.provider || front.provider || 'coding_agent_app_server')
        .trim()
        .toLowerCase();
      const providerModelRaw = String(settings.providerModel ?? front.providerModel ?? '');
      const providerModel = providerModelRaw.trim();
      const timeoutSeconds = Math.max(
        10,
        Math.min(3600, Math.round(Number(settings.timeoutSeconds ?? front.timeoutSeconds ?? 900))),
      );
      const traceRetention = Math.max(
        1,
        Math.min(200, Math.round(Number(settings.traceRetention ?? front.traceRetention ?? 30))),
      );
      const providerStrictMode = Boolean(settings.providerStrictMode ?? front.providerStrictMode ?? false);

      mockConductorWorkflowLoad = {
        ...(mockConductorWorkflowLoad || {}),
        exists: true,
        loaded: true,
        error: null,
        contract: {
          ...contract,
          frontMatter: {
            ...front,
            provider,
            providerModel: providerModel === '' ? null : providerModel,
            providerStrictMode,
            timeoutSeconds,
            traceRetention,
          },
        },
      };
      return cloneJson(mockConductorWorkflowLoad);
    }

    if (cmd === 'conductor_enqueue_task') {
      const taskId = String(args?.taskId || '').trim().toLowerCase();
      if (!taskId) throw new Error('task_id is empty');
      if (mockConductorTasks.some((task) => task.taskId === taskId)) {
        throw new Error(`task already exists: ${taskId}`);
      }
      const maxRetries = Math.max(0, Math.min(10, Number(args?.maxRetries ?? 2)));
      const now = new Date().toISOString();
      const task = {
        taskId,
        state: 'claimed',
        claimedAt: now,
        startedAt: null,
        retryAt: null,
        reconciledAt: null,
        runAttempt: 0,
        retryCount: 0,
        maxRetries,
        lastError: null,
        reconciledOutcome: null,
        reconciledNote: null,
      };
      mockConductorTasks = [...mockConductorTasks, task].sort((a, b) => a.taskId.localeCompare(b.taskId));
      pushMockConductorTrace('task_enqueued', taskId, null);
      rebuildMockConductorStatus(mockConductorStatus._traceLimit || 20);
      return cloneJson(task);
    }

    if (cmd === 'conductor_refresh') {
      const tick = {
        startedTaskIds: [],
        reclaimedRetryTaskIds: [],
        scheduledRetryTaskIds: [],
        reconciledTaskIds: [],
      };
      pushMockConductorTrace('tick_start', null, 'mock refresh');

      const now = new Date().toISOString();
      const running = mockConductorTasks.find((task) => task.state === 'running');
      if (running) {
        running.state = 'reconciled';
        running.reconciledAt = now;
        running.reconciledOutcome = 'succeeded';
        tick.reconciledTaskIds.push(running.taskId);
        pushMockConductorTrace('task_reconciled', running.taskId, 'mock completed');
      } else {
        const retry = mockConductorTasks.find((task) => task.state === 'retry_scheduled');
        if (retry) {
          retry.state = 'claimed';
          retry.claimedAt = now;
          retry.retryAt = null;
          tick.reclaimedRetryTaskIds.push(retry.taskId);
          pushMockConductorTrace('retry_reclaimed', retry.taskId, null);
        }

        const claimed = mockConductorTasks.find((task) => task.state === 'claimed');
        if (claimed) {
          claimed.state = 'running';
          claimed.startedAt = now;
          claimed.runAttempt = Number(claimed.runAttempt || 0) + 1;
          tick.startedTaskIds.push(claimed.taskId);
          pushMockConductorTrace('task_started', claimed.taskId, null);
        }
      }

      mockConductorStatus.lastTickAt = now;
      pushMockConductorTrace(
        'tick_done',
        null,
        `started=${tick.startedTaskIds.length} reconciled=${tick.reconciledTaskIds.length}`,
      );
      const status = rebuildMockConductorStatus(mockConductorStatus._traceLimit || 20);
      return cloneJson({ tick, status });
    }

    if (cmd === 'conductor_prepare_launch') {
      const taskId = String(args?.taskId || '').trim().toLowerCase();
      const task = mockConductorTasks.find((entry) => entry.taskId === taskId);
      if (!task) throw new Error(`task not found: ${taskId}`);
      const payload = {
        taskId,
        runAttempt: Number(task.runAttempt || 0) + 1,
        provider: mockConductorWorkflowLoad?.contract?.frontMatter?.provider || 'coding_agent_app_server',
        providerModel: mockConductorWorkflowLoad?.contract?.frontMatter?.providerModel || null,
        issueTitle: mockWatchdogState.activeIssueContext?.issue?.title || taskId,
        prompt: mockWatchdogState.activeIssueContext?.summary || 'Mock launch payload',
      };
      pushMockConductorTrace('launch_prepared', taskId, payload.provider);
      rebuildMockConductorStatus(mockConductorStatus._traceLimit || 20);
      return cloneJson(payload);
    }

    if (cmd === 'conductor_dispatch_launch') {
      const taskId = String(args?.taskId || '').trim().toLowerCase();
      const task = mockConductorTasks.find((entry) => entry.taskId === taskId);
      if (!task) throw new Error(`task not found: ${taskId}`);
      const sessionId = `mock-session-${Date.now()}`;
      task.state = 'running';
      task.startedAt = new Date().toISOString();
      task.activeSessionId = sessionId;
      task.runAttempt = Number(task.runAttempt || 0) + 1;
      pushMockConductorTrace('launch_dispatched', taskId, sessionId);
      rebuildMockConductorStatus(mockConductorStatus._traceLimit || 20);
      return cloneJson({
        taskId,
        sessionId,
        dispatchedAt: task.startedAt,
      });
    }

    if (cmd === 'conductor_ingest_runner_event') {
      const taskId = String(args?.taskId || '').trim().toLowerCase();
      const rawEvent = String(args?.rawEvent || '').trim();
      if (!taskId || !rawEvent) throw new Error('taskId and rawEvent are required');
      let parsed;
      try {
        parsed = JSON.parse(rawEvent);
      } catch (error) {
        throw new Error(`Invalid runner event JSON: ${error?.message || error}`);
      }
      const task = mockConductorTasks.find((entry) => entry.taskId === taskId);
      if (task && typeof parsed?.eventType === 'string') {
        if (parsed.eventType === 'completed') {
          task.state = 'reconciled';
          task.reconciledOutcome = 'succeeded';
          task.reconciledAt = new Date().toISOString();
        } else if (parsed.eventType === 'failed') {
          task.state = 'retry_scheduled';
          task.retryAt = new Date().toISOString();
          task.retryCount = Number(task.retryCount || 0) + 1;
        }
      }
      pushMockConductorTrace('runner_event_ingested', taskId, parsed?.eventType || 'unknown');
      rebuildMockConductorStatus(mockConductorStatus._traceLimit || 20);
      return cloneJson({ accepted: true, taskId, eventType: parsed?.eventType || null });
    }

    if (cmd === 'refresh_graph_clusters') {
      return {
        mode: 'heuristic',
        assignmentCount: 0,
        stale: false,
        error: 'Julia sidecar unavailable in mock bridge',
      };
    }

    if (cmd === 'search_graph_entities') {
      const query = String(args?.query || '').trim().toLowerCase();
      if (!query) return [];
      return createMockGraph()
        .nodes
        .filter((node) => node.id.toLowerCase().includes(query) || String(node.file || '').toLowerCase().includes(query))
        .map((node, index) => ({
          entityId: node.id,
          displayName: node.id,
          filePath: node.file || node.id,
          language: node.file?.split('.').pop() || 'text',
          score: 1 - index * 0.1,
          why: 'matched mock graph entity',
        }));
    }

    if (cmd === 'get_file_source') return 'defmodule MockModule do\n  def hello, do: :world\nend';
    if (cmd === 'get_file_diff') {
      return '@@ -1,2 +1,3 @@\n defmodule MockModule do\n+  def goodbye, do: :moon\n   def hello, do: :world\n end';
    }
    if (cmd === 'save_file') {
      debugLog('[MOCK] saving file', args);
      return null;
    }
    return null;
  };

  const listen = async (event, _cb) => {
    if (typeof window !== 'undefined' && window.__TAURI_INTERNALS__) {
      const { listen: tauriListen } = await import('@tauri-apps/api/event');
      return tauriListen(event, _cb);
    }
    debugLog('[MOCK] listen', event);
    return () => {};
  };

  return { invoke, listen };
}
