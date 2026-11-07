<script>
  import { untrack } from 'svelte';
  import JevTraits from './JevTraits.svelte';
  let { invoke, onBack, filePath = null, sourceId = null } = $props();
  let inventory = $state(null);
  let selected = $state([]);
  let filter = $state('');
  let maxRequests = $state(5);
  let plan = $state(null);
  let token = $state('');
  let busy = $state('');
  let error = $state('');
  let outcome = $state(null);
  let demo = $derived(inventory?.transport === 'demo');
  let groups = $derived((inventory?.groups || []).filter(group => !filePath || group.paths.includes(filePath)));
  let matching = $derived(groups.filter(g => `${g.language} ${g.paths.join(' ')}`.toLowerCase().includes(filter.toLowerCase())));
  let visible = $derived(matching.slice(0, 100));
  let cached = $derived(groups.filter(g => g.cached).length);
  let fileCount = $derived(groups.reduce((n, g) => n + g.paths.length, 0));

  function invalidate() { plan = null; token = ''; error = ''; }
  function toggle(id, checked) {
    selected = checked ? [...selected, id] : selected.filter(value => value !== id);
    invalidate();
  }
  async function loadIndex() {
    invalidate(); selected = []; busy = 'index';
    try {
      inventory = await invoke('get_jev_index');
      if (filePath) {
        const group = inventory.groups.find(group => group.paths.includes(filePath));
        if (!group) error = 'File excluded or unavailable. No source selected; no requests sent.';
        else if (sourceId && group.source_id !== sourceId) error = 'Selected snapshot differs from the working copy. Select @ to analyze the current file.';
        else if (!group.cached) { selected = [group.id]; await preview(); }
      }
    }
    catch (err) { error = String(err); }
    finally { busy = ''; }
  }
  async function preview() {
    invalidate(); busy = 'preview';
    try {
      plan = await invoke('preview_jev_analysis', { selected, maxRequests: filePath ? 1 : Number(maxRequests) });
      if (filePath && (plan.batches.some(batch => batch.groups.some(group => !group.paths.includes(filePath) || (sourceId && group.source_id !== sourceId))) || plan.request_count > 1)) {
        plan = null;
        throw new Error('File selection changed. Preview this file again.');
      }
    }
    catch (err) { error = String(err); }
    finally { busy = ''; }
  }
  async function send() {
    if (!plan || busy || (!demo && !token.trim())) return;
    const approved = plan;
    const apiKey = token;
    // Never use invokeTraced: arguments contain ephemeral credentials.
    token = ''; plan = null; busy = 'send'; error = '';
    try {
      outcome = await invoke('confirm_jev_analysis', { planId: approved.id, confirmedRequests: approved.request_count, apiKey });
      if (outcome.error) error = outcome.error;
      // Result belongs to the approved source hash, not necessarily current source.
      // Reindex locally after every outcome; no additional classifier requests.
      inventory = await invoke('get_jev_index');
      selected = selected.filter(id => inventory.groups.some(g => g.id === id && !g.cached));
    } catch (err) { error = String(err); }
    finally { busy = ''; }
  }

  $effect(() => {
    if (filePath) untrack(() => loadIndex());
  });
</script>

