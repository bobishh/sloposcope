<script>
  import { onMount } from 'svelte';
  import { fade } from 'svelte/transition';
  import Eyeloss from './lib/Eyeloss.svelte';
  import { getFortune } from './lib/fortunes.js';

  // Tauri API mocks for browser/test environment
  let invoke = async (cmd, args) => {
    if (window.__TAURI_INTERNALS__) {
      const { invoke: tauriInvoke } = await import('@tauri-apps/api/core');
      console.log(`[FRONTEND] Invoking ${cmd}`, args);
      return tauriInvoke(cmd, args);
    }
    console.log('[MOCK] invoke', cmd, args);
    if (cmd === 'get_graph') return { nodes: [{ id: 'MockModule', type: 'module', file: 'mock.ex', line_count: 42, change_status: 'unchanged', functions: [] }], edges: [] };
    if (cmd === 'get_changes') return [
      { id: 'deadbeef', description: 'Working copy', timestamp: '2026-02-24 23:00' },
      { id: 'c0ffee11', description: 'Previous commit', timestamp: '2026-02-24 22:30' },
      { id: 'b0ba0000', description: 'Oldest commit', timestamp: '2026-02-24 22:00' }
    ];
    if (cmd === 'get_bookmarks') return [{ name: 'main', id: 'c0ffee11' }];
    if (cmd === 'get_current_branch') return 'main';
    if (cmd === 'get_repo_path') return '/path/to/despair';
    if (cmd === 'select_repo') return '/new/path/to/despair';
    if (cmd === 'get_file_source') return 'defmodule MockModule do\n  def hello, do: :world\nend';
    if (cmd === 'save_file') { console.log('[MOCK] saving file', args); return null; }
    return null;
  };

  let listen = async (event, cb) => {
    if (window.__TAURI_INTERNALS__) {
      const { listen: tauriListen } = await import('@tauri-apps/api/event');
      return tauriListen(event, cb);
    }
    console.log('[MOCK] listen', event);
    return () => {};
  };

  let graph = $state({ nodes: [], edges: [] });
  let changes = $state([]);
  let bookmarks = $state([]);
  let currentBranch = $state('');
  let since = $state('@'); 
  let loading = $state(true); // Initial load
  let refreshing = $state(false); // Update state
  let currentFortune = $state('');
  let repoPath = $state('');
  let showDropdown = $state(false);
  let highlightedIndex = $state(-1);

  // Heatmap state
  let touchHeat = $state(new Map());
  let heatCounter = 0;

  function updateFortune() {
    const committers = changes.map(c => c.description.split(' ')[0]).filter(n => n && n.length > 2);
    currentFortune = getFortune(committers);
  }

  function getShortName(id) {
    if (!id) return '';
    
    if (id.includes('/')) {
      const parts = id.split('/');
      return parts[parts.length - 1]; // Return full filename: "config.exs"
    }
    
    if (id.includes('.')) {
      const parts = id.split('.');
      const last = parts[parts.length - 1];
      
      // Known file extensions
      const knownExts = ['ex', 'exs', 'svelte', 'js', 'ts', 'jsx', 'tsx', 'rs', 'py', 'rb', 'go', 'java', 'cpp', 'h', 'php', 'cs', 'json', 'md'];
      if (knownExts.includes(last.toLowerCase())) {
        return id; 
      }
      return last;
    }
    
    return id;
  }

  let filteredItems = $derived.by(() => {
    const q = (since || '').toLowerCase();
    
    const matchedBookmarks = bookmarks
      .filter(b => b.name.toLowerCase().includes(q))
      .map(b => ({ id: b.name, description: 'Bookmark', type: 'bookmark' }));
      
    const matchedChanges = changes
      .filter(c => c.id.toLowerCase().includes(q) || c.description.toLowerCase().includes(q))
      .map(c => ({ ...c, type: 'commit' }));

    return [...matchedBookmarks, ...matchedChanges];
  });

  async function loadGraph(isInitial = false) {
    if (isInitial) loading = true;
    else refreshing = true;
    
    updateFortune();

    const sinceArg = since && since !== '' ? since : null;
    try {
      console.log(`[FRONTEND] Starting loadGraph (initial: ${isInitial}) since: ${sinceArg}`);
      const [g, c, b, curr] = await Promise.all([
        invoke('get_graph', { since: sinceArg }),
        invoke('get_changes', { limit: 20 }),
        invoke('get_bookmarks'),
        invoke('get_current_branch'),
      ]);
      console.log(`[FRONTEND] Received data. Nodes: ${g.nodes.length}`);
      
      // If initial load of working copy/HEAD is empty, fallback to the last actual commit
      if (isInitial && g.nodes.length === 0 && (since === '@' || since === 'HEAD') && c.length > 1) {
        const fallback = c[1].id;
        console.log(`[FRONTEND] Initial graph for ${since} is empty, falling back to ${fallback}`);
        since = fallback;
        const g2 = await invoke('get_graph', { since: fallback });
        graph = g2;
      } else {
        graph = g;
      }

      changes = c;
      bookmarks = b;
      currentBranch = curr;
    } catch (e) {
      console.error('[FRONTEND] Error loading graph:', e);
    } finally {
      loading = false;
      refreshing = false;
    }
  }

  function setSince(revset, event) {
    if (event && (event.shiftKey || event.metaKey || event.ctrlKey)) {
      // Toggle logic for multi-select
      const current = (since || '').split(' | ').filter(s => s && s !== '');
      if (current.includes(revset)) {
        since = current.filter(s => s !== revset).join(' | ');
      } else {
        since = [...current, revset].join(' | ');
      }
    } else {
      since = revset;
    }
    
    if (!since || since === '') since = '@';
    console.log(`[FRONTEND] Setting since to: ${since}`);
    loadGraph(false);
  }

  async function selectRepo() {
    try {
      const newPath = await invoke('select_repo');
      repoPath = newPath;
      await loadGraph(true);
    } catch (e) {
      console.log('Dialog cancelled or failed', e);
    }
  }

  let theme = $state(typeof localStorage !== 'undefined' ? (localStorage.getItem('codegraph-theme') || 'midnight') : 'midnight');

  function toggleTheme() {
    theme = theme === 'midnight' ? 'victorian' : 'midnight';
    if (typeof localStorage !== 'undefined') localStorage.setItem('codegraph-theme', theme);
    applyTheme();
  }

  function applyTheme() {
    document.documentElement.setAttribute('data-theme', theme);
  }

  async function getFileSource(file) {
    return invoke('get_file_source', { file });
  }

  async function getFileDiff(file) {
    return invoke('get_file_diff', { file, since: since === '@' ? null : since });
  }

  async function saveFile(file, content) {
    return invoke('save_file', { file, content });
  }

  onMount(async () => {
    applyTheme();
    repoPath = await invoke('get_repo_path');
    await loadGraph(true);

    listen('graph-updated', (event) => {
      console.log('[FRONTEND] Graph updated event received');
      graph = event.payload.graph;
      changes = event.payload.changes;
    });

    listen('file-touched', (event) => {
      const path = event.payload;
      console.log(`[FRONTEND] File touched: ${path}`);
      
      touchHeat.set(path, ++heatCounter);
      if (touchHeat.size > 100) {
        const oldestKey = touchHeat.keys().next().value;
        touchHeat.delete(oldestKey);
      }
      // Force reactivity in Svelte 5 for Map
      touchHeat = new Map(touchHeat);
    });
  });
