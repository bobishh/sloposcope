<script>
  let {
    profiles = [],
    profileSaving = false,
    profileDeletingId = '',
    profileError = '',
    selectedCodeWorkerProfileId = $bindable(''),
    profileLabel = $bindable(''),
    profileProvider = $bindable(''),
    profileModel = $bindable(''),
    profileCommand = $bindable(''),
    profileArgs = $bindable(''),
    profileEnvJson = $bindable('{}'),
    profileCapabilities = $bindable(''),
    profileTransportKind = $bindable('pty'),
    profileActive = $bindable(false),
    profileIsReviewer = $bindable(false),
    profileEnvError = '',
    profileProviderHint = {
      title: 'Shell env first',
      summary: 'Worker inherits shell env. Env JSON adds or overrides vars for this profile only.',
      envVars: [],
    },
    onApplyTemplate,
    onSaveProfile,
    onDeleteProfile,
    onUseProfile,
    onClearForm,
    onReload,
  } = $props();

  function canSave() {
    return (
      profileLabel.trim().length > 0
      && profileProvider.trim().length > 0
      && (profileTransportKind === 'http' || profileCommand.trim().length > 0)
      && !profileEnvError
    );
  }

  function transportLabel(profile) {
    return profile?.transportKind || profile?.transport_kind || 'pty';
  }

  function isReviewerProfile(profile) {
    return Boolean(profile?.isReviewer ?? profile?.is_reviewer);
  }

  function profileEnv(profile) {
    return profile?.env && typeof profile.env === 'object' && !Array.isArray(profile.env) ? profile.env : {};
  }

  function profileEnvKeys(profile) {
    return Object.keys(profileEnv(profile));
  }

  function formatProfileEnvJson(profile) {
    return JSON.stringify(profileEnv(profile), null, 2);
  }
</script>

