<script>
  import { getAgentProfileProviderHint } from './watchdogConductorOrchestrator.js';

  let {
    missionState = null,
    missionLoading = false,
    missionError = '',
    missionFixtureRunning = false,
    missionCodeWorkerRunning = false,
    missionReviewRunning = false,
    missionReviewAction = '',
    selectedCodeWorkerProfileId = $bindable(''),
    onRunCodeWorker,
    onRunNextCodeWorker,
    onRunFixtureWorker,
    onSweepStaleSessions,
    onAcceptRun,
    onRejectRun,
    onApplyRun,
    onGateRun,
    onReload,
  } = $props();

  function formatCount(value) {
    return Number(value || 0);
  }

  function latestSession() {
    const sessions = Array.isArray(missionState?.sessions) ? missionState.sessions : [];
    return sessions.length > 0 ? sessions[sessions.length - 1] : null;
  }

  function currentSessionId() {
    return missionState?.viewerSessionId || latestSession()?.sessionId || '';
  }

  function viewerProfile() {
    return missionState?.viewerProfileId || latestSession()?.profileId || 'viewer';
  }

  function reviewState() {
    return missionState?.reviewState || latestSession()?.reviewState || 'pending';
  }

  function applyState() {
    return missionState?.applyState || latestSession()?.applyState || 'not_applied';
  }

  function gateState() {
    return missionState?.gateState || latestSession()?.gateState || 'open';
  }

  function availableProfiles() {
    return Array.isArray(missionState?.profiles) ? missionState.profiles : [];
  }

  function profileHasCapability(profile, capability) {
    const wanted = String(capability || '').trim().toLowerCase();
    if (!wanted) return false;
    const capabilities = Array.isArray(profile?.capabilities) ? profile.capabilities : [];
    return capabilities.some((entry) => String(entry || '').trim().toLowerCase() === wanted);
  }

  function availableCodeWorkerProfiles() {
    return availableProfiles().filter((profile) => profileHasCapability(profile, 'can_edit'));
  }

  function selectedProfile() {
    return availableCodeWorkerProfiles().find((entry) => entry?.id === selectedCodeWorkerProfileId) || null;
  }

  function plannerProfile() {
    const plannerProfileId = String(missionState?.plannerProfileId || '').trim();
    return (
      availableProfiles().find((profile) => profile?.id === plannerProfileId)
      || availableProfiles().find((profile) => profile?.active && profileHasCapability(profile, 'can_plan'))
      || null
    );
  }

  function plannerHint() {
    return plannerProfile() ? getAgentProfileProviderHint(plannerProfile()?.provider) : null;
  }

  function scheduler() {
    return missionState?.scheduler || {
      readyCount: 0,
      blockedCount: 0,
      activeCount: 0,
      staleSessionCount: 0,
      nextObjectiveId: null,
    };
  }

  function displaySchedulerState(value) {
    const normalized = String(value || '').trim().toLowerCase();
    if (!normalized) return '-';
    if (normalized.startsWith('blocked')) return 'blocked';
    return normalized.replaceAll('_', ' ');
  }
</script>

