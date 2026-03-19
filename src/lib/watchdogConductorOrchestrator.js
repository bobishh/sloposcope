export function extractErrorMessage(error) {
  if (!error) return '';
  if (typeof error === 'string') return error;
  if (typeof error === 'object') {
    if (typeof error.message === 'string') return error.message;
    try {
      return JSON.stringify(error);
    } catch (_e) {
      return String(error);
    }
  }
  return String(error);
}

export function toOptionalString(value) {
  const normalized = String(value ?? '').trim();
  return normalized === '' ? null : normalized;
}

export function parseMultilineInput(value) {
  return String(value ?? '')
    .split('\n')
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
}

export function parseCommaInput(value) {
  return String(value ?? '')
    .split(',')
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
}

export function joinMultiline(values) {
  if (!Array.isArray(values)) return '';
  return values.join('\n');
}

export function joinComma(values) {
  if (!Array.isArray(values)) return '';
  return values.join(', ');
}

function isJsonObject(value) {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

export function formatJsonObjectInput(value) {
  if (!isJsonObject(value)) return '{}';
  return JSON.stringify(value, null, 2);
}

export function parseJsonObjectInput(value, label = 'JSON') {
  const raw = String(value ?? '').trim();
  if (raw === '') return {};
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch (_error) {
    throw new Error(`${label} must be valid JSON.`);
  }
  if (!isJsonObject(parsed)) {
    throw new Error(`${label} must be a JSON object.`);
  }
  return parsed;
}

export function validateJsonObjectInput(value, label = 'JSON') {
  try {
    parseJsonObjectInput(value, label);
    return '';
  } catch (error) {
    return extractErrorMessage(error) || `${label} is invalid.`;
  }
}

const DEFAULT_AGENT_PROFILE_PROVIDER_HINT = {
  id: 'generic',
  title: 'Shell env first',
  summary: 'Worker inherits shell env. Env JSON adds or overrides vars for this profile only.',
  envVars: [],
};

const AGENT_PROFILE_PROVIDER_HINTS = {
  nim: {
    id: 'nim',
    title: 'NVIDIA NIM token hint',
    summary:
      'Hosted NIM chat endpoint is OpenAI-compatible. Use NVIDIA_API_KEY or NGC_API_KEY. NVIDIA_NIM_BASE_URL can override hosted endpoint.',
    envVars: ['NVIDIA_API_KEY', 'NGC_API_KEY', 'NVIDIA_NIM_API_KEY', 'NVIDIA_NIM_BASE_URL'],
  },
  opencode_nim: {
    id: 'opencode_nim',
    title: 'OpenCode on NIM quick start',
    summary:
      'Paste key. Run horse. Set NVIDIA_API_KEY for hosted NIM. NVIDIA_NIM_BASE_URL can pin alternate gateway.',
    envVars: ['NVIDIA_API_KEY', 'NGC_API_KEY', 'NVIDIA_NIM_API_KEY', 'NVIDIA_NIM_BASE_URL'],
  },
  pi: {
    id: 'pi',
    title: 'Use OpenAI-compatible NIM routing',
    summary: 'PI on NIM via OpenAI-compatible NIM endpoint.',
    envVars: ['NVIDIA_API_KEY', 'NVIDIA_NIM_BASE_URL', 'OPENAI_API_KEY', 'OPENAI_BASE_URL'],
  },
  gemini: {
    id: 'gemini',
    title: 'Gemini token hint',
    summary:
      'Use GEMINI_API_KEY for Gemini API auth. Vertex flow usually needs GOOGLE_API_KEY and GOOGLE_GENAI_USE_VERTEXAI. GOOGLE_CLOUD_PROJECT often pairs with paid org setups.',
    envVars: ['GEMINI_API_KEY', 'GOOGLE_API_KEY', 'GOOGLE_GENAI_USE_VERTEXAI', 'GOOGLE_CLOUD_PROJECT'],
  },
  codex: {
    id: 'codex',
    title: 'Codex token hint',
    summary:
      'Codex CLI can sign in with ChatGPT or use OPENAI_API_KEY. OPENAI_BASE_URL matters for proxy or gateway routing.',
    envVars: ['OPENAI_API_KEY', 'OPENAI_BASE_URL'],
  },
  claude: {
    id: 'claude',
    title: 'Claude token hint',
    summary:
      'Claude Code usually reads ANTHROPIC_API_KEY. Gateway or bearer setups can use ANTHROPIC_AUTH_TOKEN. ANTHROPIC_BASE_URL overrides endpoint.',
    envVars: ['ANTHROPIC_API_KEY', 'ANTHROPIC_AUTH_TOKEN', 'ANTHROPIC_BASE_URL'],
  },
  openai: {
    id: 'openai',
    title: 'OpenAI token hint',
    summary:
      'OpenAI clients usually read OPENAI_API_KEY. OPENAI_BASE_URL is common for proxies or compatible gateways.',
    envVars: ['OPENAI_API_KEY', 'OPENAI_BASE_URL'],
  },
  local: {
    id: 'local',
    title: 'Local token hint',
    summary: 'No provider token expected. Env JSON only if wrapper script needs extra vars.',
    envVars: [],
  },
};

export function normalizeAgentProfileProvider(provider) {
  const normalized = String(provider || '')
    .trim()
    .toLowerCase()
    .replaceAll(/[^a-z0-9]+/g, '_');
  if (!normalized) return 'generic';
  if (normalized.includes('opencode') && normalized.includes('nim')) return 'opencode_nim';
  if (normalized === 'pi_cli' || normalized.startsWith('pi_')) return 'pi';
  if (normalized.includes('nim') || normalized.includes('nvidia')) return 'nim';
  if (normalized.includes('gemini')) return 'gemini';
  if (normalized.includes('codex')) return 'codex';
  if (normalized.includes('claude')) return 'claude';
  if (normalized.includes('openai')) return 'openai';
  if (normalized.includes('local')) return 'local';
  return 'generic';
}

export function getAgentProfileProviderHint(provider) {
  const id = normalizeAgentProfileProvider(provider);
  return AGENT_PROFILE_PROVIDER_HINTS[id] || DEFAULT_AGENT_PROFILE_PROVIDER_HINT;
}

export function formatWatchdogStatus(status) {
  const value = String(status || 'insufficient_context');
  return value.replaceAll('_', ' ').toUpperCase();
}

export function parseConductorNumber(value, fallback, min, max) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.max(min, Math.min(max, Math.round(parsed)));
}