<section class="eyeloss-control-card eyeloss-agent-profiles-panel">
  <header class="eyeloss-control-card__header">
    <div>
      <div class="eyeloss-control-card__eyebrow">Debug</div>
      <h2 class="eyeloss-control-card__title">Agent Profiles</h2>
    </div>
    <div class="eyeloss-control-card__meta">{profiles.length} saved</div>
  </header>

  {#if profileError}
    <div class="eyeloss-control-card__error">{profileError}</div>
  {/if}

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Code worker selection</div>
    <label class="eyeloss-control-field">
      <span>Selected profile</span>
      <select class="input-mono" bind:value={selectedCodeWorkerProfileId}>
        <option value="">Automatic fallback</option>
        {#each profiles as profile}
          <option value={profile.id}>{profile.label} ({profile.provider})</option>
        {/each}
      </select>
    </label>
    <div class="eyeloss-control-card__hint">Runs tab uses selected profile. Empty keeps backend fallback.</div>
  </div>

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Horse templates</div>
    <div class="eyeloss-control-actions">
      <button type="button" class="btn" onclick={() => onApplyTemplate?.('nim')}>NIM</button>
      <button type="button" class="btn" onclick={() => onApplyTemplate?.('opencode_nim')}>OpenCode on NIM</button>
      <button type="button" class="btn" onclick={() => onApplyTemplate?.('pi_nim')}>PI on NIM</button>
      <button type="button" class="btn" onclick={() => onApplyTemplate?.('gemini')}>Gemini</button>
      <button type="button" class="btn" onclick={() => onApplyTemplate?.('codex')}>Codex</button>
      <button type="button" class="btn" onclick={() => onApplyTemplate?.('claude')}>Claude</button>
      <button type="button" class="btn" onclick={() => onApplyTemplate?.('local')}>Local</button>
      <button type="button" class="btn" onclick={() => onApplyTemplate?.('local_live')}>Live Local</button>
    </div>
    <div class="eyeloss-control-card__hint">Templates fill sane defaults. Save after edit if needed.</div>
  </div>

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Profile form</div>
    <div class="eyeloss-form-grid eyeloss-form-grid--two">
      <label class="eyeloss-control-field">
        <span>Label</span>
        <input class="input-mono" type="text" bind:value={profileLabel} placeholder="Gemini horse" />
      </label>
      <label class="eyeloss-control-field">
        <span>Provider</span>
        <input class="input-mono" type="text" bind:value={profileProvider} placeholder="gemini_cli" />
      </label>
      <label class="eyeloss-control-field">
        <span>Command</span>
        <input class="input-mono" type="text" bind:value={profileCommand} placeholder="gemini" />
      </label>
      <label class="eyeloss-control-field">
        <span>Model</span>
        <input class="input-mono" type="text" bind:value={profileModel} placeholder="gemini-2.5-pro" />
      </label>
      <div class="eyeloss-control-field eyeloss-control-field--wide">
        <span>Token hint</span>
        <div class="eyeloss-control-card__hint">{profileProviderHint.title}. {profileProviderHint.summary}</div>
        {#if profileProviderHint?.envVars?.length}
          <div class="eyeloss-mission__item-meta">expected env: {profileProviderHint.envVars.join(', ')}</div>
        {/if}
      </div>
      <label class="eyeloss-control-field eyeloss-control-field--wide">
        <span>Args</span>
        <textarea class="input-mono eyeloss-control-field__textarea" bind:value={profileArgs} rows="3" placeholder="--sandbox&#10;--yolo"></textarea>
      </label>
      <label class="eyeloss-control-field eyeloss-control-field--wide">
        <span>Env JSON</span>
        <textarea class="input-mono eyeloss-control-field__textarea" bind:value={profileEnvJson} rows="4" placeholder="JSON object"></textarea>
        <div class="eyeloss-control-card__hint">Merged after inherited shell env. JSON object only.</div>
        {#if profileEnvError}
          <div class="eyeloss-control-card__error">{profileEnvError}</div>
        {/if}
      </label>
      <label class="eyeloss-control-field eyeloss-control-field--wide">
        <span>Capabilities</span>
        <textarea class="input-mono eyeloss-control-field__textarea" bind:value={profileCapabilities} rows="3" placeholder="can_edit&#10;can_run_tools"></textarea>
      </label>
      <label class="eyeloss-control-field">
        <span>Transport</span>
        <select class="input-mono" bind:value={profileTransportKind}>
          <option value="pty">pty</option>
          <option value="http">http</option>
          <option value="internal">internal</option>
        </select>
      </label>
      <label class="eyeloss-control-field eyeloss-control-field--checkbox">
        <span>Active profile</span>
        <input type="checkbox" bind:checked={profileActive} />
      </label>
      <label class="eyeloss-control-field eyeloss-control-field--checkbox">
        <span>Reviewer role</span>
        <input type="checkbox" bind:checked={profileIsReviewer} />
      </label>
    </div>

    <div class="eyeloss-control-actions">
      <button type="button" class="btn btn-primary" onclick={onSaveProfile} disabled={profileSaving || !canSave()}>
        {profileSaving ? 'Saving profile...' : 'Save profile'}
      </button>
      <button type="button" class="btn" onclick={onClearForm} disabled={profileSaving}>Clear form</button>
      <button type="button" class="btn" onclick={onReload} disabled={profileSaving}>Reload</button>
    </div>
  </div>

  <div class="eyeloss-control-section">
    <div class="eyeloss-control-section__title">Saved profiles</div>
    {#if profiles.length}
      <div class="eyeloss-mission__stack">
        {#each profiles as profile}
          <article class="eyeloss-mission__item">
            <div class="eyeloss-mission__item-header">
              <span class="eyeloss-mission__item-id">{profile.id}</span>
              <span class="eyeloss-mission__item-state">{transportLabel(profile)}</span>
            </div>
            <div class="eyeloss-mission__item-title">{profile.label}</div>
            {#if profile.active}
              <div class="eyeloss-mission__item-meta">active</div>
            {/if}
            {#if isReviewerProfile(profile)}
              <div class="eyeloss-mission__item-meta">reviewer</div>
            {/if}
            <div class="eyeloss-control-card__hint">
              provider: {profile.provider}{#if profile.model} | model: {profile.model}{/if}{#if profile.command} | command: {profile.command}{/if}
            </div>
            {#if profile.capabilities?.length}
              <div class="eyeloss-mission__item-meta">capabilities: {profile.capabilities.join(', ')}</div>
            {/if}
            <div class="eyeloss-control-card__hint">
              env overrides: {profileEnvKeys(profile).length ? profileEnvKeys(profile).join(', ') : 'inherit shell only'}
            </div>
            {#if profileEnvKeys(profile).length}
              <details>
                <summary>Env JSON</summary>
                <pre class="input-mono">{formatProfileEnvJson(profile)}</pre>
              </details>
            {/if}
            <div class="eyeloss-control-actions">
              <button type="button" class="btn" onclick={() => onUseProfile?.(profile.id)} disabled={profileDeletingId === profile.id || selectedCodeWorkerProfileId === profile.id}>
                {selectedCodeWorkerProfileId === profile.id ? 'Selected' : 'Use for code worker'}
              </button>
              <button type="button" class="btn" onclick={() => onDeleteProfile?.(profile.id)} disabled={profileSaving || profileDeletingId === profile.id}>
                {profileDeletingId === profile.id ? 'Deleting...' : 'Delete'}
              </button>
            </div>
          </article>
        {/each}
      </div>
    {:else}
      <div class="eyeloss-control-card__hint">No saved profiles yet.</div>
    {/if}
  </div>
</section>
