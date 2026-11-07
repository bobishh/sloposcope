<script lang="ts">
  import BugSpecimen from './BugSpecimen.svelte';
  import { buildSharedSpecimenGeometries } from './specimenGeometry.js';

  let { comparison = null, loading = false, file = '', assessment = null } = $props();
  let comparisonZoom = $state(1);
  let comparisonScrollX = $state<number | null>(null);
  let comparisonScrollY = $state(0);
  let lastComparisonKey = '';
  const unavailable = (label, reason) => ({ label, status: 'unavailable', anatomy: null, reason });
  let before = $derived(comparison?.beforeEndpoint || unavailable('Before', 'Comparison unavailable'));
  let after = $derived(comparison?.afterEndpoint || unavailable('After', 'Comparison unavailable'));

  function endpointMessage(endpoint, isBaseline) {
    if (endpoint?.status === 'absent') return isBaseline ? 'File did not exist' : 'File did not exist';
    if (endpoint?.status === 'pending') return 'Reading selected snapshot…';
    if (endpoint?.reason) return endpoint.reason;
    return 'Anatomy unavailable';
  }

  function setComparisonZoom(value) {
    comparisonZoom = value;
  }

  function setComparisonScrollX(worldX) {
    if (comparisonScrollX == null || Math.abs(comparisonScrollX - worldX) > 1) comparisonScrollX = worldX;
  }

  function setComparisonScrollY(worldY) {
    if (Math.abs(comparisonScrollY - worldY) > 1) comparisonScrollY = worldY;
  }

  let sharedGeometries = $derived(buildSharedSpecimenGeometries(
    comparison?.before || null,
    comparison?.after || null,
    0,
    {
      assessment,
      slotOrder: comparison?.slotOrder || [],
      beforeSlotKeys: comparison?.beforeSlotKeys || [],
      afterSlotKeys: comparison?.afterSlotKeys || [],
      beforeAbsentSlotKeys: comparison?.beforeAbsentSlotKeys || [],
      afterAbsentSlotKeys: comparison?.afterAbsentSlotKeys || [],
    },
  ));

  $effect(() => {
    const key = `${file}|${comparison?.comparisonId || ''}`;
    if (lastComparisonKey !== key) {
      lastComparisonKey = key;
      comparisonZoom = 1;
      comparisonScrollX = null;
      comparisonScrollY = 0;
    }
  });

</script>

<section class="bug-comparison" data-status={loading ? 'pending' : comparison?.state || 'unavailable'} aria-label="Bug comparison">
  <div class="bug-comparison__panels">
    <section class="bug-comparison__panel" aria-label="Before endpoint">
      <header class="bug-comparison__heading">
        <h3>Before</h3>
        <span>{before?.label || 'Selected baseline'}</span>
      </header>
      {#if loading}
        <BugSpecimen loading pendingFile={file} />
      {:else if comparison?.hasBaseline && before?.status === 'available' && comparison?.before}
        <BugSpecimen
          anatomy={comparison.before}
          geometry={sharedGeometries.before}
          slotKeys={comparison.beforeSlotKeys}
          slotOrder={comparison.slotOrder}
          absenceSlotKeys={comparison.beforeAbsentSlotKeys}
          callableChanges={comparison.callableChanges}
          zoomValue={comparison.hasBaseline ? comparisonZoom : null}
          scrollWorldX={comparison.hasBaseline ? comparisonScrollX : null}
          scrollWorldY={comparison.hasBaseline ? comparisonScrollY : null}
          onZoomChange={comparison.hasBaseline ? setComparisonZoom : null}
          onScrollWorldX={comparison.hasBaseline ? setComparisonScrollX : null}
          onScrollWorld={comparison.hasBaseline ? setComparisonScrollY : null}
        />
      {:else if comparison?.hasBaseline}
        <div class="bug-comparison__reserved bug-comparison__state" data-endpoint-state={before?.status || 'unavailable'} role="status">
          <strong>{before?.status === 'absent' ? 'File did not exist' : 'Before unavailable'}</strong>
          <span>{endpointMessage(before, true)}</span>
        </div>
      {:else}
        <div class="bug-comparison__reserved bug-comparison__state" data-endpoint-state="not-selected">
          <strong>No comparison selected</strong>
          <span>Only the selected After snapshot is shown.</span>
        </div>
      {/if}
    </section>

    <section class="bug-comparison__panel" aria-label="After endpoint">
      <header class="bug-comparison__heading">
        <h3>After</h3>
        <span>{after?.label || 'Selected snapshot'}</span>
      </header>
      {#if loading}
        <BugSpecimen loading pendingFile={file} />
      {:else if after?.status === 'available' && comparison?.after}
        <BugSpecimen
          anatomy={comparison.after}
          geometry={sharedGeometries.after}
          slotKeys={comparison.afterSlotKeys}
          slotOrder={comparison.slotOrder}
          absenceSlotKeys={comparison.afterAbsentSlotKeys}
          callableChanges={comparison.callableChanges}
          zoomValue={comparison.hasBaseline ? comparisonZoom : null}
          scrollWorldX={comparison.hasBaseline ? comparisonScrollX : null}
          scrollWorldY={comparison.hasBaseline ? comparisonScrollY : null}
          onZoomChange={comparison.hasBaseline ? setComparisonZoom : null}
          onScrollWorldX={comparison.hasBaseline ? setComparisonScrollX : null}
          onScrollWorld={comparison.hasBaseline ? setComparisonScrollY : null}
        />
      {:else}
        <div class="bug-comparison__reserved bug-comparison__state" data-endpoint-state={after?.status || 'unavailable'} role="status">
          <strong>{after?.status === 'absent' ? 'File did not exist' : 'After unavailable'}</strong>
          <span>{endpointMessage(after, false)}</span>
        </div>
      {/if}
    </section>
  </div>

</section>

<style>
  .bug-comparison { display: grid; flex: none; gap: 10px; min-height: 420px; color: inherit; container-type: inline-size; }
  .bug-comparison__panels { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; align-items: start; }
  .bug-comparison__panel { min-width: 0; height: calc(58vh + 230px); min-height: 620px; border: 1px solid rgba(255,255,255,.14); padding: 6px; display: grid; align-content: start; gap: 6px; overflow: auto; }
  .bug-comparison__heading { min-height: 32px; display: flex; justify-content: space-between; align-items: baseline; gap: 8px; border-bottom: 1px solid rgba(255,255,255,.14); }
  .bug-comparison__heading h3 { margin: 0; font-size: 12px; text-transform: uppercase; letter-spacing: .08em; }
  .bug-comparison__heading span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 10px; opacity: .75; }
  .bug-comparison__reserved { min-height: 344px; display: grid; place-content: center; gap: 8px; text-align: center; padding: 12px; background: rgba(255,255,255,.025); color: inherit; }
  .bug-comparison__state strong { font-size: 13px; }
  .bug-comparison__state span { font-size: 11px; opacity: .75; }
  @media (max-width: 700px) { .bug-comparison__panels { grid-template-columns: minmax(0, 1fr); } }
  @container (max-width: 700px) { .bug-comparison__panels { grid-template-columns: minmax(0, 1fr); } }
</style>
