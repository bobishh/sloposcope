function cloneJson(value) {
  return value == null ? value : JSON.parse(JSON.stringify(value));
}

function fallbackMockAnatomy(file) {
  if (file === 'mock.ex') {
    return {
      status: 'partial', file, language: 'ex', evidence: 'tree_sitter',
      profileId: 'elixir:callable-anatomy-v1',
      callables: [
        { id: 'mock:hello', matchKey: 'MockModule::def::hello', owner: 'MockModule', name: 'hello', kind: 'def', visibility: 'public', arity: null, startLine: null, endLine: null, bodyLineSpan: null, decisionCount: null },
        { id: 'mock:secret', matchKey: 'MockModule::defp::secret', owner: 'MockModule', name: 'secret', kind: 'defp', visibility: 'private', arity: null, startLine: null, endLine: null, bodyLineSpan: null, decisionCount: null },
      ],
      stateSlots: null,
      reason: 'Mock capture lacks measurable body facts',
    };
  }
  return {
    status: 'unavailable', file, language: file.split('.').pop() || 'unknown',
    profileId: `${file.split('.').pop() || 'unknown'}:unavailable`,
    evidence: 'tree_sitter', callables: [], stateSlots: null,
    reason: 'no Tree-sitter anatomy profile for this extension',
  };
}

const MOCK_CURRENT_REPO_PATH_KEY = 'sloposcope.mockCurrentRepoPath';
const MOCK_MISSION_STATE_BY_REPO_KEY = 'sloposcope.mockMissionStateByRepo';
const MOCK_COORDINATOR_STATE_BY_REPO_KEY = 'sloposcope.mockCoordinatorStateByRepo';
const COORDINATOR_NAME = 'Coordinator';

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

function createMockGraph(projectedNodes = []) {
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
      ...projectedNodes,
    ],
    edges: [{ source: 'MockHelper', target: 'MockModule', type: 'call' }],
  };
}

function createMockGraphView(request = {}, projectedNodes = []) {
  const mode = String(request?.mode || 'investigation');
  const seed = request?.seed || { kind: 'working_copy' };
  const graph = createMockGraph(projectedNodes);
  const projectedProvenance = projectedNodes.map((node) => ({
    nodeId: node.id,
    reason: `pending graph rebuild · worker ${node.worker_session_id || 'unknown'}`,
  }));
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
            ...projectedProvenance,
          ]
        : [],
    canonicalEntityCount: graph.nodes.length,
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

function createDefaultCoordinatorState() {
  return {
    threadId: '',
    missionId: '',
    personaName: COORDINATOR_NAME,
    title: 'Unscoped mission',
    state: 'wake',
    draftMission: {
      title: '',
      brief: '',
      validationCommand: '',
      issueKey: '',
      issueUrl: '',
      plannerProfileId: '',
      plannerNotes: '',
      plannerPromptOverride: '',
    },
    pendingPlan: [],
    messages: [],
    summary: 'Coordinator waiting on mission brief.',
    latestSessionId: null,
    latestLiveSessionId: null,
    threads: [],
    threadSnapshots: {},
    canApprovePlan: false,
    canAskReport: false,
    canLaunchWorkers: false,
  };
}

