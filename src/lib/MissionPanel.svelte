<script>
  import { getAgentProfileProviderHint } from './watchdogConductorOrchestrator.js';

  let {
    missionState = null,
    missionLoading = false,
    missionSaving = false,
    missionPlanning = false,
    missionCheckRunning = false,
    missionFixtureRunning = false,
    missionError = '',
    missionTitle = $bindable(''),
    missionBrief = $bindable(''),
    missionValidationCommand = $bindable(''),
    missionIssueKey = $bindable(''),
    missionIssueUrl = $bindable(''),
    missionPlannerProfileId = $bindable(''),
    missionPlannerNotes = $bindable(''),
    missionPlannerPromptOverride = $bindable(''),
    onSaveMission,
    onPlanMission,
    onRunCheck,
    onReload,
  } = $props();

  function formatCount(value) {
    return Number(value || 0);
  }

  function availablePlannerProfiles() {
    return Array.isArray(missionState?.profiles) ? missionState.profiles.filter((profile) => {
      const capabilities = Array.isArray(profile?.capabilities) ? profile.capabilities : [];
      return capabilities.some((entry) => String(entry || '').trim().toLowerCase() === 'can_plan');
    }) : [];
  }

  function plannerLabel() {
    return availablePlannerProfiles().find((profile) => profile?.id === missionPlannerProfileId)?.label || '';
  }

  function plannerProfile() {
    const profiles = availablePlannerProfiles();
    return (
      profiles.find((profile) => profile?.id === missionPlannerProfileId)
      || profiles.find((profile) => profile?.id === missionState?.plannerProfileId)
      || profiles.find((profile) => profile?.active)
      || null
    );
  }

  function plannerHint() {
    return plannerProfile() ? getAgentProfileProviderHint(plannerProfile()?.provider) : null;
  }
</script>

<section class="eyeloss-control-card eyeloss-mission-panel">
  <header class="eyeloss-control-card__header">
    <div>
      <div class="eyeloss-control-card__eyebrow">Mission</div>
      <h2 class="eyeloss-control-card__title">Mission Brief</h2>
    </div>
    <div class="eyeloss-control-card__meta">{missionState?.status || 'idle'}</div>
  </header>

  <div class="eyeloss-control-card__meta-grid">
    <span>objectives: {formatCount(missionState?.objectives?.length)}</span>
    <span>sessions: {formatCount(missionState?.sessions?.length)}</span>
    <span>events: {formatCount(missionState?.eventLedger?.length)}</span>
    <span>saved: {missionState?.lastSavedAt || '-'}</span>
    <span>planned: {missionState?.lastPlannedAt || '-'}</span>
    <span>checked: {missionState?.lastCheckedAt || '-'}</span>
    <span>planner: {plannerLabel() || missionState?.plannerProfileLabel || '-'}</span>
  </div>

  {#if missionError}
    <div class="eyeloss-control-card__error">{missionError}</div>
  {/if}

  <div class="eyeloss-form-grid eyeloss-form-grid--two">
    <label class="eyeloss-control-field eyeloss-control-field--wide">
      <span>Title</span>
      <input class="input-mono" type="text" bind:value={missionTitle} placeholder="Stabilize viewer-first mission UI" />
    </label>
    <label class="eyeloss-control-field eyeloss-control-field--wide">
      <span>Brief</span>
      <textarea class="input-mono eyeloss-control-field__textarea" bind:value={missionBrief} rows="4" placeholder="What should happen and why?"></textarea>
    </label>
    <label class="eyeloss-control-field">
      <span>Validation command</span>
      <input class="input-mono" type="text" bind:value={missionValidationCommand} placeholder="npm run -s build && cargo test --quiet" />
    </label>
    <label class="eyeloss-control-field">
      <span>Issue key</span>
      <input class="input-mono" type="text" bind:value={missionIssueKey} placeholder="LIN-123" />
    </label>
    <label class="eyeloss-control-field eyeloss-control-field--wide">
      <span>Issue URL</span>
      <input class="input-mono" type="text" bind:value={missionIssueUrl} placeholder="https://linear.app/..." />
    </label>
    <label class="eyeloss-control-field">
      <span>Planner profile</span>
      <select class="input-mono" bind:value={missionPlannerProfileId}>
        <option value="">Auto planner</option>
        {#each availablePlannerProfiles() as profile}
          <option value={profile.id}>{profile.label}</option>
        {/each}
      </select>
      <div class="eyeloss-control-card__hint">Planner notes and prompt override need saved planner profile.</div>
      {#if plannerHint()}
        <div class="eyeloss-control-card__hint">planner hint: {plannerHint().summary}</div>
        {#if plannerHint()?.envVars?.length}
          <div class="eyeloss-mission__item-meta">planner env: {plannerHint().envVars.join(', ')}</div>
        {/if}
      {/if}
    </label>
    <label class="eyeloss-control-field eyeloss-control-field--wide">
      <span>Planner notes</span>
      <textarea class="input-mono eyeloss-control-field__textarea" bind:value={missionPlannerNotes} rows="3" placeholder="Viewer-first notes for planner."></textarea>
    </label>
    <label class="eyeloss-control-field eyeloss-control-field--wide">
      <span>Planner prompt override</span>
      <textarea class="input-mono eyeloss-control-field__textarea" bind:value={missionPlannerPromptOverride} rows="3" placeholder="Override planner system prompt for this mission."></textarea>
    </label>
  </div>

  <div class="eyeloss-control-actions">
    <button type="button" class="btn btn-primary" onclick={onSaveMission} disabled={missionLoading || missionSaving || missionPlanning || missionCheckRunning || missionFixtureRunning}>
      {missionSaving ? 'Saving...' : 'Save'}
    </button>
    <button type="button" class="btn" onclick={onPlanMission} disabled={missionLoading || missionSaving || missionPlanning || missionCheckRunning || missionFixtureRunning}>
      {missionPlanning ? 'Planning...' : 'Plan'}
    </button>
    <button type="button" class="btn" onclick={onRunCheck} disabled={missionLoading || missionSaving || missionPlanning || missionCheckRunning || missionFixtureRunning}>
      {missionCheckRunning ? 'Checking...' : 'Run check'}
    </button>
    <button type="button" class="btn" onclick={onReload} disabled={missionLoading || missionSaving || missionPlanning || missionCheckRunning || missionFixtureRunning}>
      Reload
    </button>
  </div>

  <div class="eyeloss-control-card__hint">
    {missionState?.summary || 'Mission stays in the viewer-first path until planned.'}
  </div>

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Planner face</div>
    <div class="eyeloss-control-card__meta-grid">
      <span>planner: {plannerLabel() || missionState?.plannerProfileLabel || '-'}</span>
      <span>mode: {missionState?.plannerMode || 'viewer_first'}</span>
      <span>status: {missionState?.plannerStatus || 'idle'}</span>
    </div>
    <div class="eyeloss-control-card__hint">{missionState?.plannerSummary || 'viewer-first planner ready'}</div>
    {#if missionPlannerNotes || missionState?.plannerNotes}
      <div class="eyeloss-control-card__hint">notes: {missionPlannerNotes || missionState?.plannerNotes}</div>
    {/if}
    {#if missionPlannerPromptOverride || missionState?.plannerPromptOverride}
      <div class="eyeloss-control-card__hint">prompt override: {missionPlannerPromptOverride || missionState?.plannerPromptOverride}</div>
    {/if}
  </div>
</section>