export function buildWatchdogIssueContextFromForm(form) {
  return {
    issue: {
      id: String(form.issueId || '').trim(),
      title: String(form.issueTitle || '').trim(),
      identifier: toOptionalString(form.issueIdentifier),
      url: toOptionalString(form.issueUrl),
    },
    summary: toOptionalString(form.issueSummary),
    acceptanceCriteria: parseMultilineInput(form.acceptanceCriteria),
    nonGoals: parseMultilineInput(form.nonGoals),
    labels: parseCommaInput(form.issueLabels),
    priority: toOptionalString(form.issuePriority),
  };
}

export function deriveWatchdogFormValues(view, createDefaultWatchdogState) {
  const normalized = view || createDefaultWatchdogState();
  return {
    state: normalized,
    form: {
      issueId: normalized.activeIssueContext?.issue?.id || normalized.activeIssue?.id || '',
      issueTitle: normalized.activeIssueContext?.issue?.title || normalized.activeIssue?.title || '',
      issueIdentifier:
        normalized.activeIssueContext?.issue?.identifier || normalized.activeIssue?.identifier || '',
      issueUrl: normalized.activeIssueContext?.issue?.url || normalized.activeIssue?.url || '',
      issueSummary: normalized.activeIssueContext?.summary || '',
      issueLabels: joinComma(normalized.activeIssueContext?.labels || []),
      issuePriority: normalized.activeIssueContext?.priority || '',
      acceptanceCriteria: joinMultiline(normalized.activeIssueContext?.acceptanceCriteria || []),
      nonGoals: joinMultiline(normalized.activeIssueContext?.nonGoals || []),
    },
  };
}