function createCoordinatorMessage(role, kind, content, payload = null) {
  return {
    id: `coord-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    role,
    kind,
    content,
    payload,
    createdAt: new Date().toISOString(),
  };
}

function summarizeCoordinatorThread(state) {
  return {
    id: state.threadId || `coordinator-${Date.now()}`,
    missionId: state.missionId || '',
    title: state.title || 'Unscoped mission',
    state: state.state || 'wake',
    updatedAt: new Date().toISOString(),
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

function deriveProblemTitle(problem = '') {
  const firstLine = String(problem || '')
    .split('\n')
    .map((line) => line.trim())
    .find(Boolean) || 'Unscoped mission';
  const compact = firstLine.replace(/[.!?]+$/g, '').trim();
  const title = compact.split(/\s+/).slice(0, 7).join(' ').trim();
  return title || 'Unscoped mission';
}

function extractValidationCommand(input = '') {
  const trimmed = String(input || '').trim();
  if (!trimmed) return '';
  const lower = trimmed.toLowerCase();
  const prefixes = ['validation command:', 'validation:', 'command:', 'run:'];
  for (const prefix of prefixes) {
    const index = lower.indexOf(prefix);
    if (index >= 0) {
      return trimmed.slice(index + prefix.length).trim().replace(/^`|`$/g, '');
    }
  }
  for (const marker of ['npm ', 'pnpm ', 'yarn ', 'bun ', 'cargo ', 'mix ', 'pytest ', 'uv ', './']) {
    const index = lower.indexOf(marker);
    if (index >= 0) {
      return trimmed
        .slice(index)
        .trim()
        .replace(/^`|`$/g, '')
        .replace(/[.!?;]+$/g, '')
        .trim();
    }
  }
  return trimmed
    .split('\n')
    .map((line) => line.trim().replace(/^`|`$/g, ''))
    .find((line) => /^(npm|pnpm|yarn|bun|cargo|mix|pytest|uv|\.\/*)/i.test(line))
    || '';
}

function messageWantsReport(input = '') {
  const lower = String(input || '').toLowerCase();
  return lower.includes('report') || lower.includes('status') || lower.includes('progress');
}

function messageIsValidationOnly(input = '') {
  const text = String(input || '').trim();
  const lower = text.toLowerCase();
  return Boolean(extractValidationCommand(text))
    && (
      lower.includes('validation command')
      || lower.startsWith('validation:')
      || lower.startsWith('command:')
      || lower.startsWith('run:')
      || lower.startsWith('use ')
    );
}

function missionSessionLiveliness(session = {}) {
  const runtime = String(session?.runtimeStatus || session?.state || '').toLowerCase();
  if (runtime === 'running' || runtime === 'claimed' || runtime === 'idle') return 'live';
  return 'closed';
}

function buildCoordinatorPlanMessage(state) {
  const objectives = Array.isArray(state?.pendingPlan) ? state.pendingPlan : [];
  const lines = [`Plan proposal. ${objectives.length} objective(s).`];
  for (const objective of objectives) {
    const scope = Array.isArray(objective?.scopePaths) && objective.scopePaths.length
      ? objective.scopePaths.join(', ')
      : 'inferred';
    lines.push(`- ${objective?.title || objective?.id} (scope: ${scope})`);
  }
  lines.push('Approve plan when ready.');
  return lines.join('\n');
}

function buildMissionReport(state = {}) {
  const total = Array.isArray(state?.objectives) ? state.objectives.length : 0;
  const done = (state?.objectives || []).filter((entry) => entry?.state === 'done').length;
  const pending = (state?.objectives || []).filter((entry) => entry?.state !== 'done').length;
  const liveSessions = (state?.sessions || []).filter((entry) => missionSessionLiveliness(entry) === 'live');
  const latestVerification = (state?.verifications || []).slice(-1)[0];
  const latestEvent = (state?.eventLedger || []).slice(-1)[0];
  const latestLive = liveSessions.slice(-1)[0];
  const lines = [
    `Mission report. ${state?.title || 'Untitled mission'}.`,
    `Objectives: ${total} total, ${pending} pending, ${done} done.`,
    `Live sessions: ${liveSessions.length}.`,
  ];
  if (latestLive?.sessionId) {
    lines.push(`Latest live session: ${latestLive.sessionId} [${latestLive.runtimeStatus || latestLive.state || 'running'}].`);
  }
  if (latestVerification?.summary) lines.push(`Latest verification: ${latestVerification.summary}.`);
  if (latestEvent?.summary) lines.push(`Latest event: ${latestEvent.summary}.`);
  return lines.join(' ');
}

function buildWakeMessage(repoPath, missionState, coordinatorState) {
  const repoLabel = String(repoPath || '/path/to/despair').split('/').filter(Boolean).at(-1) || 'repo';
  if (missionState?.missionId && missionState?.status && missionState.status !== 'idle') {
    return `${COORDINATOR_NAME}. Repo ${repoLabel} warm. Resuming mission '${missionState.title || coordinatorState?.title || 'untitled'}' in state ${missionState.status}. ${(missionState.objectives || []).length} objectives, ${(missionState.sessions || []).length} sessions. What problem or feature now?`;
  }
  return `${COORDINATOR_NAME}. Repo ${repoLabel} warm. No active mission locked. What problem or feature now? Validation can come later.`;
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

function lifecyclePayloadWithBase(payload = {}, objectiveId = null, sessionId = null) {
  const next = payload && typeof payload === 'object' && !Array.isArray(payload)
    ? { ...payload }
    : { value: payload };
  if (objectiveId && !next.objectiveId) next.objectiveId = objectiveId;
  if (sessionId && !next.sessionId) next.sessionId = sessionId;
  return next;
}

function mockLifecycleSortKey(value = '') {
  const text = String(value || '');
  const unixMatch = text.match(/^unix:(\d+)$/);
  if (unixMatch) return [0, Number(unixMatch[1]), text];
  const parsed = Date.parse(text);
  if (Number.isFinite(parsed)) return [1, parsed, text];
  return [2, 0, text];
}

function sortMockLifecycleEvents(events = []) {
  return [...events].sort((left, right) => {
    const leftKey = mockLifecycleSortKey(left?.occurredAt);
    const rightKey = mockLifecycleSortKey(right?.occurredAt);
    for (let index = 0; index < leftKey.length; index += 1) {
      if (leftKey[index] < rightKey[index]) return -1;
      if (leftKey[index] > rightKey[index]) return 1;
    }
    return String(left?.id || '').localeCompare(String(right?.id || ''));
  });
}

function createMockLifecycleEvent({
  id,
  taskId,
  threadId,
  type,
  actor,
  source,
  correlation,
  causation = null,
  occurredAt,
  payload = {},
  objectiveId = null,
  sessionId = null,
}) {
  return {
    id,
    taskId,
    threadId,
    type,
    actor,
    source,
    correlation,
    causation,
    occurredAt,
    payload: lifecyclePayloadWithBase(payload, objectiveId, sessionId),
  };
}

function coordinatorMessageLifecycleType(message = {}) {
  const role = String(message?.role || '');
  const kind = String(message?.kind || '');
  const content = String(message?.content || '').toLowerCase();
  if (role === 'user' && kind === 'prompt') return 'task.created';
  if (kind === 'plan') return 'plan.proposed';
  if (kind === 'report') return 'coordinator.reported';
  if (kind === 'status' && (content.startsWith('plan approved') || message?.payload?.missionId)) return 'plan.approved';
  if (kind === 'status' && content.startsWith('workers launched')) return 'workers.launched';
  return null;
}

function verificationFailed(verification = {}) {
  const validation = verification?.validation;
  const validationFailed =
    validation && typeof validation === 'object' && validation.success === false;
  return String(verification?.status || '').toLowerCase() === 'failed' || validationFailed;
}

function projectMockTaskLifecycleEvents({ taskId = '', threadId = '', messages = [], missionState = {} } = {}) {
  const normalizedTaskId = String(taskId || missionState?.missionId || threadId || 'task');
  const normalizedThreadId = String(threadId || normalizedTaskId);
  const events = [];

  for (const message of Array.isArray(messages) ? messages : []) {
    const type = coordinatorMessageLifecycleType(message);
    if (!type) continue;
    events.push(createMockLifecycleEvent({
      id: `coordinator-message-${message.id}`,
      taskId: String(message?.payload?.missionId || missionState?.missionId || normalizedTaskId),
      threadId: normalizedThreadId,
      type,
      actor: String(message?.role || 'coordinator'),
      source: 'coordinator_message',
      correlation: normalizedThreadId,
      occurredAt: message?.createdAt || new Date().toISOString(),
      payload: {
        messageId: message?.id || '',
        kind: message?.kind || '',
        content: message?.content || '',
        payload: cloneJson(message?.payload || null),
      },
      objectiveId: message?.payload?.objectiveId || null,
      sessionId: message?.payload?.sessionId || null,
    }));
  }

  for (const session of Array.isArray(missionState?.sessions) ? missionState.sessions : []) {
    const sessionId = String(session?.sessionId || '');
    if (!sessionId) continue;
    events.push(createMockLifecycleEvent({
      id: `session-${sessionId}-started`,
      taskId: String(missionState?.missionId || normalizedTaskId),
      threadId: normalizedThreadId,
      type: 'worker.started',
      actor: String(session?.profileId || 'worker'),
      source: 'agent_session',
      correlation: String(missionState?.missionId || normalizedTaskId),
      occurredAt: session?.startedAt || session?.createdAt || session?.updatedAt || new Date().toISOString(),
      payload: {
        profileId: session?.profileId || '',
        state: session?.state || '',
        workspacePath: session?.workspacePath || '',
        touchedPaths: cloneJson(session?.touchedPaths || []),
      },
      objectiveId: session?.objectiveId || null,
      sessionId,
    }));
    const runtimeStatus = String(session?.runtimeStatus || session?.state || '').toLowerCase();
    if (runtimeStatus === 'stuck' || String(session?.lastError || '').toLowerCase().includes('stale')) {
      events.push(createMockLifecycleEvent({
        id: `session-${sessionId}-stale`,
        taskId: String(missionState?.missionId || normalizedTaskId),
        threadId: normalizedThreadId,
        type: 'session.stale',
        actor: 'scheduler',
        source: 'agent_session',
        correlation: String(missionState?.missionId || normalizedTaskId),
        causation: `session-${sessionId}-started`,
        occurredAt: session?.lastEventAt || session?.updatedAt || new Date().toISOString(),
        payload: {
          runtimeStatus,
          leaseExpired: Boolean(session?.leaseExpired),
          lastError: session?.lastError || '',
        },
        objectiveId: session?.objectiveId || null,
        sessionId,
      }));
    }
  }

  for (const verification of Array.isArray(missionState?.verifications) ? missionState.verifications : []) {
    if (!verificationFailed(verification)) continue;
    const verificationId = String(verification?.verificationId || verification?.id || '');
    const sessionId = verification?.sessionId || null;
    events.push(createMockLifecycleEvent({
      id: verificationId || `verification-${events.length + 1}`,
      taskId: String(missionState?.missionId || normalizedTaskId),
      threadId: normalizedThreadId,
      type: 'verification.failed',
      actor: 'verifier',
      source: 'verification',
      correlation: String(missionState?.missionId || normalizedTaskId),
      causation: sessionId ? `session-${sessionId}-started` : null,
      occurredAt: verification?.checkedAt || verification?.createdAt || verification?.updatedAt || new Date().toISOString(),
      payload: {
        verificationId,
        status: verification?.status || '',
        summary: verification?.summary || '',
        details: cloneJson(verification?.details || verification?.validation || {}),
      },
      objectiveId: verification?.objectiveId || null,
      sessionId,
    }));
    if (verification?.retryScheduled || verification?.retryAt || verification?.details?.retryAt) {
      events.push(createMockLifecycleEvent({
        id: `${verificationId || `verification-${events.length + 1}`}-retry`,
        taskId: String(missionState?.missionId || normalizedTaskId),
        threadId: normalizedThreadId,
        type: 'task.retry_scheduled',
        actor: 'scheduler',
        source: 'verification',
        correlation: String(missionState?.missionId || normalizedTaskId),
        causation: verificationId || null,
        occurredAt: verification?.updatedAt || verification?.checkedAt || new Date().toISOString(),
        payload: {
          verificationId,
          retryAt: verification?.retryAt || verification?.details?.retryAt || null,
        },
        objectiveId: verification?.objectiveId || null,
        sessionId,
      }));
    }
  }

  const sessionsById = new Map(
    (Array.isArray(missionState?.sessions) ? missionState.sessions : [])
      .map((session) => [session?.sessionId, session])
      .filter(([sessionId]) => sessionId),
  );
  const ledger = Array.isArray(missionState?.eventLedger) ? missionState.eventLedger : [];
  ledger.forEach((entry, index) => {
    const kind = String(entry?.kind || '');
    const type =
      kind === 'accept'
        ? 'review.accepted'
        : kind === 'reject'
          ? 'review.rejected'
          : kind === 'apply'
            ? 'patch.applied'
            : '';
    if (!type) return;
    const sessionId = entry?.payload?.sessionId || null;
    const session = sessionId ? sessionsById.get(sessionId) : null;
    events.push(createMockLifecycleEvent({
      id: `event-ledger-${kind}-${index + 1}`,
      taskId: String(missionState?.missionId || normalizedTaskId),
      threadId: normalizedThreadId,
      type,
      actor: kind === 'apply' ? 'system' : 'reviewer',
      source: 'event_ledger',
      correlation: String(missionState?.missionId || normalizedTaskId),
      causation: sessionId ? `session-${sessionId}-started` : null,
      occurredAt: entry?.at || new Date().toISOString(),
      payload: {
        kind,
        summary: entry?.summary || '',
        ...cloneJson(entry?.payload || {}),
      },
      objectiveId: session?.objectiveId || entry?.payload?.objectiveId || null,
      sessionId,
    }));
  });

  let previousId = null;
  return sortMockLifecycleEvents(events).map((event) => {
    const next = event.causation ? event : { ...event, causation: previousId };
    previousId = next.id;
    return next;
  });
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
    lifecycleEvents: projectMockTaskLifecycleEvents({
      taskId: computed?.missionId || 'mission',
      threadId: computed?.missionId || 'mission',
      missionState: computed,
    }),
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
  let mockGraphProjectionsByRepo = new Map();
  const mockListeners = new Map();
  const mockJevCache = new Map();
  let mockJevPlan = null;
  const createMissionState = createDefaultMissionStateFromHost || createDefaultMissionState;
  let mockCurrentRepoPath = readMockStorage(MOCK_CURRENT_REPO_PATH_KEY, '/path/to/despair') || '/path/to/despair';
  const mockMissionStatesByRepo = new Map(
    Object.entries(readMockStorage(MOCK_MISSION_STATE_BY_REPO_KEY, {}) || {}).map(([repoPath, state]) => [
      repoPath,
      cloneJson(state) || createMissionState(),
    ]),
  );
  const mockCoordinatorStatesByRepo = new Map(
    Object.entries(readMockStorage(MOCK_COORDINATOR_STATE_BY_REPO_KEY, {}) || {}).map(([repoPath, state]) => [
      repoPath,
      cloneJson(state) || createDefaultCoordinatorState(),
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

  function persistMockCoordinatorStates() {
    const serialized = {};
    for (const [repoPath, state] of mockCoordinatorStatesByRepo.entries()) {
      serialized[repoPath] = state;
    }
    writeMockStorage(MOCK_COORDINATOR_STATE_BY_REPO_KEY, serialized);
  }

  function ensureMissionState(repoPath = mockCurrentRepoPath) {
    const normalizedRepoPath = String(repoPath || '/path/to/despair');
    if (!mockMissionStatesByRepo.has(normalizedRepoPath)) {
      mockMissionStatesByRepo.set(normalizedRepoPath, createMissionState());
      persistMockMissionStates();
    }
    return mockMissionStatesByRepo.get(normalizedRepoPath);
  }

  function normalizeCoordinatorState(repoPath, state) {
    const missionState = ensureMissionState(repoPath);
    const schedulerSummary =
      missionState?.scheduler && typeof missionState.scheduler === 'object'
        ? missionState.scheduler
        : deriveScheduler(missionState).summary;
    const next = cloneJson(state || createDefaultCoordinatorState());
    const activeThreadMissionId = String(next.missionId || '').trim();
    const missionMatchesThread = Boolean(activeThreadMissionId) && missionState?.missionId === activeThreadMissionId;
    next.personaName = next.personaName || COORDINATOR_NAME;
    next.summary = next.summary || 'Coordinator waiting on mission brief.';
    next.latestSessionId = next.latestSessionId || (missionMatchesThread ? (missionState.sessions || [])[0]?.sessionId || null : null);
    next.latestLiveSessionId = next.latestLiveSessionId
      || (missionMatchesThread ? (missionState.sessions || []).find((entry) => missionSessionLiveliness(entry) === 'live')?.sessionId || null : null);
    next.threads = Array.isArray(next.threads) ? next.threads : [];
    next.threadSnapshots = next.threadSnapshots && typeof next.threadSnapshots === 'object' ? next.threadSnapshots : {};
    next.canApprovePlan = Array.isArray(next.pendingPlan) && next.pendingPlan.length > 0;
    next.canAskReport = Boolean(activeThreadMissionId);
    next.canLaunchWorkers = missionMatchesThread && Number(schedulerSummary?.readyCount || 0) > 0;
    next.lifecycleEvents = projectMockTaskLifecycleEvents({
      taskId: activeThreadMissionId || next.threadId || '',
      threadId: next.threadId || activeThreadMissionId || '',
      messages: next.messages || [],
      missionState: missionMatchesThread ? missionState : {},
    });
    if (next.threadId) {
      next.threadSnapshots[next.threadId] = {
        ...cloneJson(next),
        threads: undefined,
        threadSnapshots: undefined,
      };
      const currentSummary = summarizeCoordinatorThread(next);
      const rest = next.threads.filter((entry) => entry?.id !== currentSummary.id);
      next.threads = [currentSummary, ...rest]
        .sort((left, right) => String(right?.updatedAt || '').localeCompare(String(left?.updatedAt || '')));
    }
    return next;
  }

  function ensureCoordinatorState(repoPath = mockCurrentRepoPath) {
    const normalizedRepoPath = String(repoPath || '/path/to/despair');
    if (!mockCoordinatorStatesByRepo.has(normalizedRepoPath)) {
      const missionState = ensureMissionState(normalizedRepoPath);
      const initial = normalizeCoordinatorState(normalizedRepoPath, {
        ...createDefaultCoordinatorState(),
        threadId: `coordinator-${Date.now()}`,
        missionId: missionState.missionId || '',
        title: missionState.title || 'Unscoped mission',
      });
      initial.messages = [createCoordinatorMessage('coordinator', 'wake_brief', buildWakeMessage(normalizedRepoPath, missionState, initial), {
        personaName: COORDINATOR_NAME,
      })];
      initial.threadSnapshots = {
        [initial.threadId]: {
          ...cloneJson(initial),
          threads: undefined,
          threadSnapshots: undefined,
        },
      };
      initial.threads = [summarizeCoordinatorThread(initial)];
      mockCoordinatorStatesByRepo.set(normalizedRepoPath, initial);
      persistMockCoordinatorStates();
    }
    return mockCoordinatorStatesByRepo.get(normalizedRepoPath);
  }

  function replaceCoordinatorState(repoPath, nextState) {
    const normalizedRepoPath = String(repoPath || '/path/to/despair');
    const cloned = normalizeCoordinatorState(normalizedRepoPath, nextState);
    mockCoordinatorStatesByRepo.set(normalizedRepoPath, cloned);
    persistMockCoordinatorStates();
    return cloned;
  }

  function mutateCoordinatorState(repoPath, updater) {
    const normalizedRepoPath = String(repoPath || '/path/to/despair');
    const current = ensureCoordinatorState(normalizedRepoPath);
    const next = updater(cloneJson(current) || createDefaultCoordinatorState());
    return replaceCoordinatorState(normalizedRepoPath, next);
  }

  function replaceMissionState(repoPath, nextState) {
    const normalizedRepoPath = String(repoPath || '/path/to/despair');
    const cloned = cloneJson(nextState || createMissionState());
    cloned.lifecycleEvents = projectMockTaskLifecycleEvents({
      taskId: cloned?.missionId || 'mission',
      threadId: cloned?.missionId || 'mission',
      missionState: cloned,
    });
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

  function mockProjectionNodes(repoPath = mockCurrentRepoPath) {
    return cloneJson(mockGraphProjectionsByRepo.get(repoPath) || []);
  }

  function mockProjectionNodesForRequest(request = {}, repoPath = mockCurrentRepoPath) {
    return request?.seed?.kind === 'commit' ? [] : mockProjectionNodes(repoPath);
  }

  function emitMockEvent(event, payload) {
    const listeners = mockListeners.get(event);
    if (!listeners || listeners.size === 0) return;
    queueMicrotask(() => {
      for (const callback of listeners) {
        callback({ event, payload });
      }
    });
  }

  function projectAppliedWorkerSession(session) {
    if (!session) return;
    const targetFile = String(session.targetFile || 'src/mock_worker.js').replace(/^\.\/+/, '');
    const projection = {
      id: targetFile,
      type: 'file',
      file: targetFile,
      line_count: 3,
      change_status: 'added',
      projection_state: 'pending_graph_rebuild',
      worker_session_id: session.sessionId || '',
      functions: [],
    };
    const current = mockGraphProjectionsByRepo.get(mockCurrentRepoPath) || [];
    const next = [
      ...current.filter((node) => node.id !== projection.id),
      projection,
    ];
    mockGraphProjectionsByRepo = new Map(mockGraphProjectionsByRepo).set(mockCurrentRepoPath, next);
    const graphView = createMockGraphView(
      { mode: 'investigation', seed: { kind: 'working_copy' }, aggregate: 'file' },
      next,
    );
    emitMockEvent('file-touched', targetFile);
    emitMockEvent('graph-updated', {
      graph: graphView.graph,
      graph_view: graphView,
      current_branch: 'main',
      current_revision: `mock-apply-${session.sessionId || Date.now()}`,
      since: '@',
    });
  }

  const invoke = async (cmd, args) => {
    const safeArgs = cmd === 'confirm_jev_analysis' ? '[credentials omitted]' : args;
    if (typeof window !== 'undefined' && window.__TAURI_INTERNALS__) {
      const { invoke: tauriInvoke } = await import('@tauri-apps/api/core');
      debugLog(`[FRONTEND] Invoking ${cmd}`, safeArgs);
      return tauriInvoke(cmd, args);
    }
    if (cmd === 'check_watchdog_before_push') cmd = 'check_watchdog_focus';
    debugLog('[MOCK] invoke', cmd, safeArgs);

    // Browser fixtures only. Native invocation returns above; these never call a model.
    if (cmd === 'get_jev_index') {
      mockJevPlan = null;
      const changed = typeof localStorage !== 'undefined' && localStorage.getItem('sloposcope.mockJevChangeDuringSend') === '1' && mockJevCache.has(`${mockCurrentRepoPath}:demo-main`);
      const groups = [
        { id: changed ? 'demo-main-v2' : 'demo-main', paths: ['mock.ex', 'mock_copy.ex'], language: 'elixir', source_bytes: 50, excerpt_bytes: 50, truncated: false },
        { id: 'demo-helper', paths: ['mock_helper.ex'], language: 'elixir', source_bytes: 40, excerpt_bytes: 40, truncated: false },
      ].map(g => ({ ...g, source_id: g.id === 'demo-main-v2' ? 'mock-source:changed' : `mock-source:${g.paths[0]}`, cached: mockJevCache.get(`${mockCurrentRepoPath}:${g.id}`) || null }));
      return { repo: mockCurrentRepoPath, groups, excluded: { generated: 1 }, transport: 'demo' };
    }
    if (cmd === 'preview_jev_analysis') {
      mockJevPlan = null;
      const inventory = await invoke('get_jev_index');
      const cap = Number(args?.maxRequests);
      if (!Number.isInteger(cap) || cap < 1 || cap > 20) throw new Error('Request limit must be 1–20');
      const selected = inventory.groups.filter(g => args?.selected?.includes(g.id));
      if (!selected.length) throw new Error('Select source groups first');
      const groups = selected.filter(g => !g.cached);
      const payload = { model: 'demo-not-a-model', state: { items: groups.map(g => ({ language: g.language, source: 'defmodule MockModule do\n  def hello, do: :world\nend', truncated: false })) }, questions: { role: { type: 'choice', instructions: 'Synthetic demo only' } } };
      const bytes = JSON.stringify(payload).length;
      mockJevPlan = { id: `demo-${Date.now()}`, repo: mockCurrentRepoPath, request_count: groups.length ? 1 : 0, payload_bytes: groups.length ? bytes : 0, cached: selected.filter(g => g.cached).map(g => g.cached), batches: groups.length ? [{ groups, payload, payload_bytes: bytes }] : [], model: 'DEMO', rubric: 'file-traits-v1' };
      return cloneJson(mockJevPlan);
    }
    if (cmd === 'confirm_jev_analysis') {
      const plan = mockJevPlan;
      mockJevPlan = null;
      if (!plan || plan.id !== args?.planId || plan.request_count !== args?.confirmedRequests || plan.repo !== mockCurrentRepoPath) throw new Error('Approval expired or consumed; preview again');
      await new Promise(resolve => setTimeout(resolve, 150));
      if (typeof localStorage !== 'undefined' && localStorage.getItem('sloposcope.mockJevFailure') === '1') {
        return { requests_attempted: 1, requests_completed: 0, input_tokens: 0, output_tokens: 0, assessments: [], error: 'Stopped after request failure; no automatic retry' };
      }
      const assessments = plan.batches.flatMap(b => b.groups).map(g => ({ id: g.id, source_id: g.source_id, paths: g.paths, model: 'DEMO', rubric: 'file-traits-v1', truncated: false, answers: {
        role: { choice: 'transformation', probabilities: { transformation: 0.8, orchestration: 0.1, boundary: 0.05, representation: 0.05, unknown: 0 }, confidence: 0.6 },
        multiple: { noul: 0.2 }, hiding: { score: 1.1, probabilities: { '0': 0.1, '1': 0.7, '2': 0.2 }, confidence: 0.4, legend: { '0': 'Shallow', '1': 'Mixed', '2': 'Deep' } },
        context: { choice: 'sufficient', probabilities: { sufficient: 0.8, insufficient: 0.2 }, confidence: 0.3 },
      } }));
      for (const item of assessments) mockJevCache.set(`${mockCurrentRepoPath}:${item.id}`, item);
      return { requests_attempted: plan.request_count, requests_completed: plan.request_count, input_tokens: 100, output_tokens: 30, assessments, error: null };
    }

    if (cmd === 'get_graph_view') {
      return createMockGraphView(args?.request, mockProjectionNodesForRequest(args?.request));
    }

    if (cmd === 'get_graph_anatomies') {
      const fixtures = readMockStorage('sloposcope.mockAnatomyComparison', null) || {};
      const existing = readMockStorage('sloposcope.mockAnatomy', null) || {};
      const delay = Number(readMockStorage('sloposcope.mockAnatomyDelay', 0));
      if (delay > 0) await new Promise(resolve => setTimeout(resolve, delay));
      return (Array.isArray(args?.files) ? args.files : []).map(file => {
        const fixture = fixtures[file];
        const hasExplicitAfter = fixture && Object.hasOwn(fixture, 'after');
        const anatomy = hasExplicitAfter
          ? fixture.after?.anatomy || null
          : existing[file] || fallbackMockAnatomy(file);
        const fixtureStatus = hasExplicitAfter ? fixture.after?.status : null;
        return {
          file,
          label: fixture?.after?.label || (args?.selection?.mode === 'working_copy' ? 'working copy' : 'selected snapshot'),
          sourceId: fixture?.after?.sourceId || anatomy?.sourceId || `mock-source:${file}`,
          status: fixtureStatus || (anatomy?.status === 'unavailable' ? 'unavailable' : 'available'),
          anatomy: anatomy ? cloneJson({ ...anatomy, sourceId: fixture?.after?.sourceId || anatomy.sourceId || `mock-source:${file}` }) : null,
          reason: fixture?.after?.reason || anatomy?.reason || null,
        };
      });
    }

    if (cmd === 'get_file_anatomy_comparison') {
      const file = String(args?.file || '');
      const fixtures = readMockStorage('sloposcope.mockAnatomyComparison', null) || {};
      const fixture = fixtures[file];
      const delay = Number(readMockStorage('sloposcope.mockAnatomyDelay', 0));
      if (delay > 0) await new Promise(resolve => setTimeout(resolve, delay));
      if (fixture) return cloneJson(fixture);
        const anatomy = readMockStorage('sloposcope.mockAnatomy', null)?.[file] || fallbackMockAnatomy(file);
      return {
        file,
        comparisonId: 'none',
        hasBaseline: false,
        before: { label: null, sourceId: null, status: 'absent', anatomy: null, reason: 'No comparison selected' },
        after: anatomy
          ? { label: 'working copy', sourceId: anatomy.sourceId || `mock-source:${file}`, status: anatomy.status === 'unavailable' ? 'unavailable' : 'available', anatomy: cloneJson({ ...anatomy, sourceId: anatomy.sourceId || `mock-source:${file}` }), reason: anatomy.reason || null }
          : { label: 'working copy', sourceId: null, status: 'unavailable', anatomy: null, reason: 'Anatomy unavailable' },
      };
    }

    if (cmd === 'get_graph') {
      const request = {
        mode: 'investigation',
        seed: args?.since ? { kind: 'commit', revset: args.since } : { kind: 'working_copy' },
        aggregate: 'file',
      };
      return createMockGraphView(request, mockProjectionNodesForRequest(request)).graph;
    }

    if (cmd === 'get_changes') {
      if (readMockStorage('sloposcope.mockHistoryFailure', false)) throw new Error('Mock history unavailable');
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
    if (cmd === 'get_coordinator_state') {
      return cloneJson(normalizeCoordinatorState(mockCurrentRepoPath, ensureCoordinatorState()));
    }
    if (cmd === 'resume_mission_thread') {
      const requestedThreadId = String(args?.threadId || '').trim();
      if (!requestedThreadId) throw new Error('threadId is required.');
      const current = ensureCoordinatorState();
      const snapshot = current.threadSnapshots?.[requestedThreadId];
      if (!snapshot) throw new Error(`Coordinator thread not found: ${requestedThreadId}`);
      return cloneJson(replaceCoordinatorState(mockCurrentRepoPath, {
        ...snapshot,
        threads: current.threads,
        threadSnapshots: current.threadSnapshots,
      }));
    }
    if (cmd === 'start_coordinator_thread') {
      const current = ensureCoordinatorState();
      return cloneJson(replaceCoordinatorState(mockCurrentRepoPath, {
        ...createDefaultCoordinatorState(),
        threadId: `coordinator-${Date.now()}`,
        missionId: '',
        title: 'Unscoped mission',
        state: 'wake',
        messages: [
          createCoordinatorMessage(
            'coordinator',
            'wake_brief',
            buildWakeMessage(mockCurrentRepoPath, null, { title: 'Unscoped mission' }),
            { personaName: COORDINATOR_NAME },
          ),
        ],
        threads: current.threads,
        threadSnapshots: current.threadSnapshots,
      }));
    }
    if (cmd === 'get_mission_state') return cloneJson(cloneMissionState(ensureMissionState()));
    if (cmd === 'list_agent_profiles') {
      const state = ensureMissionState();
      return { profiles: cloneJson(state?.profiles || []) };
    }

    if (cmd === 'send_coordinator_message') {
      const content = String(args?.content || '').trim();
      if (!content) throw new Error('Coordinator message is required.');
      const missionState = ensureMissionState();
      const next = mutateCoordinatorState(mockCurrentRepoPath, (state) => {
        const messages = [...(state.messages || []), createCoordinatorMessage('user', 'prompt', content, { content })];
        const draftMission = {
          ...(state.draftMission || createDefaultCoordinatorState().draftMission),
        };
        let pendingPlan = Array.isArray(state.pendingPlan) ? [...state.pendingPlan] : [];
        let coordinatorReply = '';
        let replyKind = 'status';
        let nextState = state.state || 'wake';
        let shouldReplan = pendingPlan.length === 0;

        if (messageWantsReport(content)) {
          coordinatorReply = buildMissionReport(missionState);
          replyKind = 'report';
          return normalizeCoordinatorState(mockCurrentRepoPath, {
            ...state,
            messages: [...messages, createCoordinatorMessage('coordinator', replyKind, coordinatorReply, { report: true })],
          });
        }

        if (!draftMission.brief) {
          draftMission.brief = content;
          draftMission.title = draftMission.title || deriveProblemTitle(content);
        } else if (!messageIsValidationOnly(content)) {
          draftMission.brief = `${draftMission.brief}\n${content}`.trim();
          if (pendingPlan.length > 0) {
            pendingPlan = [];
            shouldReplan = true;
          }
        }

        if (!draftMission.validationCommand) {
          draftMission.validationCommand = extractValidationCommand(content);
        }

        if (shouldReplan) {
          pendingPlan = planMissionObjectives({
            title: draftMission.title,
            brief: draftMission.brief,
            validationCommand: draftMission.validationCommand,
          }).map((objective) => ({
            id: objective.id,
            title: objective.title,
            prompt: objective.prompt,
            scopePaths: Array.isArray(objective?.scope?.paths) ? objective.scope.paths : [],
          }));
          coordinatorReply = buildCoordinatorPlanMessage({ pendingPlan });
          replyKind = 'plan';
          nextState = 'plan_ready';
          return normalizeCoordinatorState(mockCurrentRepoPath, {
            ...state,
            missionId: missionState.missionId || state.missionId || '',
            title: draftMission.title || state.title || 'Unscoped mission',
            state: nextState,
            draftMission,
            pendingPlan,
            messages: [...messages, createCoordinatorMessage('coordinator', replyKind, coordinatorReply, { objectiveCount: pendingPlan.length })],
          });
        }

        coordinatorReply = 'Context appended. Approve current plan or add more detail.';
        return normalizeCoordinatorState(mockCurrentRepoPath, {
          ...state,
          title: draftMission.title || state.title || 'Unscoped mission',
          draftMission,
          pendingPlan,
          messages: [...messages, createCoordinatorMessage('coordinator', replyKind, coordinatorReply, { pendingPlan: pendingPlan.length })],
        });
      });
      return cloneJson(next);
    }

    if (cmd === 'approve_coordinator_plan') {
      const coordinator = ensureCoordinatorState();
      const draftMission = coordinator.draftMission || createDefaultCoordinatorState().draftMission;
      const pendingPlan = Array.isArray(coordinator.pendingPlan) ? coordinator.pendingPlan : [];
      if (!pendingPlan.length) throw new Error('No coordinator plan ready to approve.');
      if (!draftMission.title || !draftMission.brief) {
        throw new Error('Coordinator draft is incomplete.');
      }
      const missionId = deriveMissionId(draftMission, coordinator.missionId || '');
      const now = new Date().toISOString();
      const plannerProfile = findPlannerProfileById(ensureMissionState(), draftMission.plannerProfileId) || findPlannerProfile(ensureMissionState());
      const nextMission = mutateMissionState(mockCurrentRepoPath, (state) => ({
        ...state,
        missionId,
        title: draftMission.title,
        brief: draftMission.brief,
        validationCommand: draftMission.validationCommand,
        issueKey: String(draftMission.issueKey || '').trim(),
        issueUrl: String(draftMission.issueUrl || '').trim(),
        plannerProfileId: String(draftMission.plannerProfileId || ''),
        plannerProfileLabel: plannerProfile?.label || '',
        plannerNotes: String(draftMission.plannerNotes || ''),
        plannerPromptOverride: String(draftMission.plannerPromptOverride || ''),
        plannerSummary: plannerProfile ? `Mission planned with ${plannerProfile.label}.` : 'Mission planned with default planner prompt.',
        plannerStatus: 'planned',
        plannerMode: plannerProfile ? 'profile' : 'viewer_first',
        summary: missionSummaryFromInput(draftMission),
        status: 'awaiting_launch',
        objectives: pendingPlan.map((objective) => ({
          id: objective.id,
          title: objective.title,
          prompt: objective.prompt,
          expectedValidation: draftMission.validationCommand,
          scope: { paths: Array.isArray(objective.scopePaths) ? objective.scopePaths : [] },
          state: 'pending',
          latestSessionId: null,
        })),
        lastAction: 'approve_coordinator_plan',
        lastActionAt: now,
        lastPlannedAt: now,
      }));
      nextMission.eventLedger = [...(nextMission.eventLedger || []), createMissionEvent('plan', `Planned ${pendingPlan.length} objective(s) with ${plannerProfile?.provider || 'fixture'}.`, {
        objectiveIds: pendingPlan.map((objective) => objective.id),
        planner: plannerProfile?.provider || 'fixture',
      })];
      nextMission.events = nextMission.eventLedger;
      replaceMissionState(mockCurrentRepoPath, nextMission);

      const next = mutateCoordinatorState(mockCurrentRepoPath, (state) => ({
        ...state,
        missionId,
        title: draftMission.title,
        state: 'awaiting_launch',
        draftMission,
        pendingPlan: [],
        messages: [
          ...(state.messages || []),
          createCoordinatorMessage('coordinator', 'status', 'Plan approved. Ready to launch workers.', { missionId }),
        ],
      }));
      return cloneJson(next);
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
        const liveProfile = String(selectedProfile?.provider || '').trim() === 'local_live_cli';
        const sessionId = `code-session-${(state.sessions?.length || 0) + 1}`;
        const verificationId = `verification-${(state.verifications?.length || 0) + 1}`;
        const validationCommand = String(state.validationCommand || '');
        const validationPassed = !validationCommand.toLowerCase().includes('fail');
        const objectives = Array.isArray(state.objectives)
          ? state.objectives.map((objective, index) => {
              const matchesRequested = resolvedObjectiveId ? objective?.id === resolvedObjectiveId : index === 0;
              if (!matchesRequested) return objective;
              return {
                ...objective,
                state: liveProfile ? 'claimed' : validationPassed ? 'done' : 'failed',
                latestSessionId: sessionId,
              };
            })
          : [];
        const objective = objectives.find((entry) => entry?.latestSessionId === sessionId) || null;
        const workspacePath = `${mockCurrentRepoPath}/.sloposcope/worktrees/${state.missionId || 'mission'}/${objective?.id || 'objective-1'}`;
        const verification = liveProfile
          ? null
          : {
              verificationId,
              sessionId,
              objectiveId: objective?.id || null,
              objectiveTitle: objective?.title || null,
              profileId,
              kind: 'code_worker',
              status: validationPassed ? 'passed' : 'failed',
              summary: validationPassed
                ? (objective ? `Code worker completed ${objective.id}.` : 'Code worker ran.')
                : (objective ? `Code worker failed ${objective.id}.` : 'Code worker failed.'),
              checkedAt: now,
              command: validationCommand || null,
              workspacePath,
              patchSummary: objective ? `Patch artifact for ${objective.id}` : 'Patch artifact',
              patchArtifact: `diff --git a/src/mock_worker.js b/src/mock_worker.js\n+export const objective = '${objective?.id || 'objective-1'}';\n`,
              targetFile: 'src/mock_worker.js',
              validationStatus: validationPassed ? 'passed' : 'failed',
              reviewState: 'pending',
              applyState: 'not_applied',
              gateState: 'open',
              validation: {
                command: validationCommand || null,
                success: validationPassed,
                exitCode: validationPassed ? 0 : 1,
                stdout: validationPassed ? 'mock validation ok' : '',
                stderr: validationPassed ? '' : 'mock validation failed',
              },
            };
        const session = {
          sessionId,
          objectiveId: objective?.id || null,
          objectiveTitle: objective?.title || null,
          profileId,
          workerKind: 'code_worker',
          state: liveProfile ? 'running' : 'completed',
          runtimeStatus: liveProfile ? 'running' : 'completed',
          result: liveProfile ? 'running' : validationPassed ? 'passed' : 'failed',
          startedAt: now,
          updatedAt: now,
          verificationId: verificationId,
          latestEvent: liveProfile ? 'started' : objective ? 'completed' : 'idle',
          command: validationCommand || null,
          workspacePath,
          patchSummary: objective ? `Patch artifact for ${objective.id}` : 'Patch artifact',
          patchArtifact: verification?.patchArtifact || '',
          targetFile: 'src/mock_worker.js',
          touchedPaths: ['src/mock_worker.js'],
          validationStatus: liveProfile ? 'pending' : validationPassed ? 'passed' : 'failed',
          validation: verification?.validation || null,
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
          verifications: verification ? [...(state.verifications || []), verification] : [...(state.verifications || [])],
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
      const latestSession = next.sessions?.[next.sessions.length - 1] || null;
      next.eventLedger = [...(next.eventLedger || []), createMissionEvent(
        latestSession?.state === 'running' ? 'code_worker_started' : 'code_worker',
        latestSession?.state === 'running'
          ? `Code worker started ${objective?.id || 'objective-1'}.`
          : objective ? `Code worker completed ${objective.id}.` : 'Code worker ran.',
        {
          objectiveId: objective?.id || null,
          sessionId: latestSession?.sessionId || null,
          verificationId: next.verifications?.[next.verifications.length - 1]?.verificationId || null,
        },
      )];
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

    if (cmd === 'launch_coordinator_workers') {
      const coordinator = ensureCoordinatorState();
      const requestedProfileId = String(args?.profileId || '').trim();
      const missionId = String(args?.missionId || coordinator.missionId || '').trim();
      if (!missionId || missionId !== ensureMissionState().missionId) {
        const next = mutateCoordinatorState(mockCurrentRepoPath, (state) => ({
          ...state,
          messages: [
            ...(state.messages || []),
            createCoordinatorMessage('coordinator', 'status', 'No runnable objectives to launch.', { launched: 0, profileId: requestedProfileId || null }),
          ],
        }));
        return cloneJson(next);
      }
      let launched = 0;
      while (Number(deriveScheduler(ensureMissionState()).summary?.readyCount || 0) > 0) {
        invoke('run_next_code_worker', {
          missionId,
          profileId: requestedProfileId || null,
        });
        launched += 1;
        if (launched > 16) break;
      }
      const next = mutateCoordinatorState(mockCurrentRepoPath, (state) => ({
        ...state,
        missionId: ensureMissionState().missionId || state.missionId || '',
        messages: [
          ...(state.messages || []),
          createCoordinatorMessage(
            'coordinator',
            'status',
            launched > 0 ? `Workers launched. ${launched} objective(s) dispatched.` : 'No runnable objectives to launch.',
            { launched, profileId: requestedProfileId || null },
          ),
        ],
      }));
      return cloneJson(next);
    }

    if (cmd === 'request_coordinator_report') {
      const coordinator = ensureCoordinatorState();
      const missionId = String(args?.missionId || coordinator.missionId || '').trim();
      const next = mutateCoordinatorState(mockCurrentRepoPath, (state) => ({
        ...state,
        messages: [
          ...(state.messages || []),
          createCoordinatorMessage(
            'coordinator',
            'report',
            missionId && missionId === ensureMissionState().missionId
              ? buildMissionReport(ensureMissionState())
              : 'Mission report. No mission approved yet.',
            { report: true, missionId: missionId || null },
          ),
        ],
      }));
      return cloneJson(next);
    }

    if (cmd === 'nudge_worker_session') {
      const coordinator = ensureCoordinatorState();
      const current = ensureMissionState();
      const missionId = String(args?.missionId || coordinator.missionId || '').trim();
      const latestLive = missionId && missionId === current.missionId
        ? ((current.sessions || []).find((entry) => missionSessionLiveliness(entry) === 'live') || null)
        : null;
      const message = String(args?.content || '').trim() || 'Keep moving.';
      if (latestLive && String(latestLive?.profileId || '').includes('live-local')) {
        const now = new Date().toISOString();
        const nextMission = mutateMissionState(mockCurrentRepoPath, (state) => {
          const sessions = (state.sessions || []).map((entry) => (
            entry.sessionId === latestLive.sessionId
              ? {
                  ...entry,
                  state: 'completed',
                  runtimeStatus: 'completed',
                  result: 'passed',
                  latestEvent: 'completed',
                  validationStatus: 'passed',
                  updatedAt: now,
                }
              : entry
          ));
          const objectives = (state.objectives || []).map((objective) => (
            objective?.id === latestLive.objectiveId
              ? { ...objective, state: 'done' }
              : objective
          ));
          const verificationId = `verification-${(state.verifications?.length || 0) + 1}`;
          const verification = {
            verificationId,
            sessionId: latestLive.sessionId,
            objectiveId: latestLive.objectiveId || null,
            objectiveTitle: latestLive.objectiveTitle || null,
            profileId: latestLive.profileId,
            kind: 'code_worker',
            status: 'passed',
            summary: latestLive.objectiveId ? `Code worker completed ${latestLive.objectiveId}.` : 'Code worker completed.',
            checkedAt: now,
            command: state.validationCommand || null,
            workspacePath: latestLive.workspacePath,
            patchSummary: latestLive.objectiveId ? `Patch artifact for ${latestLive.objectiveId}` : 'Patch artifact',
            patchArtifact: `diff --git a/src/mock_worker.js b/src/mock_worker.js\n+export const objective = '${latestLive.objectiveId || 'objective-1'}';\n`,
            targetFile: 'src/mock_worker.js',
            validationStatus: 'passed',
            reviewState: 'pending',
            applyState: 'not_applied',
            gateState: 'open',
            validation: {
              command: state.validationCommand || null,
              success: true,
              exitCode: 0,
              stdout: `nudge accepted: ${message}`,
              stderr: '',
            },
          };
          const eventLedger = [...(state.eventLedger || []), createMissionEvent('code_worker_completed', verification.summary, {
            objectiveId: latestLive.objectiveId || null,
            sessionId: latestLive.sessionId,
            verificationId,
            nudge: message,
          })];
          return {
            ...state,
            objectives,
            sessions,
            verifications: [...(state.verifications || []), verification],
            eventLedger,
            events: eventLedger,
          };
        });
        replaceMissionState(mockCurrentRepoPath, nextMission);
      }
      const next = mutateCoordinatorState(mockCurrentRepoPath, (state) => ({
        ...state,
        messages: [
          ...(state.messages || []),
          createCoordinatorMessage(
            'coordinator',
            'nudge',
            latestLive ? `Nudge sent to ${latestLive.sessionId}. ${message}` : 'No live worker to nudge. Ask report or relaunch work.',
            { sessionId: latestLive?.sessionId || null, nudgeAccepted: Boolean(latestLive) },
          ),
        ],
      }));
      return cloneJson(next);
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
      if (action === 'apply') {
        projectAppliedWorkerSession((next.sessions || []).find((session) => session.sessionId === next.viewerSessionId));
      }
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
      return createMockGraph(mockProjectionNodes())
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

    if (cmd === 'get_reviewer_target_context') {
      const mode = String(args?.input?.mode || 'working_copy');
      const since = String(args?.input?.since || '@');
      return {
        repoPath: mockCurrentRepoPath,
        branch: 'main',
        revision: 'deadbeefdead',
        since,
        changedFilesTotal: 2,
        changedFilesTruncated: false,
        changedFiles: [
          {
            path: 'mock_helper.ex',
            status: 'modified',
            diffExcerpt: ['@@ -1,2 +1,3 @@', ' defmodule MockHelper do', '+  def ping, do: :pong', ' end'],
            diffLineCount: 4,
            excerptTruncated: false,
          },
          {
            path: 'mock.ex',
            status: mode === 'working_copy' ? 'modified' : 'added',
            diffExcerpt: ['@@ -1,2 +1,3 @@', ' defmodule MockModule do', '+  def goodbye, do: :moon', ' end'],
            diffLineCount: 4,
            excerptTruncated: false,
          },
        ],
        graphCounts: {
          available: true,
          entityCount: 2,
          edgeCount: 1,
          indexedFileCount: 2,
          changedFilesWithGraphEntities: 2,
          changedEntitiesCount: 2,
        },
        blastRadius: {
          available: true,
          seedFilesCount: 2,
          oneHopEntityCount: 2,
          twoHopEntityCount: 2,
          affectedEntitiesCount: 1,
          relatedEntitiesCount: 1,
          affectedEntities: ['MockHelper'],
          relatedEntities: ['MockModule'],
        },
      };
    }

    if (cmd === 'reviewer_generate_elm') {
      const question = String(args?.input?.question || '').trim();
      const kind = String(args?.input?.kind || 'branch').trim();
      const title = kind === 'question' && question ? `Review answer: ${question}` : 'Task review';
      return {
        source: `module Main exposing (main)

import Browser
import Html exposing (Html, div, h1, p, text)
import Html.Attributes exposing (class)


main : Program () () Never
main =
    Browser.sandbox { init = (), update = \\_ model -> model, view = view }


view : () -> Html Never
view _ =
    div [ class "generated-reviewer" ]
        [ h1 [] [ text ${JSON.stringify(title)} ]
        , p [] [ text "Mock reviewer_generate_elm source payload." ]
        ]`,
      };
    }

    if (cmd === 'compile_reviewer_elm') {
      return `
window.Elm = window.Elm || {};
window.Elm.Main = {
  init: function(opts) {
    var root = opts.node;
    root.innerHTML = '<div class="generated-reviewer">'
      + '<div class="generated-reviewer__nav">'
      + '<button class="generated-reviewer__tab generated-reviewer__tab--active">Review</button>'
      + '<button class="generated-reviewer__tab">Decision</button>'
      + '<button class="generated-reviewer__tab">Evidence</button>'
      + '<button class="generated-reviewer__tab">Flow</button>'
      + '</div>'
      + '<div class="generated-reviewer__slide">'
      + '<span class="generated-reviewer__kicker">Review</span>'
      + '<h1>Task review</h1>'
      + '<p>What changed, why it likely changed, and where reviewer should look first.</p>'
      + '<div class="generated-reviewer__stats"><div class="generated-reviewer__stat"><span>visible nodes</span><strong>2</strong></div><div class="generated-reviewer__stat"><span>visible edges</span><strong>1</strong></div></div>'
      + '<pre><code>defmodule MockModule do\\n  def hello, do: :world\\nend</code></pre>'
      + '</div>'
      + '</div>';
    return {};
  }
};`;
    }
    if (cmd === 'get_file_source') return 'defmodule MockModule do\n  def hello, do: :world\n  defp secret, do: :moon\nend';
    if (cmd === 'get_file_anatomy') {
      const file = String(args?.file || '');
      const fixture = readMockStorage('sloposcope.mockAnatomy', null)?.[file];
      if (fixture) {
        const delay = Number(readMockStorage('sloposcope.mockAnatomyDelay', 0));
        if (delay > 0) await new Promise(resolve => setTimeout(resolve, delay));
        return cloneJson(fixture);
      }
      if (file === 'mock.ex') {
        return {
          status: 'partial',
          file,
          language: 'ex',
          evidence: 'tree_sitter',
          callables: [
            { name: 'hello', kind: 'def', visibility: 'public', arity: null, startLine: null, endLine: null },
            { name: 'secret', kind: 'defp', visibility: 'private', arity: null, startLine: null, endLine: null },
          ],
          stateSlots: null,
          reason: 'Current language query captures callable names; missing fields remain unknown',
        };
      }
      return {
        status: 'unavailable',
        file,
        language: file.split('.').pop() || 'unknown',
        evidence: 'tree_sitter',
        callables: [],
        stateSlots: null,
        reason: 'no Tree-sitter anatomy profile for this extension',
      };
    }
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
    if (!mockListeners.has(event)) mockListeners.set(event, new Set());
    mockListeners.get(event).add(_cb);
    return () => {
      mockListeners.get(event)?.delete(_cb);
    };
  };

  return { invoke, listen };
}