<section aria-label="Jev analysis" class="jev-panel" class:jev-panel--file={!!filePath}>
  <header>
    <h2>Jev analysis</h2>
    <button type="button" class="btn" onclick={onBack} disabled={busy === 'send'}>{filePath ? 'Close analysis' : 'Back to graph'}</button>
  </header>
  <p>Local index first. Select evidence. Preview exact payload. Confirm paid requests separately.</p>
  <p class="dim">No automatic analysis. Identical source shares results. Cache includes full source, language, model and rubric.</p>
  {#if demo}<p class="notice">DEMO TRANSPORT · synthetic answers · no network, no credentials</p>{/if}
  {#if !outcome}<p>No requests sent</p>{/if}
  {#if outcome}
    <p>Requests attempted: {outcome.requests_attempted} · completed: {outcome.requests_completed} · actual tokens: {outcome.input_tokens} in / {outcome.output_tokens} out</p>
  {/if}
  {#if error}<p role="alert" class="error">{error}</p>{/if}
  {#if busy}<p role="status">{busy === 'send' ? 'Sending approved batch… no automatic retry' : 'Building local preview… no requests sent'}</p>{/if}
  <div class="tools">
    {#if filePath}
    <span>{filePath}</span>
    <button type="button" class="btn" onclick={loadIndex} disabled={!!busy}>Preview file analysis</button>
    {:else}
    <button type="button" class="btn" onclick={loadIndex} disabled={!!busy}>Build local index</button>
    <label>Filter paths / languages <input aria-label="Filter source groups" bind:value={filter} disabled={!!busy} placeholder="rust / src / config…" /></label>
    <label>Request cap (1–20) <input aria-label="Request cap" type="number" min="1" max="20" value={maxRequests} oninput={e => { maxRequests = Number(e.currentTarget.value); invalidate(); }} disabled={!!busy} /></label>
    <button type="button" class="btn" onclick={preview} disabled={!!busy || !selected.length}>Preview send plan</button>
    {/if}
  </div>
  {#if inventory}
    <div class="inventory">
      <p>{groups.length} source groups · {fileCount} source files · {cached} cached · {selected.length} selected</p>
      <p class="dim">{inventory.repo}</p>
      {#if inventory.uncertain_requests}<p class="notice">{inventory.uncertain_requests} prior requests lack persisted results and may have been charged. No automatic retry. Review selection before resending.</p>{/if}
      <p class="dim">Excluded: {Object.entries(inventory.excluded).map(([reason, count]) => `${reason}: ${count}`).join(' · ') || 'none'}. Dependency/build folders omitted. Secret detection is heuristic — inspect payload.</p>
      {#if matching.length > visible.length}<p>Showing first 100 of {matching.length} groups. Narrow filter; selection remains explicit.</p>{/if}
      <div class="groups">
        {#each visible as group (group.id)}
          <div class="group">
            <label>
              {#if !filePath}<input type="checkbox" aria-label={`Select ${group.paths[0]}`} checked={selected.includes(group.id)} onchange={e => toggle(group.id, e.currentTarget.checked)} disabled={!!busy || !!group.cached} />{/if}
              <span>{group.paths[0]}</span>
              <span class="dim">{group.language} · {group.source_bytes} bytes{group.truncated ? ' · excerpt only' : ''}{group.cached ? ' · cached' : ''}</span>
            </label>
            {#if group.paths.length > 1}<div class="dim aliases">Same source: {group.paths.slice(1).join(' · ')}</div>{/if}
            {#if group.cached}<JevTraits assessment={group.cached} />{/if}
          </div>
        {/each}
      </div>
    </div>
  {/if}
  {#if plan}
    <section class="confirmation" aria-label="Send confirmation">
      <h3>Send confirmation</h3>
      {#if plan.uncertain_requests}<p class="notice">{plan.uncertain_requests} previous attempts have unknown completion. This new confirmation can incur additional charges.</p>{/if}
      <p>{plan.request_count} {plan.request_count === 1 ? 'request' : 'requests'} · {plan.batches.reduce((n,b) => n + b.groups.length, 0)} uncached source groups · {plan.cached.length} reused</p>
      <p>{plan.payload_bytes} payload bytes · approximately {Math.ceil(plan.payload_bytes / 4)} input tokens (rough estimate, not a price or billing cap)</p>
      <p class="dim">POST https://api.typesafe.ai/v1/systemone · {plan.model} · {plan.rubric}. Sequential, no retries. Timeout/error may still be charged. Approval expires after 10 minutes, cannot be reused.</p>
      {#each plan.batches as batch, i}
        <details>
          <summary>Request {i + 1} · {batch.payload_bytes} bytes · {batch.groups.map(g => g.paths.join(', ')).join(' · ')}</summary>
          <pre>{JSON.stringify(batch.payload, null, 2)}</pre>
        </details>
      {/each}
      {#if plan.request_count > 0}
        {#if !demo}
          <label>Jev API token <input aria-label="Jev API token" type="password" bind:value={token} autocomplete="off" spellcheck="false" /></label>
          <p class="dim">Memory only, this send only. Never stored, logged, or read from another project.</p>
        {/if}
        <button type="button" class="btn btn-primary" onclick={send} disabled={!!busy || (!demo && !token.trim())}>Confirm and send {plan.request_count} {plan.request_count === 1 ? 'request' : 'requests'}</button>
        <button type="button" class="btn" onclick={invalidate}>Cancel preview</button>
      {:else}<p>Everything selected is cached. No send needed.</p>{/if}
    </section>
  {/if}
</section>

<style>
  .jev-panel { flex: 1; min-height: 0; overflow: auto; padding: 20px 24px; font-size: 12px; line-height: 1.5; }
  .jev-panel--file { flex: none; overflow: visible; }
  header, .tools { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; }
  header { justify-content: space-between; border-bottom: 1px solid var(--bg-300); padding-bottom: 12px; }
  h2, h3 { font-size: 14px; font-weight: normal; text-transform: uppercase; letter-spacing: .08em; }
  p { margin: 8px 0; }
  .dim { color: var(--text-dim); font-size: 11px; }
  .notice { color: var(--secondary); }
  .error { color: var(--red); border-left: 2px solid var(--red); padding: 8px; }
  .tools { border-block: 1px solid var(--bg-300); padding: 12px 0; margin: 16px 0; }
  label { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  input { color: var(--text); background: var(--bg-100); border: 1px solid var(--bg-300); padding: 6px; font: inherit; }
  input[type='number'] { width: 65px; }
  input[type='password'] { min-width: 260px; }
  .group { padding: 10px 0; border-bottom: 1px solid var(--bg-300); overflow-wrap: anywhere; }
  .aliases { padding-left: 24px; }
  .confirmation { margin: 20px 0; border: 1px solid var(--secondary); padding: 16px; }
  summary { cursor: pointer; padding: 8px 0; overflow-wrap: anywhere; }
  pre { max-height: 350px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; padding: 12px; background: var(--bg-100); font-size: 11px; }
  .confirmation button { margin: 10px 8px 0 0; }
</style>