export function deriveConductorWorkflowValues(view, createDefaultConductorWorkflowLoad) {
  const normalized = view || createDefaultConductorWorkflowLoad();
  const frontMatter = normalized?.contract?.frontMatter;
  if (!frontMatter) {
    return {
      load: normalized,
      form: {
        provider: 'coding_agent_app_server',
        providerModel: '',
        timeoutSeconds: '900',
        traceRetention: '30',
        strictMode: false,
      },
    };
  }

  return {
    load: normalized,
    form: {
      provider: String(frontMatter.provider || 'coding_agent_app_server'),
      providerModel: String(frontMatter.providerModel || ''),
      timeoutSeconds: String(frontMatter.timeoutSeconds ?? 900),
      traceRetention: String(frontMatter.traceRetention ?? 30),
      strictMode: Boolean(frontMatter.providerStrictMode),
    },
  };
}

export async function fetchWatchdogState(invokeTraced, includeTraces = true) {
  const stateView = await invokeTraced('get_watchdog_state', {}, { warnAfterMs: 5000 });
  let traces = [];
  if (includeTraces) {
    const traceView = await invokeTraced('get_watchdog_traces', { limit: 10 }, { warnAfterMs: 5000 });
    traces = Array.isArray(traceView) ? traceView : [];
  }
  return { stateView, traces };
}

export async function saveWatchdogIssueContext(invokeTraced, issueContext) {
  return invokeTraced('set_watchdog_active_issue_context', { issueContext }, { warnAfterMs: 5000 });
}

export async function clearWatchdogIssueContext(invokeTraced) {
  return invokeTraced('set_watchdog_active_issue_context', { issueContext: null }, { warnAfterMs: 5000 });
}

export async function recordWatchdogValidation(invokeTraced, payload) {
  return invokeTraced('record_watchdog_validation', { validation: payload }, { warnAfterMs: 5000 });
}

export async function runWatchdogValidationCommand(invokeTraced, kind, command) {
  return invokeTraced(
    'run_watchdog_validation_command',
    { kind, command },
    { warnAfterMs: 120000 },
  );
}

export async function triggerWatchdogCheck(invokeTraced) {
  return invokeTraced('check_watchdog_focus', {}, { warnAfterMs: 8000 });
}

export async function fetchConductorStatus(invokeTraced, traceLimit, runTick = false) {
  if (runTick) {
    const response = await invokeTraced('conductor_refresh', { updates: [] }, { warnAfterMs: 15000 });
    return response?.status;
  }
  return invokeTraced('conductor_get_status', { traceLimit }, { warnAfterMs: 8000 });
}

export async function fetchConductorWorkflow(invokeTraced) {
  return invokeTraced('get_conductor_workflow_contract', {}, { warnAfterMs: 5000 });
}

export async function saveConductorWorkflowSettings(invokeTraced, settings) {
  return invokeTraced('save_conductor_workflow_settings', { settings }, { warnAfterMs: 8000 });
}

export async function enqueueConductorTask(invokeTraced, taskId, maxRetries) {
  return invokeTraced('conductor_enqueue_task', { taskId, maxRetries }, { warnAfterMs: 8000 });
}

export async function prepareConductorLaunch(invokeTraced, taskId) {
  return invokeTraced('conductor_prepare_launch', { taskId }, { warnAfterMs: 10000 });
}

export async function ingestConductorRunnerEvent(invokeTraced, taskId, rawEvent) {
  return invokeTraced('conductor_ingest_runner_event', { taskId, rawEvent }, { warnAfterMs: 8000 });
}

export async function dispatchConductorLaunch(invokeTraced, taskId) {
  return invokeTraced('conductor_dispatch_launch', { taskId }, { warnAfterMs: 30000 });
}

