<script>
  import { onMount } from 'svelte';
  import { fade } from 'svelte/transition';
  import Sloposcope from './lib/Sloposcope.svelte';
  import { getFortune } from './lib/fortunes.js';
  const CHANGES_PAGE_SIZE = 20;
  const ALL_CHANGES_LIMIT = 50000;
  const DEBUG_LOGS = (() => {
    try {
      if (typeof window === 'undefined') return false;
      const qs = new URLSearchParams(window.location.search);
      if (qs.get('debug') === '1') return true;
      if (qs.get('debug') === '0') return false;
      const stored = localStorage.getItem('sloposcope-debug') ?? localStorage.getItem('eyeloss-debug');
      if (stored === '1') return true;
      if (stored === '0') return false;
      return import.meta.env.DEV;
    } catch (_e) {
      return import.meta.env.DEV;
    }
  })();

  function debugLog(...args) {
    if (DEBUG_LOGS) console.log(...args);
  }

  function normalizeTouchedPath(path) {
    if (!path) return '';
    return String(path)
      .replaceAll('\\', '/')
      .replace(/^\.\/+/, '')
      .replace(/^\/+/, '');
  }

  function seedHeatmapFromGraph(g, targetProjectId = activeProjectId) {
    const project = getProjectById(targetProjectId);
    if (!project) return;
    if (!g || !Array.isArray(g.nodes)) return;
    let changed = false;
    for (const node of g.nodes) {
      const status = String(node?.change_status || '');
      if (status !== 'added' && status !== 'modified') continue;
      const file = normalizeTouchedPath(node?.file || node?.id);
      if (!file) continue;
      if (!project.heatmapData.has(file)) {
        project.heatCounter = (project.heatCounter || 0) + 1;
        project.heatmapData.set(file, { seq: project.heatCounter, touchedAt: Date.now() });
        changed = true;
      }
    }
    if (changed && activeProjectId === targetProjectId) {
      heatCounter = project.heatCounter || 0;
      heatmapData = new Map(project.heatmapData);
    }
  }

  function traceNow() {
    try {
      return new Date().toISOString();
    } catch (_e) {
      return 'unknown-time';
    }
  }

  function summarizeResult(value) {
    if (Array.isArray(value)) return `array(len=${value.length})`;
    if (value && typeof value === 'object') {
      if (Array.isArray(value.nodes) && Array.isArray(value.edges)) {
        return `graph(nodes=${value.nodes.length}, edges=${value.edges.length})`;
      }
      const keys = Object.keys(value);
      return `object(keys=${keys.length})`;
    }
    return String(value);
  }

  async function invokeTraced(cmd, args, options = {}) {
    const warnAfterMs = Number(options.warnAfterMs ?? 5000);
    const started = performance.now();
    debugLog(`[FRONTEND][TRACE] ${traceNow()} -> ${cmd}`, args);
    const warnTimer = setTimeout(() => {
      const elapsed = Math.round(performance.now() - started);
      console.warn(`[FRONTEND][TRACE] ${traceNow()} still waiting for ${cmd} (${elapsed}ms elapsed)`, args);
    }, warnAfterMs);

    try {
      const result = await invoke(cmd, args);
      const elapsed = Math.round(performance.now() - started);
      debugLog(`[FRONTEND][TRACE] ${traceNow()} <- ${cmd} (${elapsed}ms) ${summarizeResult(result)}`);
      return result;
    } catch (error) {
      const elapsed = Math.round(performance.now() - started);
      console.error(`[FRONTEND][TRACE] ${traceNow()} !! ${cmd} failed after ${elapsed}ms`, error, args);
      throw error;
    } finally {
      clearTimeout(warnTimer);
    }
  }

  const MOCK_CHANGES = [
    { id: 'deadbeef', description: 'Working copy', timestamp: '2026-02-24 23:00' },
    { id: 'c0ffee11', description: 'Refactor parser pipeline', timestamp: '2026-02-24 22:30' },
    { id: 'b0ba0000', description: 'Fix timeline hover metadata', timestamp: '2026-02-24 22:00' },
    { id: 'fadedcab', description: 'Improve graph edge resolver', timestamp: '2026-02-24 21:30' },
    { id: 'decafbad', description: 'Add file watcher heat map', timestamp: '2026-02-24 21:00' },
    { id: 'bead1234', description: 'Tidy css variables', timestamp: '2026-02-24 20:30' },
    { id: '8badf00d', description: 'Add bookmarks API', timestamp: '2026-02-24 20:00' },
    { id: 'facefeed', description: 'Stabilize canvas zoom', timestamp: '2026-02-24 19:30' },
    { id: 'ab12cd34', description: 'Introduce code window', timestamp: '2026-02-24 19:00' },
    { id: '3412dcba', description: 'Bootstrap initial graph renderer', timestamp: '2026-02-24 18:30' },
  ];

  function uniqueChanges(list) {
    const seen = new Set();
    const result = [];
    for (const change of list || []) {
      const id = change?.id;
      if (!id || seen.has(id)) continue;
      seen.add(id);
      result.push(change);
    }
    return result;
  }

  // Tauri API mocks for browser/test environment
  let invoke = async (cmd, args) => {
    if (window.__TAURI_INTERNALS__) {
      const { invoke: tauriInvoke } = await import('@tauri-apps/api/core');
      debugLog(`[FRONTEND] Invoking ${cmd}`, args);
      return tauriInvoke(cmd, args);
    }
    debugLog('[MOCK] invoke', cmd, args);
    if (cmd === 'get_graph') {
      return {
        nodes: [
          { id: 'MockModule', type: 'module', file: 'mock.ex', line_count: 42, change_status: 'unchanged', functions: [] },
          { id: 'MockHelper', type: 'module', file: 'mock_helper.ex', line_count: 12, change_status: 'modified', functions: [] },
        ],
        edges: [{ source: 'MockHelper', target: 'MockModule', type: 'call' }],
      };
    }
    if (cmd === 'get_changes') {
      const limit = Math.max(1, Number(args?.limit || 20));
      const beforeId = args?.before_id;
      if (!beforeId) return MOCK_CHANGES.slice(0, limit);
      const cursorIndex = MOCK_CHANGES.findIndex(c => c.id === beforeId);
      if (cursorIndex < 0) return [];
      return MOCK_CHANGES.slice(cursorIndex + 1, cursorIndex + 1 + limit);
    }
    if (cmd === 'get_bookmarks') return [{ name: 'main', id: 'c0ffee11' }];
    if (cmd === 'get_current_branch') return 'main';
    if (cmd === 'get_repo_path') return '/path/to/despair';
    if (cmd === 'select_repo') return '/new/path/to/despair';
    if (cmd === 'set_repo_path') return args?.path || '/path/to/despair';
    if (cmd === 'get_file_source') return 'defmodule MockModule do\n  def hello, do: :world\nend';
    if (cmd === 'get_file_diff') return '@@ -1,2 +1,3 @@\n defmodule MockModule do\n+  def goodbye, do: :moon\n   def hello, do: :world\n end';
    if (cmd === 'save_file') { debugLog('[MOCK] saving file', args); return null; }
    return null;
  };

  let listen = async (event, cb) => {
    if (window.__TAURI_INTERNALS__) {
      const { listen: tauriListen } = await import('@tauri-apps/api/event');
      return tauriListen(event, cb);
    }
    debugLog('[MOCK] listen', event);
    return () => {};
  };

  let graph = $state({ nodes: [], edges: [] });
  let changes = $state([]);
  let changesLimit = $state(CHANGES_PAGE_SIZE);
  let hasMoreChanges = $state(true);
  let bookmarks = $state([]);
  let currentBranch = $state('');
  let currentRevision = $state('');
  let since = $state('@'); 
  let loading = $state(true); // Initial load
  let refreshing = $state(false); // Update state
  let currentFortune = $state('');
  let repoPath = $state('');
  let repoActionMessage = $state('');
  let showDropdown = $state(false);
  let highlightedIndex = $state(-1);
  let loadGraphRequestSeq = 0;
  let inFlightLoadGraph = 0;
  let loadingMore = $state(false);

  // Heatmap state
  let heatmapData = $state(new Map());
  let heatCounter = 0;
  let fileContentVersions = $state(new Map());
  let contentVersion = $derived.by(() => {
    const repo = repoPath || '';
    const revision = currentRevision || '';
    const sinceValue = since || '';
    return `${repo}|${revision}|${sinceValue}`;
  });

  // Multi-project tabs (one active project; inactive projects sleep).
  let projectTabs = $state([]);
  let activeProjectId = $state(null);

  function shortRepoLabel(path) {
    if (!path) return '(unknown repo)';
    const normalized = String(path).replaceAll('\\', '/').replace(/\/+$/, '');
    const parts = normalized.split('/');
    return parts[parts.length - 1] || normalized;
  }

  function extractErrorMessage(error) {
    if (!error) return '';
    if (typeof error === 'string') return error;
    if (typeof error === 'object') {
      if (typeof error.message === 'string') return error.message;
      try {
        return JSON.stringify(error);
      } catch (_e) {
        return String(error);
      }
    }
    return String(error);
  }

  function setRepoActionMessage(message) {
    repoActionMessage = message ? String(message) : '';
  }

  function createProjectSession(path) {
    const normalizedPath = String(path || '');
    return {
      id: normalizedPath,
      path: normalizedPath,
      label: shortRepoLabel(normalizedPath),
      graph: { nodes: [], edges: [] },
      changes: [],
      changesLimit: CHANGES_PAGE_SIZE,
      hasMoreChanges: true,
      bookmarks: [],
      currentBranch: '',
      currentRevision: '',
      since: '@',
      heatmapData: new Map(),
      heatCounter: 0,
      fileContentVersions: new Map(),
      loadingMore: false,
    };
  }

  function getProjectById(id) {
    if (!id) return null;
    return projectTabs.find((p) => p.id === id) || null;
  }

  function getProjectByPath(path) {
    if (!path) return null;
    const normalizedPath = String(path);
    return projectTabs.find((p) => p.path === normalizedPath) || null;
  }

  function getActiveProject() {
    return getProjectById(activeProjectId);
  }

  function upsertProject(path) {
    const existing = getProjectByPath(path);
    if (existing) return existing;
    const created = createProjectSession(path);
    projectTabs = [...projectTabs, created];
    return created;
  }

  function applyProjectToGlobals(project) {
    if (!project) return;
    repoPath = project.path;
    graph = project.graph;
    changes = project.changes;
    changesLimit = project.changesLimit;
    hasMoreChanges = project.hasMoreChanges;
    bookmarks = project.bookmarks;
    currentBranch = project.currentBranch;
    currentRevision = project.currentRevision;
    since = project.since;
    heatCounter = project.heatCounter || 0;
    heatmapData = new Map(project.heatmapData || new Map());
    fileContentVersions = new Map(project.fileContentVersions || new Map());
    loadingMore = Boolean(project.loadingMore);
    refreshFortune();
  }

  function syncGlobalsToActiveProject() {
    const project = getActiveProject();
    if (!project) return;
    project.path = repoPath;
    project.label = shortRepoLabel(repoPath);
    project.graph = graph;
    project.changes = changes;
    project.changesLimit = changesLimit;
    project.hasMoreChanges = hasMoreChanges;
    project.bookmarks = bookmarks;
    project.currentBranch = currentBranch;
    project.currentRevision = currentRevision;
    project.since = since || '@';
    project.heatCounter = heatCounter;
    project.heatmapData = new Map(heatmapData);
    project.fileContentVersions = new Map(fileContentVersions);
    project.loadingMore = loadingMore;
  }

  function refreshFortune() {
    try {
      updateFortune();
    } catch (_e) {
      // no-op
    }
    if (!currentFortune || currentFortune.trim() === '') {
      currentFortune = getFortune([]);
    }
  }

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

  async function loadTimelineChanges(targetProjectId = activeProjectId) {
    const project = getProjectById(targetProjectId);
    if (!project) return;
    const c = await invokeTraced('get_changes', { limit: ALL_CHANGES_LIMIT }, { warnAfterMs: 8000 });
    const unique = uniqueChanges(c);
    debugLog(`[FRONTEND][TRACE] timeline fetched raw=${c?.length || 0}, unique=${unique.length}`);
    project.changes = unique;
    project.changesLimit = unique.length;
    project.hasMoreChanges = false; // We paginate locally after prefetching full history.

    if (activeProjectId === targetProjectId) {
      changes = unique;
      changesLimit = unique.length;
      hasMoreChanges = false;
      refreshFortune();
    }
  }

  async function loadGraph(isInitial = false, targetProjectId = activeProjectId) {
    const project = getProjectById(targetProjectId);
    if (!project) return;
    const requestId = ++loadGraphRequestSeq;
    const requestStartedAt = performance.now();
    const previousBranch = project.currentBranch || '';
    const sinceArg = project.since && project.since !== '' ? project.since : null;
    inFlightLoadGraph += 1;
    debugLog(
      `[FRONTEND][BRANCH][req:${requestId}] loadGraph start initial=${isInitial} since=${sinceArg} branch='${previousBranch}' inFlight=${inFlightLoadGraph}`
    );

    if (activeProjectId === targetProjectId) {
      if (isInitial) loading = true;
      else refreshing = true;
      refreshFortune();
    }

    // Safety timeout to clear loader if backend hangs
    const safetyTimeout = setTimeout(() => {
      if (activeProjectId === targetProjectId) {
        loading = false;
        refreshing = false;
      }
    }, 10000);

    try {
      const [g, b, curr] = await Promise.all([
        invokeTraced('get_graph', { since: sinceArg }, { warnAfterMs: 8000 }),
        invokeTraced('get_bookmarks', {}, { warnAfterMs: 8000 }),
        invokeTraced('get_current_branch', {}, { warnAfterMs: 8000 }),
      ]);

      const elapsedMs = Math.round(performance.now() - requestStartedAt);
      debugLog(
        `[FRONTEND][BRANCH][req:${requestId}] loadGraph done in ${elapsedMs}ms nodes=${g.nodes.length} edges=${g.edges.length} bookmarks=${b.length} branch='${previousBranch}'->'${curr}'`
      );
      project.graph = g;
      project.bookmarks = b;
      project.currentBranch = curr;
      seedHeatmapFromGraph(g, targetProjectId);
      if (activeProjectId === targetProjectId) {
        graph = g;
        bookmarks = b;
        currentBranch = curr;
      }
    } catch (e) {
      console.error(`[FRONTEND][BRANCH][req:${requestId}] loadGraph failed`, e);
    } finally {
      clearTimeout(safetyTimeout);
      if (activeProjectId === targetProjectId) {
        loading = false;
        refreshing = false;
      }
      inFlightLoadGraph = Math.max(0, inFlightLoadGraph - 1);
      debugLog(`[FRONTEND][BRANCH][req:${requestId}] loadGraph finish inFlight=${inFlightLoadGraph}`);
    }
  }

  async function loadMoreChanges(targetProjectId = activeProjectId) {
    const project = getProjectById(targetProjectId);
    if (!project) return 0;
    if (project.loadingMore || project.changes.length === 0 || !project.hasMoreChanges) return 0;
    if (activeProjectId === targetProjectId) {
      refreshFortune();
      loadingMore = true;
    }
    project.loadingMore = true;
    let loadedCount = 0;
    try {
      const targetLimit = Math.max(project.changes.length, project.changesLimit) + CHANGES_PAGE_SIZE;
      const expanded = await invokeTraced('get_changes', { limit: targetLimit }, { warnAfterMs: 8000 });
      const expandedUnique = uniqueChanges(expanded);
      const existingIds = new Set(project.changes.map(c => c.id));
      const tail = expandedUnique.filter(c => !existingIds.has(c.id));

      if (tail.length > 0) {
        project.changes = [...project.changes, ...tail];
        loadedCount = tail.length;
      }

      project.changesLimit = Math.max(project.changesLimit, expandedUnique.length);
      if (expandedUnique.length < targetLimit || loadedCount === 0) {
        project.hasMoreChanges = false;
      }
      if (activeProjectId === targetProjectId) {
        changes = project.changes;
        changesLimit = project.changesLimit;
        hasMoreChanges = project.hasMoreChanges;
      }
    } catch (e) {
      console.error('[FRONTEND] Error loading more changes:', e);
    } finally {
      project.loadingMore = false;
      if (activeProjectId === targetProjectId) loadingMore = false;
    }
    return loadedCount;
  }

  function setSince(revset, event) {
    const previousSince = since;
    const previousBranch = currentBranch;
    const isMultiToggle = Boolean(event && (event.shiftKey || event.metaKey || event.ctrlKey));
    const isBookmark = bookmarks.some((b) => b.name === revset || b.id === revset);
    debugLog(
      `[FRONTEND][BRANCH] setSince input='${revset}' prev='${previousSince}' branch='${previousBranch}' multi=${isMultiToggle} bookmarkMatch=${isBookmark}`
    );
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
    const active = getActiveProject();
    if (active) {
      active.since = since;
    }
    debugLog(`[FRONTEND][BRANCH] setSince resolved='${since}' (from '${previousSince}') -> triggering loadGraph`);
    loadGraph(false, activeProjectId);
  }

  async function activateProjectTab(
    projectId,
    { skipBackendSet = false, isInitial = false, waitForRefresh = false } = {}
  ) {
    const next = getProjectById(projectId);
    if (!next) return;

    if (activeProjectId !== projectId) {
      syncGlobalsToActiveProject();
      activeProjectId = projectId;
      applyProjectToGlobals(next);
    }

    showDropdown = false;
    highlightedIndex = -1;

    if (!skipBackendSet) {
      try {
        const switchedPath = await invokeTraced(
          'set_repo_path',
          { path: next.path },
          { warnAfterMs: 10000 }
        );
        next.path = switchedPath;
        next.label = shortRepoLabel(switchedPath);
        repoPath = switchedPath;
      } catch (e) {
        console.error('[FRONTEND] Failed to activate project tab:', e);
        return;
      }
    }

    const refreshPromise = Promise.all([
      loadTimelineChanges(projectId),
      loadGraph(isInitial, projectId),
    ]);

    if (waitForRefresh) {
      await refreshPromise;
    } else {
      refreshPromise.catch((e) => console.error('[FRONTEND] Project refresh failed:', e));
    }
  }

  function closeProjectTab(projectId, event) {
    event?.stopPropagation?.();
    if (projectTabs.length <= 1) return;
    const idx = projectTabs.findIndex((p) => p.id === projectId);
    if (idx < 0) return;

    const wasActive = activeProjectId === projectId;
    const remaining = projectTabs.filter((p) => p.id !== projectId);
    projectTabs = remaining;

    if (!wasActive) return;
    const next = remaining[Math.max(0, idx - 1)] || remaining[0];
    if (!next) return;
    activateProjectTab(next.id).catch((e) =>
      console.error('[FRONTEND] Failed to activate fallback project tab:', e)
    );
  }

  function isEditableTarget(target) {
    if (!(target instanceof HTMLElement)) return false;
    return (
      target.tagName === 'INPUT' ||
      target.tagName === 'TEXTAREA' ||
      target.tagName === 'SELECT' ||
      target.isContentEditable
    );
  }

  function activateRelativeProjectTab(step) {
    const total = projectTabs.length;
    if (total <= 1) return;
    const currentIndex = projectTabs.findIndex((project) => project.id === activeProjectId);
    if (currentIndex < 0) return;
    const nextIndex = (currentIndex + step + total) % total;
    const nextProject = projectTabs[nextIndex];
    if (!nextProject || nextProject.id === activeProjectId) return;
    activateProjectTab(nextProject.id).catch((e) =>
      console.error('[FRONTEND] Failed to activate project tab from hotkey:', e)
    );
  }

  function activateProjectTabByNumber(numberKey) {
    const total = projectTabs.length;
    if (total === 0) return;
    const index = numberKey === 9 ? total - 1 : Math.min(numberKey - 1, total - 1);
    const targetProject = projectTabs[index];
    if (!targetProject || targetProject.id === activeProjectId) return;
    activateProjectTab(targetProject.id).catch((e) =>
      console.error('[FRONTEND] Failed to activate numbered project tab:', e)
    );
  }

  function handleAppHotkeys(event) {
    if (!(event.metaKey || event.ctrlKey)) return;
    if (event.altKey) return;
    if (isEditableTarget(event.target)) return;
    const key = String(event.key || '').toLowerCase();

    if (key === 'n') {
      event.preventDefault();
      selectRepo();
      return;
    }

    if (key === 'o') {
      event.preventDefault();
      changeCurrentRepo();
      return;
    }

    if (event.key === ']' || event.code === 'BracketRight') {
      event.preventDefault();
      activateRelativeProjectTab(1);
      return;
    }

    if (event.key === '[' || event.code === 'BracketLeft') {
      event.preventDefault();
      activateRelativeProjectTab(-1);
      return;
    }

    const digit = Number(event.key);
    if (Number.isInteger(digit) && digit >= 1 && digit <= 9) {
      event.preventDefault();
      activateProjectTabByNumber(digit);
    }
  }

  async function selectRepo() {
    try {
      const newPath = await invokeTraced('select_repo', {}, { warnAfterMs: 10000 });
      debugLog(`[FRONTEND][TRACE] repo selected: ${newPath}`);
      setRepoActionMessage('');
      const project = upsertProject(newPath);
      project.path = newPath;
      project.label = shortRepoLabel(newPath);
      await activateProjectTab(project.id, {
        skipBackendSet: true,
        isInitial: true,
        waitForRefresh: true,
      });
    } catch (e) {
      const message = extractErrorMessage(e);
      const lowered = message.toLowerCase();
      if (lowered.includes('no folder selected') || lowered.includes('dialog cancelled')) {
        debugLog('Dialog cancelled', e);
        return;
      }
      setRepoActionMessage(message || 'Failed to open selected directory.');
      console.error('[FRONTEND] selectRepo failed:', e);
    }
  }

  async function changeCurrentRepo() {
    const current = getActiveProject();
    if (!current) return;
    try {
      const newPath = await invokeTraced('select_repo', {}, { warnAfterMs: 10000 });
      debugLog(`[FRONTEND][TRACE] change current repo -> ${newPath}`);
      setRepoActionMessage('');

      const duplicate = getProjectByPath(newPath);
      if (duplicate && duplicate.id !== current.id) {
        const remaining = projectTabs.filter((p) => p.id !== current.id);
        projectTabs = remaining;
        await activateProjectTab(duplicate.id, {
          skipBackendSet: true,
          isInitial: true,
          waitForRefresh: true,
        });
        return;
      }

      if (newPath === current.path) {
        await activateProjectTab(current.id, {
          skipBackendSet: true,
          isInitial: true,
          waitForRefresh: true,
        });
        return;
      }

      const replacement = createProjectSession(newPath);
      const index = projectTabs.findIndex((p) => p.id === current.id);
      if (index >= 0) {
        const nextTabs = [...projectTabs];
        nextTabs[index] = replacement;
        projectTabs = nextTabs;
      } else {
        projectTabs = [replacement];
      }
      activeProjectId = replacement.id;
      applyProjectToGlobals(replacement);
      await activateProjectTab(replacement.id, {
        skipBackendSet: true,
        isInitial: true,
        waitForRefresh: true,
      });
    } catch (e) {
      const message = extractErrorMessage(e);
      const lowered = message.toLowerCase();
      if (lowered.includes('no folder selected') || lowered.includes('dialog cancelled')) {
        debugLog('Dialog cancelled', e);
        return;
      }
      setRepoActionMessage(message || 'Failed to change directory.');
      console.error('[FRONTEND] changeCurrentRepo failed:', e);
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
    return invokeTraced('get_file_source', { file }, { warnAfterMs: 6000 });
  }

  async function getFileDiff(file) {
    return invokeTraced('get_file_diff', { file, since: since === '@' ? null : since }, { warnAfterMs: 6000 });
  }

  async function saveFile(file, content) {
    return invokeTraced('save_file', { file, content }, { warnAfterMs: 6000 });
  }

  onMount(async () => {
    const mountStart = performance.now();
    debugLog(`[FRONTEND][TRACE] ${traceNow()} onMount start`);
    applyTheme();
    debugLog(`[FRONTEND][TRACE] ${traceNow()} theme applied`);
    refreshFortune();
    debugLog(`[FRONTEND][TRACE] ${traceNow()} fortune refreshed`);
    const initialRepoPath = await invokeTraced('get_repo_path', {}, { warnAfterMs: 8000 });
    const initialProject = upsertProject(initialRepoPath);
    activeProjectId = initialProject.id;
    applyProjectToGlobals(initialProject);
    debugLog(`[FRONTEND][TRACE] ${traceNow()} repo path resolved: ${initialRepoPath}`);
    await Promise.all([
      loadTimelineChanges(initialProject.id),
      loadGraph(true, initialProject.id),
    ]);
    debugLog(`[FRONTEND][TRACE] ${traceNow()} initial data loaded in ${Math.round(performance.now() - mountStart)}ms`);

    listen('graph-updated', (event) => {
      const startedAt = performance.now();
      const activeProject = getActiveProject();
      if (!activeProject) return;
      const previousBranch = activeProject.currentBranch || '';
      const previousRevision = activeProject.currentRevision || '';
      const payload = event?.payload || {};
      const payloadBranch = payload.current_branch || '';
      const payloadRevision = payload.current_revision || '';
      const payloadSince = typeof payload.since === 'string' ? payload.since : '';
      const payloadSinceReset = Boolean(payload.since_reset);
      const payloadNodes = payload.graph?.nodes?.length || 0;
      const payloadEdges = payload.graph?.edges?.length || 0;
      const payloadBookmarks = Array.isArray(payload.bookmarks) ? payload.bookmarks.length : 0;
      debugLog(
        `[FRONTEND][BRANCH][event] graph-updated recv branch='${previousBranch}'->'${payloadBranch || previousBranch}' rev='${previousRevision || '-'}'->'${payloadRevision || previousRevision || '-'}' since='${activeProject.since}'->'${payloadSince || activeProject.since}' reset=${payloadSinceReset} nodes=${payloadNodes} edges=${payloadEdges} bookmarks=${payloadBookmarks}`
      );
      activeProject.graph = event.payload.graph;
      seedHeatmapFromGraph(event.payload.graph, activeProject.id);
      if (event.payload.current_branch) {
        activeProject.currentBranch = event.payload.current_branch;
      }
      if (event.payload.current_revision) {
        activeProject.currentRevision = event.payload.current_revision;
      }
      if (Array.isArray(event.payload.bookmarks)) {
        activeProject.bookmarks = event.payload.bookmarks;
      }
      const branchChanged = Boolean(payloadBranch) && payloadBranch !== previousBranch;
      const revisionChanged = Boolean(payloadRevision) && payloadRevision !== previousRevision;
      if ((payloadSinceReset || branchChanged) && payloadSince && activeProject.since !== payloadSince) {
        activeProject.since = payloadSince;
      }
      applyProjectToGlobals(activeProject);
      if (branchChanged || revisionChanged) {
        loadTimelineChanges(activeProject.id)
          .then(() => {
            debugLog(
              `[FRONTEND][BRANCH][event] timeline refreshed in ${Math.round(performance.now() - startedAt)}ms (branchChanged=${branchChanged} revisionChanged=${revisionChanged})`
            );
          })
          .catch((e) => console.error('[FRONTEND] Failed to refresh timeline changes:', e));
      } else {
        debugLog('[FRONTEND][BRANCH][event] timeline refresh skipped (no branch/revision change)');
      }
    }).catch((e) => {
      console.error('[FRONTEND] Failed to register graph-updated listener:', e);
    });

    listen('file-touched', (event) => {
      const activeProject = getActiveProject();
      if (!activeProject) return;
      const path = normalizeTouchedPath(event.payload);
      if (!path) return;
      debugLog(`[FRONTEND] File touched: ${path}`);

      activeProject.heatCounter = (activeProject.heatCounter || 0) + 1;
      activeProject.heatmapData.set(path, {
        seq: activeProject.heatCounter,
        touchedAt: Date.now(),
      });
      activeProject.fileContentVersions.set(
        path,
        Number(activeProject.fileContentVersions.get(path) || 0) + 1
      );
      if (activeProject.heatmapData.size > 100) {
        let oldestKey = null;
        let oldestSeq = Infinity;
        for (const [key, value] of activeProject.heatmapData.entries()) {
          const seq = typeof value === 'number' ? value : Number(value?.seq ?? 0);
          if (seq < oldestSeq) {
            oldestSeq = seq;
            oldestKey = key;
          }
        }
        if (oldestKey) activeProject.heatmapData.delete(oldestKey);
      }

      if (activeProjectId === activeProject.id) {
        heatCounter = activeProject.heatCounter;
        // Force reactivity in Svelte 5 for Map
        heatmapData = new Map(activeProject.heatmapData);
        fileContentVersions = new Map(activeProject.fileContentVersions);
      }
    }).catch((e) => {
      console.error('[FRONTEND] Failed to register file-touched listener:', e);
    });
    debugLog(`[FRONTEND][TRACE] ${traceNow()} listeners registered`);
  });

</script>

<svelte:window onkeydown={handleAppHotkeys} />

<div class="eyeloss-page">
  <div class="eyeloss-project-tabs">
    {#each projectTabs as project}
      <button
        type="button"
        class="eyeloss-project-tab"
        class:eyeloss-project-tab--active={project.id === activeProjectId}
        title={project.path}
        onclick={() => activateProjectTab(project.id)}
      >
        <span class="eyeloss-project-tab__name">{project.label}</span>
        {#if project.currentBranch}
          <span class="eyeloss-project-tab__branch">{project.currentBranch}</span>
        {/if}
        {#if projectTabs.length > 1}
          <span
            class="eyeloss-project-tab__close"
            role="button"
            tabindex="0"
            onclick={(e) => closeProjectTab(project.id, e)}
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') closeProjectTab(project.id, e);
            }}
          >
            x
          </span>
        {/if}
      </button>
    {/each}
    <button
      type="button"
      class="eyeloss-project-tab eyeloss-project-tab--new"
      onclick={selectRepo}
      title="Open another repo in a new tab"
      aria-label="Open new project tab"
    >
      +
    </button>
  </div>

  <div class="eyeloss-controls">
    <div class="eyeloss-controls__left">
      <div class="eyeloss-controls__row">
        <span class="eyeloss-controls__repo">{repoPath}</span>
        <button
          type="button"
          class="btn btn-ghost"
          onclick={changeCurrentRepo}
          style="font-size: 0.6rem; padding: 2px 6px;"
          title="Change repo in current tab"
        >
          Change
        </button>
      </div>
      <div class="eyeloss-controls__row">
        <span class="eyeloss-controls__branch">branch: {currentBranch}</span>
      </div>
      {#if currentBranch === 'NO_VCS'}
        <div class="eyeloss-controls__row eyeloss-controls__warning">
          no vcs detected: file-view mode only, init git/jj for history
        </div>
      {/if}
      {#if repoActionMessage}
        <div class="eyeloss-controls__row eyeloss-controls__error">{repoActionMessage}</div>
      {/if}
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

  <Sloposcope {graph} 
    {since} 
    {changes} 
    {bookmarks} 
    {contentVersion}
    {fileContentVersions}
    {getFileDiff} 
    {getFileSource} 
    {saveFile} 
    {theme} 
    {heatmapData}
    {loadingMore}
    {hasMoreChanges}
    onSelectSince={setSince} 
    onLoadMoreChanges={loadMoreChanges}
    onProjectChange={selectRepo}
  />

  {#if loading || refreshing || loadingMore}
    <div class={loading ? 'eyeloss-splash' : 'eyeloss-refresh-overlay'} transition:fade={{ duration: 120 }}>
      <div class="eyeloss-splash__fortune">{currentFortune}</div>
    </div>
  {/if}
</div>
