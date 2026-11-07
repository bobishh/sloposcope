<script>
  let { assessment } = $props();
  let answers = $derived(assessment?.answers);
  let insufficient = $derived(answers?.context?.choice !== 'sufficient');
</script>

{#if assessment && answers}
  <section aria-label="Jev traits" class="jev-traits">
    <div class="jev-traits__title">Jev · interpreted traits · cached snapshot</div>
    <div>Role: {answers.role?.choice || 'unknown'} · confidence {Number(answers.role?.confidence || 0).toFixed(2)}</div>
    <div>Context: {answers.context?.choice || 'unknown'}{assessment.truncated ? ' · excerpt, middle omitted' : ' · full file'}</div>
    {#if !insufficient}
      <div>Multiple responsibilities: P(yes) = {Number(answers.multiple?.noul || 0).toFixed(2)}</div>
      <div>Information hiding: {Number(answers.hiding?.score || 0).toFixed(2)} / 2 · shallow → mixed → deep</div>
    {:else}
      <div>Insufficient context — structural conclusions withheld. Raw answers below.</div>
    {/if}
    <details>
      <summary>Evidence / distributions / rubric</summary>
      <div>{assessment.model} · {assessment.rubric} · {assessment.id.slice(0, 12)}</div>
      <div>Confidence describes distribution concentration, not verified accuracy. No measured complexity here.</div>
      <pre>{JSON.stringify(answers, null, 2)}</pre>
    </details>
  </section>
{/if}

<style>
  .jev-traits { border-left: 2px solid var(--secondary); padding: 8px 12px; margin: 8px 0; font-size: 11px; line-height: 1.6; color: var(--text); }
  .jev-traits__title { color: var(--secondary); text-transform: uppercase; font-size: 10px; }
  summary { cursor: pointer; color: var(--text-dim); }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; font-size: 10px; }
</style>