export function buildMissionInputFromForm(form) {
  return {
    title: String(form?.title || '').trim(),
    brief: String(form?.brief || '').trim(),
    validationCommand: String(form?.validationCommand || '').trim(),
    issueKey: String(form?.issueKey || '').trim(),
    issueUrl: String(form?.issueUrl || '').trim(),
    plannerProfileId: String(form?.plannerProfileId || '').trim(),
    plannerNotes: String(form?.plannerNotes || '').trim(),
    plannerPromptOverride: String(form?.plannerPromptOverride || '').trim(),
  };
}

export function buildAgentProfileInputFromForm(form) {
  return {
    id: toOptionalString(form?.id),
    label: String(form?.label || '').trim(),
    provider: String(form?.provider || '').trim(),
    model: toOptionalString(form?.model),
    command: toOptionalString(form?.command),
    args: parseMultilineInput(form?.args),
    env: parseJsonObjectInput(form?.envJson ?? form?.env, 'Env JSON'),
    capabilities: parseMultilineInput(form?.capabilities),
    transportKind: String(form?.transportKind || 'pty').trim() || 'pty',
    active: Boolean(form?.active),
  };
}

export function deriveMissionFormValues(view, createDefaultMissionState) {
  const normalized = view || createDefaultMissionState();
  return {
    state: normalized,
    planner: {
      profileLabel: String(normalized?.plannerProfileLabel || ''),
      summary: String(normalized?.plannerSummary || 'viewer-first planner ready'),
      status: String(normalized?.plannerStatus || 'idle'),
      mode: String(normalized?.plannerMode || 'viewer_first'),
    },
    form: {
      title: String(normalized?.title || ''),
      brief: String(normalized?.brief || ''),
      validationCommand: String(normalized?.validationCommand || ''),
      issueKey: String(normalized?.issueKey || ''),
      issueUrl: String(normalized?.issueUrl || ''),
      plannerProfileId: String(normalized?.plannerProfileId || ''),
      plannerNotes: String(normalized?.plannerNotes || ''),
      plannerPromptOverride: String(normalized?.plannerPromptOverride || ''),
      plannerSummary: String(normalized?.plannerSummary || 'viewer-first planner ready'),
      plannerStatus: String(normalized?.plannerStatus || 'idle'),
      plannerMode: String(normalized?.plannerMode || 'viewer_first'),
    },
  };
}

export async function fetchMissionState(invokeTraced) {
  return invokeTraced('get_mission_state', {}, { warnAfterMs: 5000 });
}

export async function saveMissionInput(invokeTraced, missionInput) {
  return invokeTraced('save_mission_input', { missionInput }, { warnAfterMs: 8000 });
}

export async function planMission(invokeTraced, missionInput) {
  return invokeTraced('plan_mission', { missionInput }, { warnAfterMs: 8000 });
}

export async function runFixtureWorker(invokeTraced, missionId, objectiveId) {
  return invokeTraced('run_fixture_worker', { missionId, objectiveId }, { warnAfterMs: 30000 });
}

export async function runCodeWorker(invokeTraced, missionId, objectiveId) {
  return invokeTraced('run_code_worker', { missionId, objectiveId }, { warnAfterMs: 30000 });
}

export async function runNextCodeWorker(invokeTraced, missionId, profileId) {
  return invokeTraced('run_next_code_worker', { missionId, profileId }, { warnAfterMs: 30000 });
}

export async function sweepStaleSessions(invokeTraced, missionId) {
  return invokeTraced('sweep_stale_sessions', { missionId }, { warnAfterMs: 15000 });
}

export async function fetchAgentProfiles(invokeTraced) {
  return invokeTraced('list_agent_profiles', {}, { warnAfterMs: 5000 });
}

export async function saveAgentProfile(invokeTraced, profile) {
  return invokeTraced('save_agent_profile', { profile }, { warnAfterMs: 8000 });
}

export async function deleteAgentProfile(invokeTraced, profileId) {
  return invokeTraced('delete_agent_profile', { profileId }, { warnAfterMs: 8000 });
}

export async function runCodeWorkerWithProfile(invokeTraced, missionId, objectiveId, profileId) {
  return invokeTraced('run_code_worker', { missionId, objectiveId, profileId }, { warnAfterMs: 30000 });
}
