<script>
  let {
    conductorStatus,
    conductorLoading = false,
    conductorWorkflowLoading = false,
    conductorRunning = false,
    conductorError,
    conductorLaunchResult = null,
    conductorLaunchLoading = false,
    conductorDispatchLoading = false,
    conductorIngestLoading = false,
    conductorTaskId = $bindable(''),
    conductorMaxRetries = $bindable('2'),
    conductorTraceLimit = $bindable('20'),
    conductorIngestTaskId = $bindable(''),
    conductorIngestRawEvent = $bindable(''),
    onEnqueue,
    onTick,
    onRefresh,
    onLaunch,
    onDispatch,
    onIngestEvent,
  } = $props();

  const sampleRunnerEvent = '{"eventType":"completed","sessionId":"abc"}';
</script>

<section class="eyeloss-control-card eyeloss-runs-panel">
  <header class="eyeloss-control-card__header">
    <div>
      <div class="eyeloss-control-card__eyebrow">Runs</div>
      <h2 class="eyeloss-control-card__title">Issue Runs</h2>
    </div>
    <div class="eyeloss-control-card__meta">{conductorStatus?.issueRuns?.length || 0} tracked</div>
  </header>

  <div class="eyeloss-control-card__meta-grid">
    <span>total: {conductorStatus?.tasksTotal || 0}</span>
    <span>running: {conductorStatus?.tasksRunning || 0}</span>
    <span>retry: {conductorStatus?.tasksRetryScheduled || 0}</span>
    <span>done: {conductorStatus?.tasksReconciled || 0}</span>
    <span>traces: {conductorStatus?.tracesTotal || 0}</span>
    <span>last tick: {conductorStatus?.lastTickAt || '-'}</span>
  </div>

  {#if conductorError}
    <div class="eyeloss-control-card__error">{conductorError}</div>
  {/if}

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Run controls</div>
    {#if conductorStatus?.issueRuns?.length}
      <div class="eyeloss-runs__list">
        {#each conductorStatus.issueRuns as ir}
          <div class="eyeloss-runs__row">
            <div class="eyeloss-runs__row-main">
              <div>{ir.taskId}</div>
              <div class="eyeloss-control-card__hint">{ir.state} | a{ir.runAttempt} | {ir.activeSessionId || 'no session'}</div>
            </div>
            <div class="eyeloss-control-actions eyeloss-control-actions--compact">
              <button type="button" class="btn btn-primary" onclick={() => onLaunch?.(ir.taskId)} disabled={conductorLaunchLoading}>
                {conductorLaunchLoading ? '...' : 'Prepare launch'}
              </button>
              <button type="button" class="btn" onclick={() => onDispatch?.(ir.taskId)} disabled={conductorDispatchLoading}>
                {conductorDispatchLoading ? '...' : 'Dispatch'}
              </button>
            </div>
          </div>
        {/each}
      </div>
    {:else}
      <div class="eyeloss-control-card__hint">No runs yet.</div>
    {/if}
  </div>

  {#if conductorLaunchResult}
    <div class="eyeloss-control-section">
      <div class="eyeloss-control-section__title">Launch payload</div>
      <pre class="sloposcope-conductor__payload">{JSON.stringify(conductorLaunchResult, null, 2)}</pre>
    </div>
  {/if}

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Queue</div>
    <div class="eyeloss-form-grid eyeloss-form-grid--three">
      <label class="eyeloss-control-field">
        <span>Task ID</span>
        <input class="input-mono" type="text" bind:value={conductorTaskId} placeholder="issue_123" />
      </label>
      <label class="eyeloss-control-field">
        <span>Max retries</span>
        <input class="input-mono" type="number" min="0" max="10" bind:value={conductorMaxRetries} />
      </label>
      <label class="eyeloss-control-field">
        <span>Trace limit</span>
        <input class="input-mono" type="number" min="1" max="100" bind:value={conductorTraceLimit} />
      </label>
    </div>
    <div class="eyeloss-control-actions">
      <button type="button" class="btn btn-primary" onclick={onEnqueue} disabled={conductorLoading || conductorWorkflowLoading || conductorRunning}>Enqueue</button>
      <button type="button" class="btn" onclick={onTick} disabled={conductorLoading || conductorWorkflowLoading || conductorRunning}>
        {conductorRunning ? 'Ticking...' : 'Tick now'}
      </button>
      <button type="button" class="btn" onclick={onRefresh} disabled={conductorLoading || conductorWorkflowLoading}>Reload</button>
    </div>
  </div>

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Runner ingest</div>
    <div class="eyeloss-form-grid eyeloss-form-grid--one">
      <label class="eyeloss-control-field">
        <span>Task ID</span>
        <input class="input-mono" type="text" bind:value={conductorIngestTaskId} placeholder="issue_123" />
      </label>
      <label class="eyeloss-control-field">
        <span>Raw event JSON</span>
        <textarea class="input-mono eyeloss-control-field__textarea" rows="4" bind:value={conductorIngestRawEvent} placeholder={sampleRunnerEvent}></textarea>
      </label>
    </div>
    <div class="eyeloss-control-actions">
      <button type="button" class="btn btn-primary" onclick={onIngestEvent} disabled={conductorIngestLoading || !conductorIngestTaskId || !conductorIngestRawEvent}>
        {conductorIngestLoading ? 'Ingesting...' : 'Ingest event'}
      </button>
    </div>
  </div>

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Recent traces</div>
    {#if conductorStatus?.recentTraces?.length}
      <div class="eyeloss-conductor__traces">
        {#each conductorStatus.recentTraces as trace}
          <div class="eyeloss-conductor__trace">
            {trace.at} | {trace.event}{trace.taskId ? ` | ${trace.taskId}` : ''}{trace.detail ? ` | ${trace.detail}` : ''}
          </div>
        {/each}
      </div>
    {:else}
      <div class="eyeloss-control-card__hint">No run traces yet.</div>
    {/if}
  </div>
</section>