</script>

<div class="eyeloss-page">
  {#if refreshing || loading}
    <div class="eyeloss-refresh-overlay" transition:fade={{ duration: 200 }}>
      <div class="eyeloss-spinner">
        <div class="eyeloss-spinner__dot"></div>
      </div>
      <div class="eyeloss-splash__fortune">
        {currentFortune}
      </div>
    </div>
  {/if}

  <div class="eyeloss-controls">
    <div class="eyeloss-controls__left">
      <div class="eyeloss-controls__row">
        <span class="eyeloss-controls__repo">{repoPath}</span>
        <button type="button" class="btn btn-ghost" onclick={selectRepo} style="font-size: 0.6rem; padding: 2px 6px;">Change</button>
      </div>
      <div class="eyeloss-controls__row">
        <span class="eyeloss-controls__branch">branch: {currentBranch}</span>
      </div>
    </div>

    <div class="eyeloss-controls__center">
      <form 
        onsubmit={(e) => { e.preventDefault(); setSince(since); showDropdown = false; }}
        style="position: relative;"
      >
        <label for="since-input">Since</label>
        <div style="position: relative; display: inline-block;">
          <input
            id="since-input"
            name="since"
            type="text"
            bind:value={since}
            onfocus={() => showDropdown = true}
            onblur={() => setTimeout(() => showDropdown = false, 200)}
            onkeydown={(e) => {
              if (e.key === 'ArrowDown') {
                e.preventDefault();
                highlightedIndex = Math.min(highlightedIndex + 1, filteredItems.length - 1);
              } else if (e.key === 'ArrowUp') {
                e.preventDefault();
                highlightedIndex = Math.max(highlightedIndex - 1, 0);
              } else if (e.key === 'Enter' && highlightedIndex >= 0) {
                e.preventDefault();
                setSince(filteredItems[highlightedIndex].id);
                showDropdown = false;
              } else if (e.key === 'Escape') {
                showDropdown = false;
              }
            }}
            placeholder="@ | @- | ancestors(@, 5)"
            class="input-mono"
            autocomplete="off"
          />
          {#if showDropdown && filteredItems.length > 0}
            <div class="eyeloss-dropdown">
              {#each filteredItems as item, i}
                <button
                  type="button"
                  class="eyeloss-dropdown__item"
                  class:eyeloss-dropdown__item--highlighted={i === highlightedIndex}
                  onclick={() => { setSince(item.id); showDropdown = false; }}
                >
                  <span class="eyeloss-dropdown__id">
                    {getShortName(item.id)} 
                    {#if item.type === 'bookmark'}<small style="opacity: 0.5; font-weight: normal; margin-left: 4px;">(bookmark)</small>{/if}
                  </span>
                  <span class="eyeloss-dropdown__desc">{item.description}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
        <button type="submit" class="btn btn-primary">Apply</button>
        {#if since}
          <button type="button" class="btn btn-ghost" onclick={() => { setSince(null); since = ''; }}>Clear</button>
        {/if}
      </form>
    </div>

    <div class="eyeloss-controls__right">
      <button type="button" class="btn btn-ghost theme-toggle" onclick={toggleTheme} title="Toggle Theme">
        {theme === 'midnight' ? '☀️' : '🌙'}
      </button>
    </div>
  </div>

  <Eyeloss 
    {graph} 
    {since} 
    {changes} 
    {bookmarks} 
    {getFileDiff} 
    {getFileSource} 
    {saveFile} 
    {theme} 
    {touchHeat}
    onSelectSince={setSince} 
  />
</div>
