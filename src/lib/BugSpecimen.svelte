<script lang="ts">
  import { classifyPathRole } from './ecosystemModel.js';
  import { buildSingleSpecimenGeometry } from './specimenGeometry.js';
  import {
    hitTestFileOrganism,
    hitTestFileOrganismState,
    paintFileOrganism,
    type FileOrganismGeometry,
    type FileOrganismLimb,
  } from './fileOrganism.ts';

  type ComparisonChange = {
    status?: string;
    before?: Record<string, number | null> | null;
    after?: Record<string, number | null> | null;
    beforeProfileId?: string | null;
    afterProfileId?: string | null;
    metricsCompatible?: boolean;
  };

  type Props = {
    anatomy?: FileAnatomyView | null;
    roleFact?: string | null;
    geometry?: FileOrganismGeometry | null;
    loading?: boolean;
    pendingFile?: string;
    slotKeys?: string[] | null;
    slotOrder?: string[] | null;
    absenceSlotKeys?: string[];
    callableChanges?: Record<string, ComparisonChange>;
    zoomValue?: number | null;
    scrollWorldX?: number | null;
    scrollWorldY?: number | null;
    onZoomChange?: ((zoom: number) => void) | null;
    onScrollWorldX?: ((worldX: number) => void) | null;
    onScrollWorld?: ((worldY: number) => void) | null;
  };

  type FileAnatomyView = {
    file?: string;
    status?: string;
    evidence?: string;
    reason?: string;
    lineCount?: number | null;
    sourceId?: string | null;
    profileId?: string | null;
    firstSeenAt?: string | null;
    ageAsOf?: string | null;
    ageDays?: number | null;
    ageReason?: string | null;
    callables?: readonly Record<string, unknown>[] | null;
    stateSlots?: readonly Record<string, unknown>[] | null;
  };

  let {
    anatomy = null,
    roleFact = null,
    geometry: suppliedGeometry = null,
    loading = false,
    pendingFile = '',
    slotKeys = null,
    slotOrder = null,
    absenceSlotKeys = [],
    callableChanges = {},
    zoomValue = null,
    scrollWorldX = null,
    scrollWorldY = null,
    onZoomChange = null,
    onScrollWorldX = null,
    onScrollWorld = null,
  }: Props = $props();

  let viewport = $state<HTMLDivElement | undefined>();
  let canvases = $state<(HTMLCanvasElement | undefined)[]>([]);
  let stageWidth = $state(640);
  let zoom = $state(1);
  let selectedId = $state<string | null>(null);
  let selectedKind = $state<'callable' | 'state'>('callable');
  let selectedGeometryKey = '';
  const geometry = $derived(suppliedGeometry || buildSingleSpecimenGeometry(anatomy, {
    slotOrder: slotOrder || [],
    slotKeys: slotKeys || [],
    absenceSlotKeys,
    roleFact: roleFact || classifyPathRole({ file: anatomy?.file || '' }).role,
  }));
  const canvasWidth = $derived(Math.max(1, geometry.bounds.width * zoom));
  const canvasHeight = $derived(Math.max(1, geometry.bounds.height * zoom));
  const tiles = $derived.by(() => {
    const tileHeight = 1800;
    const count = Math.max(1, Math.ceil(geometry.bounds.height / tileHeight));
    return Array.from({ length: count }, (_, index) => ({
      index,
      y: index * tileHeight,
      height: Math.min(tileHeight, geometry.bounds.height - index * tileHeight),
    }));
  });
  const selected = $derived(selectedKind === 'callable'
    ? geometry.limbs.find(limb => limb.id === selectedId) || null
    : geometry.stateSlots.find(state => state.id === selectedId) || null);
  const selectedChange = $derived(selected && selectedKind === 'callable'
    ? callableChanges?.[selected.slotKey] || null
    : null);

  function metricChangeLabel(label: string, field: 'bodyLineSpan' | 'decisionCount', change: ComparisonChange | null) {
    const before = change?.before?.[field];
    const after = change?.after?.[field];
    const beforeLabel = before == null ? 'unknown' : String(before);
    const afterLabel = after == null ? 'unknown' : String(after);
    const compatible = change?.metricsCompatible === true
      || Boolean(change?.beforeProfileId && change.beforeProfileId === change.afterProfileId);
    const beforeNumber = Number(before);
    const afterNumber = Number(after);
    const delta = compatible && before != null && after != null && Number.isFinite(beforeNumber) && Number.isFinite(afterNumber)
      ? ` (${afterNumber - beforeNumber >= 0 ? '+' : ''}${afterNumber - beforeNumber})`
      : '';
    return `${label}: ${beforeLabel} → ${afterLabel}${delta}`;
  }

  function visibilityLabel(value: string | null | undefined) {
    return ['public', 'private', 'protected'].includes(value || '') ? value : 'visibility unknown';
  }

  function rangeLabel(item: { startLine?: number | null; endLine?: number | null }) {
    if (item.startLine == null) return 'source range unknown';
    return item.endLine == null ? `L${item.startLine}–?` : `L${item.startLine}–${item.endLine}`;
  }

  function ageSummary(value: FileOrganismGeometry) {
    const firstSeen = value.firstSeenAt?.slice(0, 10);
    const reason = value.ageReason ? ` · ${value.ageReason}` : '';
    if (!firstSeen) return `First Git appearance unknown${reason}`;
    if (value.ageDays == null) return `First Git appearance: ${firstSeen} · age unknown${reason}`;
    const asOf = value.ageAsOf?.slice(0, 10);
    return `First Git appearance: ${firstSeen} · ${value.ageDays.toLocaleString()} days${asOf ? ` by ${asOf}` : ''}`;
  }

  function callableDescription(limb: FileOrganismLimb) {
    const bodySize = limb.bodyLineSpan == null ? 'body size unknown' : `${limb.bodyLineSpan.toLocaleString()} body lines`;
    const branching = limb.decisionCount == null ? 'branching unknown' : `${limb.decisionCount.toLocaleString()} decisions`;
    const arity = limb.arity == null ? 'arity unknown' : `${limb.arity} params`;
    return `${limb.kind} · ${visibilityLabel(limb.visibility)} · ${arity} · ${bodySize} · ${branching} · ${rangeLabel(limb)}`;
  }

  function changeGlyph(status: string | undefined) {
    return ({ added: '+', removed: '−', changed: 'Δ', unresolved: '?' } as Record<string, string>)[status || ''] || '';
  }

  function drawTile(target: HTMLCanvasElement | undefined, tile: { y: number; height: number }) {
    if (!target || loading || geometry.status === 'unavailable') return;
    const ratio = Math.max(1, Math.min(3, window.devicePixelRatio || 1));
    target.width = Math.ceil(canvasWidth * ratio);
    target.height = Math.ceil(tile.height * zoom * ratio);
    const context = target.getContext('2d');
    if (!context) return;
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, canvasWidth, tile.height * zoom);
    paintFileOrganism(context, geometry, {
      x: -geometry.bounds.minX * zoom,
      y: (-geometry.bounds.minY - tile.y) * zoom,
      scale: zoom,
      selectedCallableId: selectedKind === 'callable' ? selectedId : null,
      selectedStateId: selectedKind === 'state' ? selectedId : null,
      showCallableLabels: false,
      showStateLabels: true,
      labelFontSize: 11,
    });
  }

  function selectAtPointer(event: { clientX: number; clientY: number }) {
    if (!viewport) return;
    const extent = viewport.querySelector('.bug-specimen__extent');
    if (!extent) return;
    const rect = extent.getBoundingClientRect();
    const point = {
      x: (event.clientX - rect.left) / zoom + geometry.bounds.minX,
      y: (event.clientY - rect.top) / zoom + geometry.bounds.minY,
    };
    const callableId = hitTestFileOrganism(geometry, point);
    if (callableId) {
      selectedId = callableId;
      selectedKind = 'callable';
      return;
    }
    const state = hitTestFileOrganismState(geometry, point);
    if (state) {
      selectedId = state.id;
      selectedKind = 'state';
    }
  }

  function selectCallable(id: string) {
    selectedId = id;
    selectedKind = 'callable';
  }

  function selectState(id: string) {
    selectedId = id;
    selectedKind = 'state';
  }

  function changeZoom(factor: number, anchorX = stageWidth / 2, anchorY = 160) {
    if (!viewport) {
      zoom = Math.max(.25, Math.min(2, zoom * factor));
      onZoomChange?.(zoom);
      return;
    }
    const worldX = (viewport.scrollLeft + anchorX) / zoom;
    const worldY = (viewport.scrollTop + anchorY) / zoom;
    zoom = Math.max(.25, Math.min(2, zoom * factor));
    const nextZoom = zoom;
    onZoomChange?.(nextZoom);
    requestAnimationFrame(() => {
      if (!viewport) return;
      viewport.scrollLeft = Math.max(0, worldX * nextZoom - anchorX);
      viewport.scrollTop = Math.max(0, worldY * nextZoom - anchorY);
    });
  }

  function handleSpecimenWheel(event: WheelEvent) {
    if (!(event.ctrlKey || event.metaKey)) return;
    event.preventDefault();
    const rect = viewport?.getBoundingClientRect();
    if (!rect) return;
    changeZoom(event.deltaY < 0 ? 1.12 : 1 / 1.12, event.clientX - rect.left, event.clientY - rect.top);
  }

  function handleSpecimenKeyDown(event: KeyboardEvent) {
    if (!(event.ctrlKey || event.metaKey)) return;
    const key = event.key.toLowerCase();
    if (!['=', '+', '-', '_', 'numpadadd', 'numpadsubtract'].includes(key)) return;
    event.preventDefault();
    event.stopPropagation();
    changeZoom(['=', '+', 'numpadadd'].includes(key) ? 1.14 : 1 / 1.14);
  }

  function handleStageScroll() {
    if (!viewport) return;
    const centeredOffset = Math.max(0, (viewport.clientWidth - canvasWidth) / 2);
    onScrollWorldX?.(geometry.bounds.minX + (viewport.scrollLeft - centeredOffset) / zoom);
    onScrollWorld?.(viewport.scrollTop / zoom);
  }

  function handleCanvasKeyDown(event: KeyboardEvent, canvas: HTMLCanvasElement) {
    handleSpecimenKeyDown(event);
    if (event.defaultPrevented || (event.key !== 'Enter' && event.key !== ' ')) return;
    event.preventDefault();
    const rect = canvas.getBoundingClientRect();
    selectAtPointer({ clientX: rect.left + rect.width / 2, clientY: rect.top + rect.height / 2 });
  }

  $effect(() => {
    const identity = `${geometry.sourceId || ''}|${geometry.profileId || ''}|${geometry.file}|${geometry.limbs.length}|${geometry.stateSlots.length}`;
    if (selectedGeometryKey === identity) return;
    selectedGeometryKey = identity;
    selectedId = null;
    selectedKind = 'callable';
    if (zoomValue == null) zoom = 1;
  });

  $effect(() => {
    if (zoomValue != null && zoom !== zoomValue) zoom = zoomValue;
  });

  $effect(() => {
    if (scrollWorldX == null || !viewport) return;
    const centeredOffset = Math.max(0, (viewport.clientWidth - canvasWidth) / 2);
    const target = (scrollWorldX - geometry.bounds.minX) * zoom + centeredOffset;
    if (Math.abs(viewport.scrollLeft - target) > 1) viewport.scrollLeft = target;
  });

  $effect(() => {
    if (scrollWorldY == null || !viewport) return;
    const target = scrollWorldY * zoom;
    if (Math.abs(viewport.scrollTop - target) > 1) viewport.scrollTop = target;
  });

  $effect(() => {
    if (!viewport) return;
    const observer = new ResizeObserver(entries => { stageWidth = Math.floor(entries[0].contentRect.width); });
    observer.observe(viewport);
    return () => observer.disconnect();
  });

  $effect(() => {
    const currentGeometry = geometry;
    const currentTiles = tiles;
    const currentCanvases = canvases;
    selectedId;
    selectedKind;
    zoom;
    if (loading || currentGeometry.status === 'unavailable') return;
    const frame = requestAnimationFrame(() => currentTiles.forEach(tile => drawTile(currentCanvases[tile.index], tile)));
    return () => cancelAnimationFrame(frame);
  });