<section class="eyeloss-control-card eyeloss-mission-runs-panel">
  <header class="eyeloss-control-card__header">
    <div>
      <div class="eyeloss-control-card__eyebrow">Runs</div>
      <h2 class="eyeloss-control-card__title">Mission Runs</h2>
    </div>
    <div class="eyeloss-control-card__meta">{missionState?.status || 'idle'}</div>
  </header>

  <div class="eyeloss-control-card__meta-grid">
    <span>objectives: {formatCount(missionState?.objectives?.length)}</span>
    <span>sessions: {formatCount(missionState?.sessions?.length)}</span>
    <span>verifications: {formatCount(missionState?.verifications?.length)}</span>
    <span>events: {formatCount(missionState?.eventLedger?.length)}</span>
    <span>last run: {missionState?.lastRunAt || '-'}</span>
    <span>last check: {missionState?.lastCheckedAt || '-'}</span>
    <span>state: {missionState?.status || 'idle'}</span>
  </div>

  {#if missionError}
    <div class="eyeloss-control-card__error">{missionError}</div>
  {/if}

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Viewer-first review</div>
    <div class="eyeloss-control-card__hint">Latest session drives review, apply, and gate. Mock state mirrors backend state.</div>
    <div class="eyeloss-control-card__meta-grid">
      <span>viewer profile: {viewerProfile()}</span>
      <span>session: {currentSessionId() || '-'}</span>
      <span>review: {reviewState()}</span>
      <span>apply: {applyState()}</span>
      <span>gate: {gateState()}</span>
      <span>last action: {missionState?.lastAction || '-'}</span>
    </div>
    <div class="eyeloss-control-actions">
      <button
        type="button"
        class="btn btn-primary"
        onclick={() => onAcceptRun?.(currentSessionId())}
        disabled={missionLoading || missionReviewRunning || missionFixtureRunning || missionCodeWorkerRunning || reviewState() !== 'pending'}
      >
        {missionReviewRunning && missionReviewAction === 'accept' ? 'Accepting...' : 'Accept'}
      </button>
      <button
        type="button"
        class="btn"
        onclick={() => onRejectRun?.(currentSessionId())}
        disabled={missionLoading || missionReviewRunning || missionFixtureRunning || missionCodeWorkerRunning || reviewState() !== 'pending'}
      >
        {missionReviewRunning && missionReviewAction === 'reject' ? 'Rejecting...' : 'Reject'}
      </button>
      <button
        type="button"
        class="btn"
        onclick={() => onApplyRun?.(currentSessionId())}
        disabled={missionLoading || missionReviewRunning || missionFixtureRunning || missionCodeWorkerRunning || reviewState() !== 'accepted' || gateState() !== 'ready' || applyState() === 'applied'}
      >
        {missionReviewRunning && missionReviewAction === 'apply' ? 'Applying...' : 'Apply'}
      </button>
      <button
        type="button"
        class="btn"
        onclick={() => onGateRun?.(currentSessionId())}
        disabled={missionLoading || missionReviewRunning || missionFixtureRunning || missionCodeWorkerRunning || reviewState() !== 'accepted' || applyState() === 'applied'}
      >
        {missionReviewRunning && missionReviewAction === 'gate' ? 'Gating...' : 'Gate'}
      </button>
    </div>
  </div>

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Code worker profile</div>
    <label class="eyeloss-control-field">
      <span>Code worker profile</span>
      <select class="input-mono" bind:value={selectedCodeWorkerProfileId}>
        <option value="">Automatic fallback</option>
        {#each availableCodeWorkerProfiles() as profile}
          <option value={profile.id}>{profile.label} ({profile.provider})</option>
        {/each}
      </select>
    </label>
    <div class="eyeloss-control-card__hint">
      {#if selectedProfile()}
        selected: {selectedProfile().command || selectedProfile().provider}
        {#if selectedProfile().model}
          {' | '}model: {selectedProfile().model}
        {/if}
      {:else}
        No saved profile selected. Backend fallback stays available.
      {/if}
    </div>
  </div>

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Mission summary</div>
    <div class="eyeloss-control-card__hint">{missionState?.summary || missionState?.brief || 'No mission summary yet.'}</div>
    <div class="eyeloss-control-card__hint">{missionState?.plannerSummary || 'viewer-first planner ready'}</div>
    <div class="eyeloss-control-card__meta-grid">
      <span>planner profile: {missionState?.plannerProfileId || '-'}</span>
      <span>planner mode: {missionState?.plannerMode || 'viewer_first'}</span>
      <span>planner status: {missionState?.plannerStatus || 'idle'}</span>
    </div>
    {#if plannerHint()}
      <div class="eyeloss-control-card__hint">planner hint: {plannerHint().summary}</div>
      {#if plannerHint()?.envVars?.length}
        <div class="eyeloss-mission__item-meta">planner env: {plannerHint().envVars.join(', ')}</div>
      {/if}
    {/if}
    {#if missionState?.plannerNotes}
      <div class="eyeloss-control-card__hint">planner notes: {missionState.plannerNotes}</div>
    {/if}
    {#if missionState?.plannerPromptOverride}
      <div class="eyeloss-control-card__hint">prompt override: {missionState.plannerPromptOverride}</div>
    {/if}
  </div>

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Scheduler</div>
    <div class="eyeloss-control-card__hint">ready next objective only. scope conflicts and stale claims block queue.</div>
    <div class="eyeloss-control-card__meta-grid">
      <span>ready: {formatCount(scheduler().readyCount)}</span>
      <span>blocked: {formatCount(scheduler().blockedCount)}</span>
      <span>active: {formatCount(scheduler().activeCount)}</span>
      <span>stale: {formatCount(scheduler().staleSessionCount)}</span>
      <span>next objective: {scheduler().nextObjectiveId || '-'}</span>
    </div>
  </div>

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Objectives</div>
    {#if missionState?.objectives?.length}
      <div class="eyeloss-mission__stack">
        {#each missionState.objectives as objective}
          <article class="eyeloss-mission__item">
            <div class="eyeloss-mission__item-header">
              <span class="eyeloss-mission__item-id">{objective.id}</span>
              <span class="eyeloss-mission__item-state">{objective.state || 'pending'}</span>
            </div>
            <div class="eyeloss-mission__item-title">{objective.title}</div>
            <div class="eyeloss-control-card__hint">{objective.prompt}</div>
            <div class="eyeloss-mission__item-meta">
              scheduler: {displaySchedulerState(objective.schedulerState || 'ready')}
              {#if objective.schedulerReason}
                {' | '}reason: {objective.schedulerReason}
              {/if}
            </div>
            {#if objective.expectedValidation}
              <div class="eyeloss-mission__item-meta">validation: {objective.expectedValidation}</div>
            {/if}
          </article>
        {/each}
      </div>
    {:else}
      <div class="eyeloss-control-card__hint">No objectives planned.</div>
    {/if}
  </div>

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Sessions</div>
    {#if missionState?.sessions?.length}
      <div class="eyeloss-mission__stack">
        {#each missionState.sessions as session}
          <article class="eyeloss-mission__item">
            <div class="eyeloss-mission__item-header">
              <span class="eyeloss-mission__item-id">{session.sessionId}</span>
              <span class="eyeloss-mission__item-state">{session.state || 'unknown'}</span>
            </div>
            <div class="eyeloss-mission__item-title">{session.profileId || 'fixture_worker'}</div>
            <div class="eyeloss-control-card__hint">
              objective: {session.objectiveId || '-'} | result: {session.result || session.state || '-'} | started: {session.startedAt || '-'}
            </div>
            {#if session.runtimeStatus || session.heartbeatAgeSec != null || session.eventAgeSec != null || session.leaseExpired}
              <div class="eyeloss-control-card__hint">
                runtime: {session.runtimeStatus || session.state || 'unknown'}
                {#if session.heartbeatAgeSec != null}
                  {' | '}heartbeat age: {session.heartbeatAgeSec}s
                {/if}
                {#if session.eventAgeSec != null}
                  {' | '}event age: {session.eventAgeSec}s
                {/if}
                {#if session.leaseExpired}
                  {' | '}lease expired
                {/if}
              </div>
            {/if}
            {#if session.schedulerState || session.schedulerReason}
              <div class="eyeloss-control-card__hint">
                scheduler: {displaySchedulerState(session.schedulerState || '-')}
                {#if session.schedulerReason}
                  {' | '}reason: {session.schedulerReason}
                {/if}
              </div>
            {/if}
            {#if session.objectiveTitle || session.verificationId || session.command || session.workspacePath || session.patchSummary}
              <div class="eyeloss-mission__item-meta">
                {#if session.objectiveTitle}
                  objective title: {session.objectiveTitle}
                {/if}
                {#if session.verificationId}
                  {session.objectiveTitle ? ' | ' : ''}verification: {session.verificationId}
                {/if}
                {#if session.command}
                  {(session.objectiveTitle || session.verificationId) ? ' | ' : ''}command: {session.command}
                {/if}
              </div>
            {/if}
            {#if session.workspacePath}
              <div class="eyeloss-control-card__hint">worktree: {session.workspacePath}</div>
            {/if}
            {#if session.patchSummary || session.patchPath}
              <div class="eyeloss-control-card__hint">
                {session.patchSummary || 'Patch artifact'}{session.patchPath ? ` | ${session.patchPath}` : ''}
              </div>
            {/if}
            {#if session.command || session.validationStatus}
              <div class="eyeloss-control-card__hint">
                Validation: {session.command || session.validation?.command || session.validationStatus}
                {#if session.validationStatus && session.validationStatus !== session.command}
                  {' | '}status: {session.validationStatus}
                {/if}
              </div>
            {/if}
          </article>
        {/each}
      </div>
    {:else}
      <div class="eyeloss-control-card__hint">No sessions yet.</div>
    {/if}
  </div>

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Verification records</div>
    {#if missionState?.verifications?.length}
      <div class="eyeloss-mission__stack">
        {#each missionState.verifications as verification}
          <article class="eyeloss-mission__item">
            <div class="eyeloss-mission__item-header">
              <span class="eyeloss-mission__item-id">{verification.verificationId}</span>
              <span class="eyeloss-mission__item-state">{verification.status || 'unknown'}</span>
            </div>
            <div class="eyeloss-mission__item-title">{verification.summary || verification.kind || 'Verification'}</div>
            <div class="eyeloss-control-card__hint">
              session: {verification.sessionId || '-'} | objective: {verification.objectiveId || '-'} | checked: {verification.checkedAt || '-'}
            </div>
            {#if verification.objectiveTitle || verification.command || verification.profileId}
              <div class="eyeloss-mission__item-meta">
                {#if verification.objectiveTitle}
                  objective title: {verification.objectiveTitle}
                {/if}
                {#if verification.command}
                  {verification.objectiveTitle ? ' | ' : ''}command: {verification.command}
                {/if}
                {#if verification.profileId}
                  {(verification.objectiveTitle || verification.command) ? ' | ' : ''}profile: {verification.profileId}
                {/if}
              </div>
            {/if}
            {#if verification.workspacePath}
              <div class="eyeloss-control-card__hint">worktree: {verification.workspacePath}</div>
            {/if}
            {#if verification.patchSummary || verification.patchPath || verification.targetFile}
              <div class="eyeloss-control-card__hint">
                {verification.patchSummary || 'Patch artifact'}
                {#if verification.targetFile}
                  {' | '}target: {verification.targetFile}
                {/if}
                {#if verification.patchPath}
                  {' | '}{verification.patchPath}
                {/if}
              </div>
            {/if}
            {#if verification.command || verification.validationStatus}
              <div class="eyeloss-control-card__hint">
                Validation: {verification.command || verification.validation?.command || verification.validationStatus}
                {#if verification.validationStatus && verification.validationStatus !== verification.command}
                  {' | '}status: {verification.validationStatus}
                {/if}
              </div>
            {/if}
          </article>
        {/each}
      </div>
    {:else}
      <div class="eyeloss-control-card__hint">No verification records yet.</div>
    {/if}
  </div>

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Event ledger</div>
    {#if missionState?.eventLedger?.length}
      <div class="eyeloss-mission__ledger">
        {#each [...missionState.eventLedger].slice().reverse() as event}
          <div class="eyeloss-mission__ledger-row">
            <span class="eyeloss-mission__ledger-time">{event.at}</span>
            <span class="eyeloss-mission__ledger-kind">{event.kind || 'event'}</span>
            <span class="eyeloss-mission__ledger-summary">{event.summary || event.message || '-'}</span>
          </div>
        {/each}
      </div>
    {:else}
      <div class="eyeloss-control-card__hint">No mission events yet.</div>
    {/if}
  </div>

  <div class="eyeloss-control-actions">
    <button type="button" class="btn btn-primary" onclick={onRunNextCodeWorker} disabled={missionLoading || missionCodeWorkerRunning || missionFixtureRunning || !scheduler().nextObjectiveId}>
      {missionCodeWorkerRunning ? 'Writing...' : 'Run next code worker'}
    </button>
    <button type="button" class="btn btn-primary" onclick={onRunCodeWorker} disabled={missionLoading || missionCodeWorkerRunning || missionFixtureRunning}>
      {missionCodeWorkerRunning ? 'Writing...' : 'Run code worker'}
    </button>
    <button type="button" class="btn" onclick={onRunFixtureWorker} disabled={missionLoading || missionFixtureRunning || missionCodeWorkerRunning}>
      {missionFixtureRunning ? 'Running...' : 'Run fixture worker'}
    </button>
    <button type="button" class="btn" onclick={onSweepStaleSessions} disabled={missionLoading || missionFixtureRunning || missionCodeWorkerRunning || missionReviewRunning}>
      Sweep stale
    </button>
    <button type="button" class="btn" onclick={onReload} disabled={missionLoading || missionFixtureRunning || missionCodeWorkerRunning}>Reload</button>
  </div>
</section>
