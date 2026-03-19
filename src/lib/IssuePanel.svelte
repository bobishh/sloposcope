<script>
  let {
    watchdogState,
    watchdogTraces,
    watchdogStatusClass,
    watchdogLoading,
    watchdogRunning,
    watchdogError,
    clusterRefreshLoading = false,
    graphMeta = null,
    watchdogIssueId = $bindable(''),
    watchdogIssueTitle = $bindable(''),
    watchdogIssueIdentifier = $bindable(''),
    watchdogIssueUrl = $bindable(''),
    watchdogIssueSummary = $bindable(''),
    watchdogIssueLabels = $bindable(''),
    watchdogIssuePriority = $bindable(''),
    watchdogAcceptanceCriteria = $bindable(''),
    watchdogNonGoals = $bindable(''),
    watchdogValidationDetails = $bindable(''),
    formatWatchdogStatus,
    onSaveIssueContext,
    onClearIssueContext,
    onRunCheck,
    onRunValidationCommand,
    onRefreshClusters,
    onReload,
  } = $props();

  let showAdvanced = $state(false);
</script>

<section class="eyeloss-control-card eyeloss-issue-panel">
  <header class="eyeloss-control-card__header">
    <div>
      <div class="eyeloss-control-card__eyebrow">Issue</div>
      <h2 class="eyeloss-control-card__title">Issue Context</h2>
    </div>
    <div class={`eyeloss-watchdog__status ${watchdogStatusClass}`}>
      {formatWatchdogStatus(watchdogState?.lastVerdict?.status || 'insufficient_context')}
    </div>
  </header>

  <div class="eyeloss-control-card__meta-grid">
    <span>trigger: {watchdogState?.config?.triggerMode || 'manual_only'}</span>
    <span>alerts: {watchdogState?.alerts?.length || 0}</span>
    <span>traces: {watchdogTraces.length}</span>
    <span>clusters: {graphMeta?.clusteringMode || 'heuristic'}</span>
    <span>entities: {graphMeta?.canonicalEntityCount || 0}</span>
    <span>edges: {graphMeta?.canonicalEdgeCount || 0}</span>
  </div>

  {#if watchdogError}
    <div class="eyeloss-control-card__error">{watchdogError}</div>
  {/if}

  <div class="eyeloss-form-grid eyeloss-form-grid--two">
    <label class="eyeloss-control-field eyeloss-control-field--wide">
      <span>Title</span>
      <input class="input-mono" type="text" bind:value={watchdogIssueTitle} placeholder="Investigate drift in graph runtime" />
    </label>
    <label class="eyeloss-control-field eyeloss-control-field--wide">
      <span>Brief</span>
      <textarea class="input-mono eyeloss-control-field__textarea" bind:value={watchdogIssueSummary} rows="4" placeholder="What are we actually trying to accomplish?"></textarea>
    </label>
    <label class="eyeloss-control-field">
      <span>Validation command</span>
      <input class="input-mono" type="text" bind:value={watchdogValidationDetails} placeholder="npm run -s build && cargo test --quiet" />
    </label>
    <label class="eyeloss-control-field">
      <span>Identifier / issue key</span>
      <input class="input-mono" type="text" bind:value={watchdogIssueIdentifier} placeholder="LIN-123" />
    </label>
    <label class="eyeloss-control-field eyeloss-control-field--wide">
      <span>URL</span>
      <input class="input-mono" type="text" bind:value={watchdogIssueUrl} placeholder="https://linear.app/..." />
    </label>
  </div>

  <div class="eyeloss-control-actions">
    <button type="button" class="btn btn-primary" onclick={onSaveIssueContext} disabled={watchdogLoading}>Save</button>
    <button type="button" class="btn btn-ghost" onclick={onClearIssueContext} disabled={watchdogLoading}>Clear</button>
    <button type="button" class="btn" onclick={() => showAdvanced = !showAdvanced}>
      {showAdvanced ? 'Hide advanced' : 'Show advanced'}
    </button>
  </div>

  <div class="eyeloss-control-card__hint">
    Task id is auto-derived from identifier/title if left empty.
  </div>

  {#if showAdvanced}
    <div class="eyeloss-control-section">
      <div class="eyeloss-control-section__title">Advanced</div>
      <div class="eyeloss-form-grid eyeloss-form-grid--two">
        <label class="eyeloss-control-field">
          <span>Task ID</span>
          <input class="input-mono" type="text" bind:value={watchdogIssueId} placeholder="issue-123" />
        </label>
        <label class="eyeloss-control-field">
          <span>Priority</span>
          <input class="input-mono" type="text" bind:value={watchdogIssuePriority} placeholder="high" />
        </label>
        <label class="eyeloss-control-field eyeloss-control-field--wide">
          <span>Labels (comma separated)</span>
          <input class="input-mono" type="text" bind:value={watchdogIssueLabels} placeholder="watchdog,graph" />
        </label>
        <label class="eyeloss-control-field eyeloss-control-field--wide">
          <span>Acceptance criteria (one per line)</span>
          <textarea class="input-mono eyeloss-control-field__textarea" bind:value={watchdogAcceptanceCriteria} rows="4" placeholder="criterion 1&#10;criterion 2"></textarea>
        </label>
        <label class="eyeloss-control-field eyeloss-control-field--wide">
          <span>Non-goals (one per line)</span>
          <textarea class="input-mono eyeloss-control-field__textarea" bind:value={watchdogNonGoals} rows="4" placeholder="non-goal 1&#10;non-goal 2"></textarea>
        </label>
      </div>
    </div>
  {/if}

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Watchdog</div>

    <div class="eyeloss-control-actions">
      <button type="button" class="btn btn-primary" onclick={onRunCheck} disabled={watchdogRunning || watchdogLoading}>
        {watchdogRunning ? 'Checking...' : 'Run check'}
      </button>
      <button type="button" class="btn" onclick={() => onRunValidationCommand('build')} disabled={watchdogLoading}>Build</button>
      <button type="button" class="btn" onclick={() => onRunValidationCommand('test')} disabled={watchdogLoading}>Test</button>
      <button type="button" class="btn" onclick={onRefreshClusters} disabled={clusterRefreshLoading || watchdogLoading}>
        {clusterRefreshLoading ? 'Refreshing...' : 'Refresh clusters'}
      </button>
      <button type="button" class="btn" onclick={onReload} disabled={watchdogLoading}>Reload</button>
    </div>

    {#if watchdogState?.lastValidation}
      <div class="eyeloss-control-card__hint">
        last validation: {watchdogState.lastValidation.kind} / {watchdogState.lastValidation.success ? 'ok' : 'fail'} / {watchdogState.lastValidation.ranAt}
      </div>
    {/if}

    {#if watchdogState?.lastVerdict}
      <div class="eyeloss-watchdog__verdict-summary">
        {watchdogState.lastVerdict.summary} (confidence {(Number(watchdogState.lastVerdict.confidence || 0) * 100).toFixed(0)}%)
      </div>
      {#if watchdogState.lastVerdict.issues?.length}
        <div class="eyeloss-watchdog__issues">
          {#each watchdogState.lastVerdict.issues as issue}
            <div class="eyeloss-watchdog__issue">[{issue.severity}] {issue.kind}: {issue.title}</div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>

  {#if watchdogState?.lastSnapshot}
    {@const snap = watchdogState.lastSnapshot}
    <div class="eyeloss-control-section">
      <div class="eyeloss-control-section__title">Graph evidence</div>
      <div class="eyeloss-control-card__meta-grid">
        <span>changed: {snap.changedFilesTotal || 0}</span>
        <span>affected: {snap.affectedEntities?.length || 0}</span>
        <span>related: {snap.relatedEntities?.length || 0}</span>
        <span>blast radius: {snap.blastRadiusCount || 0}</span>
      </div>
      {#if snap.relatedEntities?.length}
        <div class="eyeloss-control-card__hint">
          neighbors: {snap.relatedEntities.slice(0, 8).join(', ')}{snap.relatedEntities.length > 8 ? '…' : ''}
        </div>
      {/if}
    </div>
  {/if}

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Recent traces</div>
    {#if watchdogTraces.length > 0}
      <div class="eyeloss-watchdog__traces">
        {#each watchdogTraces.slice(0, 5) as trace}
          <div class="eyeloss-watchdog__trace">
            {trace.id} | {formatWatchdogStatus(trace?.verdict?.status || 'insufficient_context')} | {trace.createdAt}
          </div>
        {/each}
      </div>
    {:else}
      <div class="eyeloss-control-card__hint">No watchdog traces yet.</div>
    {/if}
  </div>
</section>