</script>

{#if loading}
  <section class="bug-specimen bug-specimen--pending" data-status="pending" aria-live="polite">
    <header class="bug-specimen__header">
      <span>SPECIMEN · READING SELECTED SNAPSHOT</span>
      <h3>{pendingFile || 'Selected file'}</h3>
      <p>Callable metrics pending · state pending</p>
      <p>First Git appearance pending · file size pending</p>
    </header>
    <div class="bug-specimen__controls">
      <div class="bug-specimen__legend" aria-label="Visual channel legend">
        <span>Shape = file role · shell = first-seen age</span>
        <span>Limb length = lines · thickness = decisions</span>
      </div>
      <div class="bug-specimen__zoom" aria-label="Specimen zoom controls">
        <button class="btn" aria-label="Zoom specimen out" disabled>−</button>
        <span>100%</span>
        <button class="btn" aria-label="Zoom specimen in" disabled>+</button>
        <button class="btn" disabled>1:1</button>
      </div>
    </div>
    <div class="bug-specimen__stage" bind:this={viewport} role="region" aria-label="Specimen diagram" onwheel={handleSpecimenWheel}>
      <div class="bug-specimen__inspection" role="status" aria-live="polite">Reading selected snapshot… diagram reserved.</div>
      <div class="bug-specimen__extent" style:width={`${canvasWidth}px`} style:height={`${canvasHeight}px`}></div>
    </div>
  </section>
{:else if geometry.status === 'unavailable'}
  <section class="bug-specimen bug-specimen--unavailable" data-status="unavailable">
    <div class="bug-specimen__unknown" aria-hidden="true">?</div>
    <h3>Anatomy unavailable</h3>
    <p>{anatomy?.reason || 'Callable anatomy is unavailable for this endpoint.'}</p>
    <p>{geometry.file || 'unknown'} · callables unknown · state unknown</p>
  </section>
{:else}
  <section class="bug-specimen" data-status={geometry.status}
    data-source-id={geometry.sourceId || ''} data-profile-id={geometry.profileId || ''}
    data-line-count={geometry.lineCount ?? 'unknown'} data-first-seen-at={geometry.firstSeenAt || 'unknown'}
    data-age-days={geometry.ageDays ?? 'unknown'} data-age-reason={geometry.ageReason || ''}
    data-callable-coverage={geometry.callableCoverage} data-state-coverage={geometry.stateCoverage}>
    <header class="bug-specimen__header" title={`Source ${geometry.sourceId || 'unknown'} · profile ${geometry.profileId || 'unknown'}`}>
      <span>SPECIMEN · {anatomy?.evidence || 'STRUCTURAL CAPTURE'} · {geometry.status}</span>
      <h3 title={geometry.file}>{geometry.file}</h3>
      <p>{geometry.lineCount == null ? 'File size unknown' : `${geometry.lineCount.toLocaleString()} physical lines`} · {geometry.callableCoverage === 'known' ? `${geometry.limbs.length.toLocaleString()} callables` : 'callables unknown'} · {geometry.stateCoverage === 'known' ? `${geometry.stateSlots.length} state slots` : 'state unknown'}</p>
      <p>{ageSummary(geometry)}</p>
      {#if geometry.interpretation}
        <div role="region" aria-label="Interpreted anatomy">
          Jev estimates · head suture = responsibility mixing: {geometry.interpretation.responsibilityMix.toFixed(2)} · inner shell = information hiding: {geometry.interpretation.informationHiding.toFixed(2)} / 2
        </div>
      {/if}
    </header>
    <div class="bug-specimen__controls">
      <div class="bug-specimen__legend" aria-label="Visual channel legend">
        <span>Shape = file role · shell = first-seen age</span>
        <span>Limb length = lines · thickness = decisions</span>
      </div>
      <div class="bug-specimen__zoom" aria-label="Specimen zoom controls">
        <button class="btn" aria-label="Zoom specimen out" onclick={() => changeZoom(1 / 1.25)} disabled={zoom <= .25}>−</button>
        <span>{Math.round(zoom * 100)}%</span>
        <button class="btn" aria-label="Zoom specimen in" onclick={() => changeZoom(1.25)} disabled={zoom >= 2}>+</button>
        <button class="btn" onclick={() => changeZoom(1 / zoom)}>1:1</button>
      </div>
    </div>
    <div class="bug-specimen__stage" bind:this={viewport} onscroll={handleStageScroll} onwheel={handleSpecimenWheel} role="region" aria-label="Specimen diagram">
      <div class="bug-specimen__inspection" role="region" aria-label="Selected organ" aria-live="polite">
        {#if selected && selectedKind === 'callable'}
          {@const limb = selected as FileOrganismLimb}
          <strong>{limb.displayName}</strong> · {callableDescription(limb)}
          {#if selectedChange}
            <span data-change-status={selectedChange.status || 'unresolved'}> · {selectedChange.status || 'change status unknown'} · {metricChangeLabel('body lines', 'bodyLineSpan', selectedChange)} · {metricChangeLabel('decisions', 'decisionCount', selectedChange)}</span>
          {/if}
        {:else if selected && selectedKind === 'state'}
          <strong>{selected.name}</strong> · state · {selected.kind} · {visibilityLabel(selected.visibility)} · {rangeLabel(selected)}
        {:else}
          Select an attached callable or state spot. Name and facts stay on the shared file geometry.
        {/if}
      </div>
      <div class="bug-specimen__extent" style:width={`${canvasWidth}px`} style:height={`${canvasHeight}px`}>
        <div class="bug-specimen__drawing" style:width={`${canvasWidth}px`} style:height={`${canvasHeight}px`}>
          {#each tiles as tile (tile.index)}
            <canvas bind:this={canvases[tile.index]} role="button" tabindex={tile.index === 0 ? 0 : undefined}
              aria-label={tile.index === 0 ? `Bug specimen for ${geometry.file}` : `Bug specimen tile ${tile.index + 1} for ${geometry.file}`}
              onclick={event => selectAtPointer(event)} onkeydown={event => handleCanvasKeyDown(event, event.currentTarget as HTMLCanvasElement)}
              style:top={`${tile.y * zoom}px`} style:width={`${canvasWidth}px`} style:height={`${tile.height * zoom}px`}></canvas>
          {/each}
          {#each geometry.limbs as limb (limb.id)}
            {@const hit = limb.hitRegion}
            {@const label = limb.labelRegion}
            {@const labelWidthPx = label.width * zoom}
            {@const labelLeft = limb.side < 0 ? (label.maxX - geometry.bounds.minX) * zoom - labelWidthPx : (label.minX - geometry.bounds.minX) * zoom}
            <button type="button" class="bug-specimen__organ" class:bug-specimen__organ--left={limb.side < 0}
              data-organ-kind="callable" data-organ-id={limb.id} data-visibility={limb.visibility || 'unknown'} data-change-status={callableChanges?.[limb.slotKey]?.status || undefined}
              data-slot-key={limb.slotKey} data-layout-y={limb.dataLayoutY}
              data-body-lines={limb.bodyLineSpan ?? 'unknown'} data-decisions={limb.decisionCount ?? 'unknown'}
              aria-label={`Function ${limb.displayName}`} aria-pressed={selectedId === limb.id}
              title={callableDescription(limb)}
              style:left={`${(hit.minX - geometry.bounds.minX) * zoom}px`} style:top={`${(hit.minY - geometry.bounds.minY) * zoom}px`}
              style:width={`${hit.width * zoom}px`} style:height={`${hit.height * zoom}px`}
              onkeydown={handleSpecimenKeyDown}
              onclick={() => selectCallable(limb.id)}>
              <span class="sr-only">{callableDescription(limb)}</span>
              <span class="bug-specimen__organ-label" style:left={`${labelLeft - (hit.minX - geometry.bounds.minX) * zoom}px`}
                style:top={`${(label.minY - hit.minY) * zoom}px`}
                style:width={`${labelWidthPx}px`} style:height={`${label.height * zoom}px`}>
                {#if changeGlyph(callableChanges?.[limb.slotKey]?.status)}
                  <span class="bug-specimen__change" data-change-status={callableChanges[limb.slotKey].status} aria-hidden="true">{changeGlyph(callableChanges[limb.slotKey].status)}</span>
                {/if}
                <span class="bug-specimen__name">{limb.displayName}</span>
              </span>
            </button>
          {/each}
          {#each geometry.absenceMarkers as marker (`${marker.slotKey}`)}
            <span class="bug-specimen__absence" data-organ-kind="absence" data-slot-key={marker.slotKey}
              aria-label={`Callable absent: ${marker.slotKey}`}
              style:left={`${(marker.x - geometry.bounds.minX - 34) * zoom}px`} style:top={`${(marker.y - geometry.bounds.minY - 12) * zoom}px`}>
              {changeGlyph(callableChanges?.[marker.slotKey]?.status) || '×'} Not present
            </span>
          {/each}
          {#each geometry.stateSlots as state (state.id)}
            <button type="button" class="bug-specimen__organ bug-specimen__organ--state"
              data-organ-kind="state" data-organ-id={state.id} aria-label={`State ${state.name}`} aria-pressed={selectedId === state.id}
              style:left={`${(state.hitRegion.minX - geometry.bounds.minX) * zoom}px`} style:top={`${(state.hitRegion.minY - geometry.bounds.minY) * zoom}px`}
              style:width={`${state.hitRegion.width * zoom}px`} style:height={`${state.hitRegion.height * zoom}px`}
              onkeydown={handleSpecimenKeyDown}
              title={`${state.kind} · ${visibilityLabel(state.visibility)} · ${rangeLabel(state)}`}
              onclick={() => selectState(state.id)}>
              <span class="sr-only">{state.name}</span>
            </button>
          {/each}
          {#if geometry.stateNote}
            <span class="bug-specimen__body-note" style:left={`${(geometry.body.x - geometry.bounds.minX + 8) * zoom}px`}
              style:top={`${(geometry.body.y + geometry.body.height - 24 - geometry.bounds.minY) * zoom}px`}>
              {geometry.stateNote}
            </span>
          {/if}
        </div>
      </div>
    </div>
    <p class="bug-specimen__evidence">One limb = one callable. Limb length and thickness use shared logarithmic scales; inspect raw values. Shell color/bands = age since first Git appearance. Absence marks reserve comparison slots; state spots remain separate. {anatomy?.reason || ''}</p>
  </section>
{/if}

<style>
  .bug-specimen { width: 100%; min-width: 0; box-sizing: border-box; padding: 12px; color: var(--text); background: var(--bg-100); border-radius: 0; }
  .bug-specimen__header { height: 88px; box-sizing: border-box; overflow: hidden; }
  .bug-specimen__header span { color: var(--secondary); font: 10px var(--font-mono); letter-spacing: .08em; }
  .bug-specimen h3 { overflow: hidden; margin: 6px 0; font: 600 13px var(--font-mono); text-overflow: ellipsis; white-space: nowrap; }
  .bug-specimen__header p, .bug-specimen__evidence { overflow: hidden; margin: 6px 0; color: var(--text-dim); font: 10px/1.5 var(--font-mono); text-overflow: ellipsis; white-space: nowrap; }
  .bug-specimen__controls { display: flex; min-width: 0; height: 40px; flex-wrap: nowrap; justify-content: space-between; align-items: center; gap: 8px; padding: 3px 0; overflow: hidden; }
  .bug-specimen__legend { display: flex; min-width: 0; max-width: calc(100% - 130px); flex: 1 1 auto; flex-wrap: nowrap; gap: 8px; align-items: center; overflow: hidden; font: 9px var(--font-mono); white-space: nowrap; }
  .bug-specimen__legend span { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .bug-specimen__zoom { display: flex; align-items: center; gap: 8px; font: 10px var(--font-mono); }
  .bug-specimen__zoom button { font-size: 10px; padding: 3px 8px; }
  .bug-specimen__stage { position: relative; width: 100%; min-width: 0; box-sizing: border-box; height: min(58vh, 420px); min-height: 180px; overflow: auto; border: 1px solid var(--bg-300); border-radius: 0; background: var(--bg); }
  .bug-specimen__inspection { position: sticky; left: 0; top: 0; z-index: 3; box-sizing: border-box; height: 64px; padding: 6px 10px; border-bottom: 1px solid var(--bg-300); color: var(--text-dim); background: var(--bg-100); font: 10px/1.4 var(--font-mono); overflow: auto; overflow-wrap: anywhere; }
  .bug-specimen__inspection strong { color: var(--secondary); }
  .bug-specimen__extent { position: relative; margin-inline: auto; }
  .bug-specimen__drawing { position: absolute; inset: 0 auto auto 0; }
  .bug-specimen__drawing canvas { position: absolute; left: 0; display: block; padding: 0; border: 0; border-radius: 0; background: transparent; cursor: crosshair; }
  .bug-specimen__organ { position: absolute; z-index: 2; display: block; padding: 0; border: 0; border-radius: 0; background: transparent; color: var(--text); cursor: pointer; overflow: visible; }
  .bug-specimen__organ:focus-visible { outline: 1px solid var(--primary); outline-offset: 1px; }
  .bug-specimen__organ-label { position: absolute; display: flex; align-items: center; overflow: visible; color: var(--text); text-align: left; white-space: nowrap; font: 10px/1.2 var(--font-mono); pointer-events: none; }
  .bug-specimen__organ--left .bug-specimen__organ-label { justify-content: flex-end; text-align: right; }
  .bug-specimen__name { display: block; flex: none; }
  .bug-specimen__change { display: inline-flex; width: 10px; height: 18px; flex: 0 0 10px; align-items: center; justify-content: center; margin-inline-end: 2px; color: var(--secondary); font-weight: 700; }
  .bug-specimen__absence { position: absolute; z-index: 2; color: var(--text-dim); font: 9px var(--font-mono); white-space: nowrap; }
  .bug-specimen__organ--state { background: transparent; }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }
  .bug-specimen__body-note { position: absolute; max-width: 70%; color: var(--text-dim); font: 9px var(--font-mono); overflow-wrap: anywhere; }
</style>
