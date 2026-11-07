<script>
  let {
    watchdogState,
    watchdogError,
    watchdogConfigEnabled = $bindable(true),
    watchdogConfigTriggerMode = $bindable('manual_only'),
    watchdogConfigAlertThreshold = $bindable('warning'),
    watchdogConfigRequireActiveIssue = $bindable(true),
    watchdogConfigMaxFiles = $bindable('20'),
    watchdogConfigMaxDiffLines = $bindable('200'),
    watchdogConfigDedupeWindow = $bindable('300'),
    watchdogConfigChecks = $bindable({
      issueAlignment: true,
      semanticAlignment: false,
      instructionCompliance: true,
      missingValidation: true,
      costRisk: true,
    }),
    watchdogConfigSaving = false,
    onSaveWatchdogConfig,
    conductorWorkflowLoad,
    conductorWorkflowProvider = $bindable('coding_agent_app_server'),
    conductorWorkflowProviderModel = $bindable(''),
    conductorWorkflowTimeout = $bindable('900'),
    conductorWorkflowTraceRetention = $bindable('30'),
    conductorWorkflowStrictMode = $bindable(false),
    conductorLoading,
    conductorWorkflowLoading,
    conductorSettingsSaving,
    conductorError,
    onSaveSettings,
    onRefresh,
  } = $props();
</script>

<section class="eyeloss-control-card eyeloss-config-panel">
  <header class="eyeloss-control-card__header">
    <div>
      <div class="eyeloss-control-card__eyebrow">Config</div>
      <h2 class="eyeloss-control-card__title">Application Config</h2>
    </div>
  </header>

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Watchdog</div>

    {#if watchdogState?.configLoad}
      {@const cl = watchdogState.configLoad}
      <div class="eyeloss-control-card__hint">
        {cl.path || '.sloposcope/watchdog.json'} — {cl.loaded ? 'loaded' : cl.exists ? 'parse error' : 'not found'}
      </div>
      {#if cl.error}
        <div class="eyeloss-control-card__error">{cl.error}</div>
      {/if}
    {/if}

    {#if watchdogError}
      <div class="eyeloss-control-card__error">{watchdogError}</div>
    {/if}

    <div class="eyeloss-form-grid eyeloss-form-grid--two">
      <label class="eyeloss-control-field eyeloss-control-field--checkbox">
        <span>Enabled</span>
        <input type="checkbox" bind:checked={watchdogConfigEnabled} />
      </label>
      <label class="eyeloss-control-field eyeloss-control-field--checkbox">
        <span>Require active issue</span>
        <input type="checkbox" bind:checked={watchdogConfigRequireActiveIssue} />
      </label>
      <label class="eyeloss-control-field">
        <span>Trigger mode</span>
        <select class="input-mono" bind:value={watchdogConfigTriggerMode}>
          <option value="manual_only">manual_only</option>
          <option value="before_push">before_push</option>
          <option value="on_large_change_batch">on_large_change_batch</option>
          <option value="on_branch_switch_after_quiet_period">on_branch_switch_after_quiet_period</option>
        </select>
      </label>
      <label class="eyeloss-control-field">
        <span>Alert threshold</span>
        <select class="input-mono" bind:value={watchdogConfigAlertThreshold}>
          <option value="info">info</option>
          <option value="warning">warning</option>
          <option value="critical">critical</option>
        </select>
      </label>
      <label class="eyeloss-control-field">
        <span>Max files in snapshot</span>
        <input class="input-mono" type="number" min="1" max="100" bind:value={watchdogConfigMaxFiles} />
      </label>
      <label class="eyeloss-control-field">
        <span>Max diff lines per file</span>
        <input class="input-mono" type="number" min="10" max="1000" bind:value={watchdogConfigMaxDiffLines} />
      </label>
      <label class="eyeloss-control-field">
        <span>Dedupe window (seconds)</span>
        <input class="input-mono" type="number" min="0" max="3600" bind:value={watchdogConfigDedupeWindow} />
      </label>
    </div>

    <div class="eyeloss-control-section__title" style="margin-top: 0.75rem;">Checks</div>
    <div class="eyeloss-form-grid eyeloss-form-grid--two">
      <label class="eyeloss-control-field eyeloss-control-field--checkbox">
        <span>Issue alignment</span>
        <input type="checkbox" bind:checked={watchdogConfigChecks.issueAlignment} />
      </label>
      <label class="eyeloss-control-field eyeloss-control-field--checkbox">
        <span>Semantic alignment</span>
        <input type="checkbox" bind:checked={watchdogConfigChecks.semanticAlignment} />
      </label>
      <label class="eyeloss-control-field eyeloss-control-field--checkbox">
        <span>Instruction compliance</span>
        <input type="checkbox" bind:checked={watchdogConfigChecks.instructionCompliance} />
      </label>
      <label class="eyeloss-control-field eyeloss-control-field--checkbox">
        <span>Missing validation</span>
        <input type="checkbox" bind:checked={watchdogConfigChecks.missingValidation} />
      </label>
      <label class="eyeloss-control-field eyeloss-control-field--checkbox">
        <span>Cost risk</span>
        <input type="checkbox" bind:checked={watchdogConfigChecks.costRisk} />
      </label>
    </div>

    <div class="eyeloss-control-actions">
      <button type="button" class="btn btn-primary" onclick={onSaveWatchdogConfig} disabled={watchdogConfigSaving}>
        {watchdogConfigSaving ? 'Saving...' : 'Save watchdog config'}
      </button>
    </div>
  </div>

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Conductor</div>

    {#if conductorWorkflowLoad && !conductorWorkflowLoad.exists}
      <div class="eyeloss-control-card__hint">WORKFLOW.md is missing in the active repo.</div>
    {/if}
    {#if conductorWorkflowLoad?.error}
      <div class="eyeloss-control-card__error">{conductorWorkflowLoad.error}</div>
    {/if}
    {#if conductorError}
      <div class="eyeloss-control-card__error">{conductorError}</div>
    {/if}

    <div class="eyeloss-form-grid eyeloss-form-grid--two">
      <label class="eyeloss-control-field">
        <span>Provider</span>
        <select class="input-mono" bind:value={conductorWorkflowProvider}>
          <option value="coding_agent_app_server">coding_agent_app_server</option>
          <option value="openai">openai</option>
          <option value="nim">nim</option>
        </select>
      </label>
      <label class="eyeloss-control-field">
        <span>Provider model</span>
        <input class="input-mono" type="text" bind:value={conductorWorkflowProviderModel} placeholder="gpt-4.1-mini" />
      </label>
      <label class="eyeloss-control-field">
        <span>Timeout sec</span>
        <input class="input-mono" type="number" min="10" max="3600" bind:value={conductorWorkflowTimeout} />
      </label>
      <label class="eyeloss-control-field">
        <span>Trace retention</span>
        <input class="input-mono" type="number" min="1" max="200" bind:value={conductorWorkflowTraceRetention} />
      </label>
      <label class="eyeloss-control-field eyeloss-control-field--checkbox">
        <span>Strict mode</span>
        <input type="checkbox" bind:checked={conductorWorkflowStrictMode} />
      </label>
    </div>
    <div class="eyeloss-control-actions">
      <button type="button" class="btn btn-primary" onclick={onSaveSettings} disabled={conductorLoading || conductorWorkflowLoading || conductorSettingsSaving}>
        {conductorSettingsSaving ? 'Saving...' : 'Save conductor config'}
      </button>
      <button type="button" class="btn" onclick={onRefresh} disabled={conductorLoading || conductorWorkflowLoading}>Reload</button>
    </div>
  </div>
</section>
