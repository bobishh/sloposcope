<script>
  import { onDestroy, onMount } from 'svelte';

  let {
    bundleJs = '',
    surfaceId = 'generated-elm',
    moduleName = 'Main',
    flags = null,
    status = '',
    onmount = null,
    onerror = null,
  } = $props();

  let root = $state(null);
  let mountedBundle = $state('');
  let error = $state('');

  const scriptId = $derived(`sloposcope-${surfaceId}-script`);

  function validateBundle(js) {
    const source = String(js || '');
    if (!source.trim()) return 'Review content missing.';
    if (source.length > 3_000_000) return 'Review content too large.';
    if (!source.includes('Elm')) return 'Review content could not mount.';
    return '';
  }

  function disposeScript() {
    const oldScript = document.getElementById(scriptId);
    if (oldScript) oldScript.remove();
  }

  function mount(js, mountFlags = flags) {
    if (!root || typeof document === 'undefined') return;

    const validationError = validateBundle(js);
    if (validationError) {
      error = validationError;
      onerror?.(validationError);
      return;
    }

    root.innerHTML = '';
    disposeScript();

    try {
      const script = document.createElement('script');
      script.id = scriptId;
      script.text = String(js);
      document.head.appendChild(script);

      const module = window.Elm?.[moduleName];
      if (!module?.init) {
        throw new Error(`Elm.${moduleName}.init missing`);
      }

      module.init({ node: root, flags: mountFlags });
      mountedBundle = js;
      error = '';
      onmount?.();
    } catch (err) {
      const message = err?.message || String(err);
      error = message;
      onerror?.(message);
    }
  }

  function handleExternalMount(event) {
    const detail = event?.detail || {};
    if (detail.surfaceId && detail.surfaceId !== surfaceId) return;
    mount(detail.bundleJs || detail.payload || '', detail.flags ?? flags);
  }

  onMount(() => {
    window.addEventListener('sloposcope-mount-generated-elm', handleExternalMount);
    if (bundleJs) mount(bundleJs);
  });

  onDestroy(() => {
    if (typeof window !== 'undefined') {
      window.removeEventListener('sloposcope-mount-generated-elm', handleExternalMount);
    }
    if (typeof document !== 'undefined') disposeScript();
  });

  $effect(() => {
    if (bundleJs && bundleJs !== mountedBundle) {
      mount(bundleJs);
    }
  });
</script>

<div class="eyeloss-generated-elm" data-surface-id={surfaceId}>
  <div bind:this={root} class="eyeloss-generated-elm__root"></div>
  {#if !bundleJs && !mountedBundle && !error}
    <div class="eyeloss-generated-elm__empty">{status || 'Review waiting.'}</div>
  {/if}
  {#if error}
    <div class="eyeloss-generated-elm__error">{error}</div>
  {/if}
</div>

<style>
  .eyeloss-generated-elm {
    min-width: 0;
    min-height: 0;
    height: 100%;
    position: relative;
  }

  .eyeloss-generated-elm__root {
    min-width: 0;
    min-height: 0;
    height: 100%;
  }

  .eyeloss-generated-elm__empty,
  .eyeloss-generated-elm__error {
    padding: 18px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .eyeloss-generated-elm__error {
    color: #ffb4aa;
  }
</style>
