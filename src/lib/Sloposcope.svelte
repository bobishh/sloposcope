<script>
  import { onMount, untrack } from 'svelte';
  import { stepScene, createFrameScheduler } from './sceneRuntime';
  import Window from './Window.svelte';
  import Editor from './Editor.svelte';
  import CoordinatorWindow from './CoordinatorWindow.svelte';
  import ReviewerPresentationWindow from './ReviewerPresentationWindow.svelte';
  import NodeDetailsWindow from './NodeDetailsWindow.svelte';
  import Robot from './Robot.svelte';
  import { buildFileOrganismGeometry, paintFileOrganism } from './fileOrganism.ts';
  import { setupShortcuts } from './shortcuts.js';
  import {
    buildOverviewPhenotype,
    classifyPathRole,
    buildPathClusterTargets,
    buildPathRegions,
    getGraphDetailLevel,
    getPathClusterKey,
  } from './ecosystemModel.js';

  let { 
    graph = { nodes: [], edges: [] }, 
    graphMeta = null,
    reviewTarget = null,
    since = null, 
    changes = [], 
    bookmarks = [], 
    contentVersion = '',
    fileContentVersions = new Map(),
    comparisonSelection = { mode: 'none', since: null },
    getFileDiff, 
    getFileSource, 
    getFileAnatomy,
    getFileAnatomyComparison,
    jevByFile = {},
    invokeJev = null,
    getReviewerTargetContext,
    generateReviewerElm,
    compileReviewerElm,
    saveFile, 
    theme = 'midnight', 
    heatmapData = new Map(),
    onSelectSince,
    onReviewTargetChange = null,
    onLoadMoreChanges,
    onProjectChange,
    loadingMore = false,
    hasMoreChanges = true,
    missionState = null,
    missionRobotTargets = [],
    coordinatorState = null,
    coordinatorLoading = false,
    coordinatorSending = false,
    coordinatorError = '',
    onCoordinatorSend = null,
    onCoordinatorApprovePlan = null,
    onCoordinatorLaunchWorkers = null,
    onCoordinatorAskReport = null,
    onCoordinatorNudgeLatestWorker = null,
    onCoordinatorStartThread = null,
    onCoordinatorResumeThread = null,
  } = $props();

  function normalizeFilePath(path) {
    if (!path) return '';
    return String(path)
      .replaceAll('\\', '/')
      .replace(/^\.\/+/, '')
      .replace(/^\/+/, '');
  }

  function nodePathCandidates(node) {
    return [
      normalizeFilePath(node?.file),
      normalizeFilePath(node?.id),
    ].filter(Boolean);
  }

  function hasNodeAnatomyEndpoint(node) {
    return node?.anatomy != null || node?.anatomyEndpointState != null;
  }

  function nodeHasKnownCallables(node) {
    if (hasNodeAnatomyEndpoint(node)) {
      return Array.isArray(node?.anatomy?.callables)
        && node?.anatomy?.status !== 'unavailable'
        && !['absent', 'unavailable'].includes(String(node?.anatomyEndpointState || '').toLowerCase());
    }
    return Array.isArray(node?.functions);
  }

  function nodeCallableCount(node) {
    if (!nodeHasKnownCallables(node)) return 0;
    return hasNodeAnatomyEndpoint(node) ? node.anatomy.callables.length : node.functions.length;
  }

  function pathMatchesNode(path, node) {
    const normalized = normalizeFilePath(path);
    if (!normalized) return false;
    return nodePathCandidates(node).some((candidate) =>
      candidate === normalized
        || candidate.endsWith(`/${normalized}`)
        || normalized.endsWith(`/${candidate}`)
    );
  }

  function getFileContentVersion(file) {
    const normalized = normalizeFilePath(file);
    if (!normalized) return 0;
    return Number(fileContentVersions?.get(normalized) || 0);
  }

  function areSetsEqual(left, right) {
    if (left.size !== right.size) return false;
    for (const value of left) {
      if (!right.has(value)) return false;
    }
    return true;
  }

  function findNodeByFilePath(file) {
    const normalizedFile = normalizeFilePath(file);
    if (!normalizedFile) return null;

    return (
      simNodes.find((n) => normalizeFilePath(n.file) === normalizedFile) ||
      simNodes.find((n) => normalizeFilePath(n.id) === normalizedFile) ||
      simNodes.find((n) => normalizeFilePath(n.file).endsWith(`/${normalizedFile}`)) ||
      simNodes.find((n) => normalizeFilePath(n.id).endsWith(`/${normalizedFile}`)) ||
      null
    );
  }

  function clampRobotScreenTarget(desiredX, desiredY) {
    if (width <= 0 || height <= 0) {
      return { x: desiredX, y: desiredY };
    }

    const marginX = 56;
    const marginY = 56;
    return {
      x: Math.max(marginX, Math.min(width - marginX, desiredX)),
      y: Math.max(marginY, Math.min(height - marginY, desiredY)),
    };
  }

  function missionRobotState(session) {
    const runtimeState = String(session?.runtimeStatus || session?.liveliness || session?.state || 'idle').toLowerCase();
    const applyState = String(session?.applyState || '').toLowerCase();
    const gateState = String(session?.gateState || '').toLowerCase();
    const reviewState = String(session?.reviewState || '').toLowerCase();

    if (applyState === 'applied') return 'applied';
    if (runtimeState === 'failed' || runtimeState === 'error') return 'failed';
    if (runtimeState === 'stuck') return 'stuck';
    if (gateState === 'blocked') return 'stuck';
    if (gateState === 'ready' || reviewState === 'accepted') return 'gated';
    if (runtimeState === 'completed' || runtimeState === 'done') return 'completed';
    if (runtimeState === 'running' || runtimeState === 'claimed') return 'running';
    return 'idle';
  }

  function nodeWorkerSession(node) {
    const sessions = Array.isArray(missionState?.sessions) ? [...missionState.sessions].reverse() : [];
    const explicitSessionId = String(node?.worker_session_id || '').trim();
    if (explicitSessionId) {
      const explicit = sessions.find((session) => session?.sessionId === explicitSessionId);
      if (explicit) return explicit;
    }
    return sessions.find((session) => {
      const touchedPaths = Array.isArray(session?.touchedPaths) ? session.touchedPaths : [];
      const scopePaths = Array.isArray(session?.scopePaths) ? session.scopePaths : [];
      return [...touchedPaths, ...scopePaths].some((path) => pathMatchesNode(path, node));
    }) || null;
  }

  function nodeWorkerSessionId(node) {
    return String(nodeWorkerSession(node)?.sessionId || node?.worker_session_id || '').trim();
  }

  function nodeVerification(node) {
    const session = nodeWorkerSession(node);
    const verifications = Array.isArray(missionState?.verifications) ? [...missionState.verifications].reverse() : [];
    const bySession = session?.sessionId
      ? verifications.find((verification) => verification?.sessionId === session.sessionId)
      : null;
    if (bySession) return bySession;
    if (session?.objectiveId) {
      const byObjective = verifications.find((verification) => verification?.objectiveId === session.objectiveId);
      if (byObjective) return byObjective;
    }
    return verifications.find((verification) => pathMatchesNode(verification?.targetFile, node)) || null;
  }

  function nodeVerificationState(node) {
    const verification = nodeVerification(node);
    const session = nodeWorkerSession(node);
    const status = String(
      verification?.status
        || verification?.validationStatus
        || session?.validationStatus
        || ''
    ).toLowerCase();
    if (status === 'failed' || status === 'error') return 'failed';
    if (status === 'passed' || status === 'success') return 'passed';
    if (status === 'pending' || status === 'running') return status;
    return '';
  }

  function robotAnchorNodeForSession(session, index) {
    const touchedPaths = Array.isArray(session?.touchedPaths) ? session.touchedPaths : [];
    for (const path of touchedPaths) {
      const matched = findNodeByFilePath(path);
      if (matched) return matched;
    }

    const scopedPaths = Array.isArray(session?.scopePaths) ? session.scopePaths : [];
    for (const path of scopedPaths) {
      const matched = findNodeByFilePath(path);
      if (matched) return matched;
    }

    return (
      simNodes.find((n) => n.change_status === 'modified' || n.change_status === 'added') ||
      simNodes[index] ||
      simNodes[0] ||
      null
    );
  }

  let lastEditedNode = $derived.by(() => {
    if (!heatmapData || heatmapData.size === 0) return null;

    const touchedFiles = [];
    for (const [file, info] of heatmapData.entries()) {
      touchedFiles.push({
        file,
        touchedAt: getHeatTouchedAt(info) ?? 0,
      });
    }

    touchedFiles.sort((a, b) => b.touchedAt - a.touchedAt);
    for (const entry of touchedFiles) {
      const matched = findNodeByFilePath(entry.file);
      if (matched) return matched;
    }

    return null;
  });

  let robotTarget = $derived.by(() => {
    const _motion = sceneMotionVersion;
    const fallbackNode =
      simNodes.find((n) => n.change_status === 'modified' || n.change_status === 'added') ||
      simNodes[0] ||
      null;
    const targetNode = lastEditedNode || fallbackNode;
    if (!targetNode) return { x: 0, y: 0, active: false };
    const screen = worldToScreen(targetNode.x, targetNode.y);
    const size = getNodeSize(targetNode);
    // Keep robot on the edited document (upper-right area), not floating far away.
    const desiredX = screen.x + size.w * camera.zoom * 0.22;
    const desiredY = screen.y - size.h * camera.zoom * 0.28;
    return {
      ...clampRobotScreenTarget(desiredX, desiredY),
      active: true,
    };
  });

  let normalizedMissionRobotTargets = $derived.by(() => {
    const _motion = sceneMotionVersion;
    const explicitTargets = Array.isArray(missionRobotTargets) ? missionRobotTargets : [];
    const stateTargets = Array.isArray(missionState?.robotTargets) ? missionState.robotTargets : [];
    const sourceTargets = explicitTargets.length > 0 ? explicitTargets : stateTargets;
    if (sourceTargets.length > 0) {
      return sourceTargets
      .map((target, index) => ({
        x: Number(target?.x ?? 0),
        y: Number(target?.y ?? 0),
        active: Boolean(target?.active ?? true),
        state: String(target?.state || 'running'),
        instanceId: String(target?.instanceId || `mission-${index}`),
      }))
      .filter((target) => Number.isFinite(target.x) && Number.isFinite(target.y));
    }

    if (!missionState || String(missionState.status || 'idle') === 'idle') return [];
    const sessions = Array.isArray(missionState.sessions) ? missionState.sessions : [];
    const recentSessions = sessions.slice(-4).reverse();
    if (recentSessions.length === 0) return [];
    return recentSessions
      .map((session, index) => {
        const anchorNode = robotAnchorNodeForSession(session, index);
        if (!anchorNode) return null;
        const screen = worldToScreen(anchorNode.x, anchorNode.y);
        const size = getNodeSize(anchorNode);
        const dx = size.w * camera.zoom * (0.18 + index * 0.1);
        const dy = size.h * camera.zoom * (0.4 + (index % 2) * 0.22);
        const clamped = clampRobotScreenTarget(screen.x + dx, screen.y - dy);
        return {
          ...clamped,
          active: true,
          state: missionRobotState(session),
          instanceId: String(session?.sessionId || `mission-${anchorNode.id || index}`),
        };
      })
      .filter(Boolean);
  });

  let searchInput = $state(null);
  let highlightedSearchIndex = $state(-1);
  let coordinatorWindow = $state({
    x: 26,
    y: 24,
    width: 460,
    height: 580,
    visible: true,
  });
  let reviewerWindow = $state({
    x: 506,
    y: 24,
    width: 520,
    height: 580,
    visible: true,
  });

  function coordinatorCanLaunchWorkers() {
    return Boolean(
      coordinatorState?.canLaunchWorkers
        || (
          coordinatorState?.missionId
          && missionState?.missionId === coordinatorState?.missionId
          && Number(missionState?.scheduler?.readyCount || 0) > 0
        ),
    );
  }

  function coordinatorCanAskReport() {
    return Boolean(coordinatorState?.missionId && coordinatorState?.canAskReport);
  }

  function coordinatorDockStateLabel() {
    if (coordinatorState?.canApprovePlan) return 'Plan ready';
    if (coordinatorCanLaunchWorkers()) return 'Launch ready';
    if (coordinatorCanAskReport()) return 'Active';
    if (coordinatorLoading) return 'Loading';
    if (coordinatorError) return 'Needs attention';
    return 'Listening';
  }

  function reviewerDockStateLabel() {
    const target = reviewTarget || {};
    const mode = String(target.mode || 'working_copy');
    const includeWorkingCopy = target.includeWorkingCopy === undefined ? true : Boolean(target.includeWorkingCopy);
    if (mode === 'range') {
      const base = String(target.base || '(base)').trim() || '(base)';
      const head = String(target.head || '(head)').trim() || '(head)';
      return `${base}..${head}${includeWorkingCopy ? ' + working copy' : ''}`;
    }
    if (mode === 'since') {
      const sinceLabel = String(target.since || '@').trim() || '@';
      return `${sinceLabel}${includeWorkingCopy ? ' + working copy' : ''}`;
    }
    return includeWorkingCopy ? 'Working copy only' : 'Repository only';
  }

  onMount(() => {
    return setupShortcuts(
      () => onProjectChange?.(),
      () => {
        panelCollapsed = false;
        setTimeout(() => searchInput?.focus(), 10);
      },
      (dir) => {
        if (filteredNodes.length === 0) return;
        highlightedSearchIndex = (highlightedSearchIndex + dir + filteredNodes.length) % filteredNodes.length;
        focusNode(filteredNodes[highlightedSearchIndex]);
      }
    );
  });

  const TIMELINE_MIN_PAGE_SIZE = 8;
  const TIMELINE_TICK_WIDTH = 40;
  const TIMELINE_SIDE_RESERVE = 152;
  let timelineOffset = $state(0);
  let timelinePaging = $state(false);
  let timelineOldestExhausted = $state(false);
  let timelineDatasetToken = $state('');
  let timelineTrackEl = $state(null);
  let timelineHoverId = $state(null);
  const sessionStartedAt = Date.now();

  let timelineChanges = $derived.by(() => [...changes].reverse());
  let timelinePageSize = $derived.by(() => {
    const available = Math.max(0, width - TIMELINE_SIDE_RESERVE);
    const fit = Math.floor(available / TIMELINE_TICK_WIDTH);
    return Math.max(TIMELINE_MIN_PAGE_SIZE, fit);
  });

  let timelinePage = $derived.by(() => {
    const total = timelineChanges.length;
    const pageSize = timelinePageSize;
    if (total === 0) {
      return {
        total: 0,
        start: 0,
        end: 0,
        items: [],
        canMoveBack: false,
        canMoveForward: false,
        atOldestLoaded: true,
      };
    }

    const maxOffset = Math.max(0, total - 1);
    const safeOffset = Math.min(Math.max(timelineOffset, 0), maxOffset);
    const end = Math.max(1, total - safeOffset);
    const start = Math.max(0, end - pageSize);

    return {
      total,
      start,
      end,
      items: timelineChanges.slice(start, end),
      canMoveBack: start > 0 || (Boolean(onLoadMoreChanges) && hasMoreChanges && !timelineOldestExhausted),
      canMoveForward: end < total,
      atOldestLoaded: start === 0,
    };
  });

  let timelineStatus = $derived.by(() => {
    if (timelinePage.total === 0) return 'No commits';
    if (timelinePaging || loadingMore) return 'Loading older commits...';
    if (timelinePage.start === 0 && !timelinePage.canMoveBack) {
      return `Showing 1-${timelinePage.end} of ${timelinePage.total} (oldest loaded)`;
    }
    const start = timelinePage.start + 1;
    const end = timelinePage.end;
    return `Showing ${start}-${end} of ${timelinePage.total}`;
  });

  let timelineFocusChange = $derived.by(() => {
    const _hoverId = timelineHoverId;
    const _items = timelinePage.items;
    if (_hoverId) {
      const hovered = _items.find((change) => change.id === _hoverId);
      if (hovered) return hovered;
    }
    const active = _items.find((change, i) => isTickActive(change, timelinePage.start + i, timelinePage.total));
    if (active) return active;
    return _items[_items.length - 1] || null;
  });

  let timelineFocusDescription = $derived.by(() => {
    const _change = timelineFocusChange;
    if (!_change) return '';
    const desc = (_change.description || '').trim();
    if (desc.length > 0) return desc;
    return '(no description)';
  });

  function setTimelineOffset(nextOffset) {
    const maxOffset = Math.max(0, timelineChanges.length - 1);
    timelineOffset = Math.min(Math.max(nextOffset, 0), maxOffset);
  }

  async function loadOlderTimelinePage() {
    if (!onLoadMoreChanges || loadingMore || timelinePaging) return;
    timelinePaging = true;
    try {
      const loadedCount = (await onLoadMoreChanges()) || 0;
      if (loadedCount > 0) {
        timelineOldestExhausted = false;
        setTimelineOffset(timelineOffset + loadedCount);
      } else {
        timelineOldestExhausted = true;
      }
    } finally {
      timelinePaging = false;
    }
  }

  async function moveTimelineBack() {
    if (timelinePage.start > 0) {
      setTimelineOffset(timelineOffset + timelinePageSize);
      return;
    }
    await loadOlderTimelinePage();
  }

  function moveTimelineForward() {
    if (!timelinePage.canMoveForward) return;
    setTimelineOffset(timelineOffset - timelinePageSize);
  }

  $effect(() => {
    const _changes = changes;
    const newestId = _changes.length > 0 ? _changes[0].id : '';
    const oldestId = _changes.length > 0 ? _changes[_changes.length - 1].id : '';
    const nextToken = `${_changes.length}:${newestId}:${oldestId}`;
    if (timelineDatasetToken !== nextToken) {
      // If commit data was replaced (new filter/repo), allow trying to load older again.
      timelineDatasetToken = nextToken;
      timelineOldestExhausted = false;
    }
  });

  $effect(() => {
    const _hasMoreChanges = hasMoreChanges;
    if (_hasMoreChanges) {
      timelineOldestExhausted = false;
    }
  });

  $effect(() => {
    const _start = timelinePage.start;
    const _end = timelinePage.end;
    const _width = width;
    if (!timelineTrackEl) return;
    // Prevent stale horizontal offsets from making the strip look right-shifted.
    timelineTrackEl.scrollLeft = 0;
  });

  $effect(() => {
    const _items = timelinePage.items;
    const _hoverId = timelineHoverId;
    if (!_hoverId) return;
    if (!_items.some((change) => change.id === _hoverId)) {
      timelineHoverId = null;
    }
  });

  function getHeatSeq(raw) {
    if (typeof raw === 'number') return raw;
    if (raw && typeof raw === 'object') {
      const seq = Number(raw.seq ?? raw.value ?? 0);
      return Number.isFinite(seq) ? seq : 0;
    }
    return 0;
  }

  function getHeatTouchedAt(raw) {
    if (raw && typeof raw === 'object') {
      const ts = Number(raw.touchedAt ?? raw.ts ?? 0);
      if (Number.isFinite(ts) && ts > 0) return ts;
    }
    return null;
  }

  function buildHeatRuntimeIndex(nowMs = Date.now()) {
    const index = new Map();
    if (!heatmapData || heatmapData.size === 0) {
      return { index, avgAgeMs: 0, sessionMs: Math.max(1, nowMs - sessionStartedAt) };
    }

    const entries = [];
    let minSeq = Infinity;
    let maxSeq = -Infinity;
    for (const [file, raw] of heatmapData.entries()) {
      const seq = getHeatSeq(raw);
      const touchedAt = getHeatTouchedAt(raw);
      entries.push({ file, seq, touchedAt });
      minSeq = Math.min(minSeq, seq);
      maxSeq = Math.max(maxSeq, seq);
    }

    const sessionMs = Math.max(1, nowMs - sessionStartedAt);
    const seqRange = Math.max(1, maxSeq - minSeq);
    const agesMs = entries.map((entry) => {
      if (entry.touchedAt) return Math.max(0, nowMs - entry.touchedAt);
      const synthetic = ((maxSeq - entry.seq) / seqRange) * sessionMs * 0.8 + sessionMs * 0.1;
      return Math.max(0, synthetic);
    });
    const avgAgeRaw = agesMs.reduce((sum, age) => sum + age, 0) / Math.max(1, agesMs.length);
    const avgAgeMs = Math.max(6000, Math.min(Math.max(12000, sessionMs), avgAgeRaw || 12000));

    for (let i = 0; i < entries.length; i++) {
      const entry = entries[i];
      const ageMs = agesMs[i];
      const relativeAge = ageMs / Math.max(1000, avgAgeMs);
      const tier =
        relativeAge < 0.55 ? 0 :
        relativeAge < 1.10 ? 1 :
        relativeAge < 1.80 ? 2 : 3;
      const brightnessByTier = [1.0, 0.74, 0.48, 0.28];
      const frequencyByTier = [2.4, 1.6, 0.95, 0.5];
      const recency = seqRange > 0 ? (entry.seq - minSeq) / seqRange : 1;
      const intensity = Math.max(0.12, (0.25 + 0.75 * recency) * brightnessByTier[tier]);
      const oldness = Math.max(0, Math.min(1, relativeAge / 2.2));
      index.set(entry.file, {
        tier,
        intensity,
        pulseFrequency: frequencyByTier[tier],
        pulseAmplitude: 0.25 + 0.55 * brightnessByTier[tier],
        oldness,
        ageMs,
      });
    }

    return { index, avgAgeMs, sessionMs };
  }

  function getNodeHeat(nodeId) {
    const runtime = buildHeatRuntimeIndex(Date.now());
    return runtime.index.get(nodeId)?.intensity || 0;
  }

  let canvas;
  let width = $state(0);
  let height = $state(0);
  let pendingAutoCenter = $state(false);

  let camera = $state({ x: 0, y: 0, zoom: 1 });

  let simNodes = $state.raw([]);
  let simEdges = $state.raw([]);
  let sceneMotionVersion = $state(0);
  let animating = $state(true);
  let selectedNodeIds = $state(new Set());
  let activeNodeId = $state(null);
  let nodeWindows = $state([]);
  let hoveredNode = $state.raw(null);
  let graphNodesById = $derived.by(() => new Map((graph.nodes || []).map((node) => [node.id, node])));
  let sourceWindows = $state([]);
  let panelCollapsed = $state(false);
  let searchQuery = $state('');
  let defaultNodeWindowViewMode = $state('bug');
  let organismGraphStats = $derived.by(() => ({
    files: simNodes.length,
    pathRegions: buildPathRegions(simNodes, 1).length,
    staticLinks: simEdges.length,
  }));

  let dragging = $state(false);
  let dragStart = $state({ x: 0, y: 0 });
  let dragCameraStart = $state({ x: 0, y: 0 });

  let draggedNode = $state.raw(null);
  let dragOffset = $state({ x: 0, y: 0 });

  let frameScheduler = null;
  let scenePositionRevision = 0;
  const sceneMetrics = { treeVisits: 0, collisionTests: 0, physics: 0, render: 0, preparation: 0, frames: 0 };
  const regionCache = new Map();
  function sceneRegions(depth) {
    const now = performance.now();
    const previous = regionCache.get(depth);
    if (previous && (previous.revision === scenePositionRevision || (animating && now - previous.at < 100))) return previous.regions;
    const regions = buildPathRegions(simNodes, depth);
    regionCache.set(depth, { at: now, revision: scenePositionRevision, regions });
    return regions;
  }
  let simulationIterations = 0;
  const MIN_ZOOM = 0.0001;
  const MAX_ZOOM = 10;

  let canvasColors = $state({
    primary: '#4a8c5c',
    secondary: '#c8a620',
    text: '#ffffff',
    bg100: '#16213e'
  });

  function updateCanvasColors() {
    const style = getComputedStyle(document.documentElement);
    canvasColors = {
      primary: style.getPropertyValue('--primary').trim() || '#4a8c5c',
      secondary: style.getPropertyValue('--secondary').trim() || '#c8a620',
      text: style.getPropertyValue('--text').trim() || '#ffffff',
      bg100: style.getPropertyValue('--bg-100').trim() || '#16213e'
    };
  }

  $effect(() => {
    // Re-run whenever theme changes
    const _t = theme;
    updateCanvasColors();
  });

  const NAMESPACE_COLORS = $derived({
    lib: canvasColors.primary,
    src: canvasColors.primary,
    assets: canvasColors.secondary,
    config: '#b87333',
    test: '#8c4a4a',
    Svelte: '#e34c26',
    JS: canvasColors.secondary,
    Root: '#555',
  });

  const EDGE_COLORS = $derived({
    call: 'rgba(120, 120, 120, 0.4)',
    use: 'rgba(70, 100, 180, 0.5)',
    import: 'rgba(70, 150, 70, 0.5)',
    alias: 'rgba(200, 140, 50, 0.5)',
  });

  const DEFAULT_NODE_COLOR = '#666666';
  const DEFAULT_EDGE_COLOR = 'rgba(120, 120, 120, 0.3)';
  const FIGURINE_VARIANTS = [
    { kind: 'sheet', tag: 'right' },
    { kind: 'folded', tag: 'right' },
    { kind: 'stack', tag: 'left' },
    { kind: 'tabbed', tag: 'left' },
    { kind: 'receipt', tag: 'center' },
  ];

  function hashString(value) {
    let hash = 2166136261;
    const input = String(value || '');
    for (let i = 0; i < input.length; i++) {
      hash ^= input.charCodeAt(i);
      hash += (hash << 1) + (hash << 4) + (hash << 7) + (hash << 8) + (hash << 24);
    }
    return Math.abs(hash >>> 0);
  }

  function getNodeExtension(node) {
    const fileish = node?.file || node?.id || '';
    const base = fileish.split('/').pop() || fileish;
    const idx = base.lastIndexOf('.');
    if (idx <= 0 || idx === base.length - 1) return 'misc';
    return base.slice(idx + 1).toLowerCase();
  }

  function getNamespace(id) {
    if (!id) return 'Root';
    
    if (id.includes('/')) {
      const parts = id.split('/');
      return parts[0]; // e.g. "lib", "assets", "config"
    }
    
    const parts = id.split('.');
    if (parts.length >= 2) return parts[0];
    return 'Root';
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

  const NAV_PANEL_WIDTH_PX = 260;
  const NAV_ITEM_HORIZONTAL_PADDING_PX = 24;
  const NAV_DOT_AND_GAP_PX = 16;
  const NAV_ITEM_SAFE_GUTTER_PX = 12;
  const NAV_MONO_CHAR_WIDTH_PX = 9;
  const NAV_LABEL_MAX_CHARS = Math.max(
    12,
    Math.floor(
      (NAV_PANEL_WIDTH_PX - NAV_ITEM_HORIZONTAL_PADDING_PX - NAV_DOT_AND_GAP_PX - NAV_ITEM_SAFE_GUTTER_PX) /
        NAV_MONO_CHAR_WIDTH_PX
    )
  );

  function middleEllipsis(text, startChars = 9, endChars = 9, maxChars = NAV_LABEL_MAX_CHARS) {
    if (!text) return '';
    const value = String(text);
    if (value.length <= maxChars) return value;
    const minVisible = startChars + endChars + 1;
    if (maxChars <= minVisible) return `${value.slice(0, startChars)}…${value.slice(-endChars)}`;
    return `${value.slice(0, startChars)}…${value.slice(-endChars)}`;
  }

  function getNavNodeLabel(node) {
    return middleEllipsis(getShortName(node?.id || ''), 9, 9);
  }

  function getNodeColor(moduleId) {
    const parts = moduleId.split('.');
    if (parts[0] === 'Svelte') return NAMESPACE_COLORS['Svelte'];
    if (parts[0] === 'JS') return NAMESPACE_COLORS['JS'];
    const ns = parts.length >= 2 ? parts[1] : parts[0];
    return NAMESPACE_COLORS[ns] || DEFAULT_NODE_COLOR;
  }

  function getClusterKey(moduleId) {
    const parts = moduleId.split('.');
    if (parts[0] === 'Svelte') return 'Svelte';
    if (parts[0] === 'JS') return 'JS';
    if (parts.length >= 2) return parts[0] + '.' + parts[1];
    return parts[0];
  }

  function getNodeClusterKey(node) {
    return getPathClusterKey(node);
  }

  function parseHexColor(color, fallback = { r: 128, g: 128, b: 128 }) {
    if (!color || typeof color !== 'string') return fallback;
    const value = color.trim();
    const short = value.match(/^#([a-fA-F0-9]{3})$/);
    if (short) {
      const [, v] = short;
      return {
        r: parseInt(v[0] + v[0], 16),
        g: parseInt(v[1] + v[1], 16),
        b: parseInt(v[2] + v[2], 16),
      };
    }
    const full = value.match(/^#([a-fA-F0-9]{6})$/);
    if (full) {
      const [, v] = full;
      return {
        r: parseInt(v.slice(0, 2), 16),
        g: parseInt(v.slice(2, 4), 16),
        b: parseInt(v.slice(4, 6), 16),
      };
    }
    const rgb = value.match(/^rgba?\(\s*(\d+)\s*[, ]\s*(\d+)\s*[, ]\s*(\d+)/i);
    if (rgb) {
      return {
        r: Math.max(0, Math.min(255, Number(rgb[1]))),
        g: Math.max(0, Math.min(255, Number(rgb[2]))),
        b: Math.max(0, Math.min(255, Number(rgb[3]))),
      };
    }
    return fallback;
  }

  function mixRgb(a, b, t) {
    const clamped = Math.max(0, Math.min(1, t));
    return {
      r: Math.round(a.r + (b.r - a.r) * clamped),
      g: Math.round(a.g + (b.g - a.g) * clamped),
      b: Math.round(a.b + (b.b - a.b) * clamped),
    };
  }

  function rgbToCss(rgb, alpha = 1) {
    return `rgba(${rgb.r}, ${rgb.g}, ${rgb.b}, ${alpha})`;
  }

  function getNodePalette(node) {
    const ext = node.file_ext || getNodeExtension(node);
    const seed = hashString(ext);
    const primary = parseHexColor(canvasColors.primary, { r: 74, g: 140, b: 92 });
    const secondary = parseHexColor(canvasColors.secondary, { r: 200, g: 166, b: 32 });
    const text = parseHexColor(canvasColors.text, { r: 224, g: 224, b: 224 });
    const bg = parseHexColor(canvasColors.bg100, { r: 22, g: 33, b: 62 });

    const warm = mixRgb(primary, secondary, 0.32);
    const cool = mixRgb(primary, bg, 0.30);
    const copper = mixRgb(secondary, bg, 0.34);
    const mist = mixRgb(text, bg, 0.34);

    const swatches = [
      mixRgb(primary, bg, 0.14),
      mixRgb(primary, bg, 0.28),
      mixRgb(primary, text, 0.16),
      mixRgb(secondary, bg, 0.14),
      mixRgb(secondary, bg, 0.27),
      mixRgb(secondary, text, 0.20),
      mixRgb(warm, bg, 0.20),
      mixRgb(warm, text, 0.24),
      mixRgb(cool, text, 0.20),
      mixRgb(copper, text, 0.24),
      mixRgb(mist, primary, 0.22),
      mixRgb(mist, secondary, 0.22),
    ];

    const base = swatches[seed % swatches.length];
    const edge = mixRgb(base, bg, 0.56);
    const accent = mixRgb(base, text, 0.50);
    const detail = mixRgb(primary, secondary, ((seed >> 3) % 100) / 100);
    return {
      fill: rgbToCss(base),
      edge: rgbToCss(edge),
      accent: rgbToCss(accent),
      detail: rgbToCss(detail),
      ext,
    };
  }

  function getFigurineVariant(node) {
    if (Number.isInteger(node?.figure_variant)) {
      return FIGURINE_VARIANTS[node.figure_variant % FIGURINE_VARIANTS.length];
    }
    const seed = hashString(`${node.id}|${node.file || ''}|${node.file_ext || ''}`);
    return FIGURINE_VARIANTS[seed % FIGURINE_VARIANTS.length];
  }

  function buildFigurineGeometry(x0, y0, w, h) {
    const path = new Path2D();
    const docX = x0 + w * 0.10;
    const docY = y0 + h * 0.08;
    const docW = w * 0.80;
    const docH = h * 0.84;
    const fold = Math.max(7, Math.min(docW, docH) * 0.20);

    // Single document silhouette with folded corner.
    path.moveTo(docX, docY);
    path.lineTo(docX + docW - fold, docY);
    path.lineTo(docX + docW, docY + fold);
    path.lineTo(docX + docW, docY + docH);
    path.lineTo(docX, docY + docH);
    path.closePath();

    return {
      path,
      docX,
      docY,
      docW,
      docH,
      tagX: docX + docW * 0.10,
      tagY: docY + docH * 0.14,
      tagAlign: 'left',
      foldCorner: { x: docX + docW - fold, y: docY, w: fold, h: fold },
    };
  }

  function buildNodeOrganism(node) {
    const phenotype = buildOverviewPhenotype(node, 0, 1);
    const anatomy = {
      ...node.anatomy,
      file: node.file || node.id,
      language: node.anatomy?.language || phenotype.language,
      status: phenotype.anatomyStatus,
      callables: phenotype.callablesKnown ? phenotype.callables : null,
    };
    // A selected historical endpoint must supply its own size.
    const lineCount = node.anatomy?.lineCount
      ?? (node.anatomyEndpointState == null ? node.line_count ?? null : null);
    return buildFileOrganismGeometry({ anatomy, lineCount, roleFact: classifyPathRole(node).role,
      assessment: jevByFile[normalizeFilePath(node.file)] });
  }

  function getNodeGeometry(node) {
    return node.organismGeometry || buildNodeOrganism(node);
  }

  function getNodeSize(node) {
    const { bounds } = getNodeGeometry(node);
    return { w: bounds.width, h: bounds.height };
  }

  function getNodeMass(node) {
    const lc = node.line_count || 50;
    const raw = Math.pow(Math.max(1, lc) / 40, 0.35);
    return Math.min(3.2, Math.max(0.9, raw));
  }

  function centerCameraOnNodes(nodes, padding = 120) {
    if (!nodes || nodes.length === 0) return;

    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;

    for (const node of nodes) {
      const { bounds } = getNodeGeometry(node);
      minX = Math.min(minX, node.x + bounds.minX);
      maxX = Math.max(maxX, node.x + bounds.maxX);
      minY = Math.min(minY, node.y + bounds.minY);
      maxY = Math.max(maxY, node.y + bounds.maxY);
    }

    camera.x = (minX + maxX) / 2;
    camera.y = (minY + maxY) / 2;

    if (width <= 0 || height <= 0) return;

    const graphW = Math.max(1, maxX - minX);
    const graphH = Math.max(1, maxY - minY);
    const innerW = Math.max(80, width - padding * 2);
    const innerH = Math.max(80, height - padding * 2);
    const fitZoom = Math.min(innerW / graphW, innerH / graphH);

    // Keep it comfortably visible, not too tiny and not over-zoomed.
    camera.zoom = Math.max(MIN_ZOOM, Math.min(1.4, fitZoom));
  }

  function findNodeById(nodeId) {
    return graphNodesById.get(nodeId) || null;
  }

  function ensureNodeWindow(node) {
    const existing = nodeWindows.find((w) => w.nodeId === node.id);
    if (existing) {
      nodeWindows = [
        ...nodeWindows.filter((w) => w.nodeId !== node.id),
        existing,
      ];
      return;
    }

    const slot = (nodeWindows.length + sourceWindows.length) % 6;
    const detailWidth = Math.min(900, Math.max(420, width * 0.62));
    const detailHeight = Math.min(800, Math.max(320, height * 0.84));
    const nextWindow = {
      id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      nodeId: node.id,
      initialViewMode: defaultNodeWindowViewMode,
      x: Math.max(16, 24 + slot * 28),
      y: Math.max(16, 20 + slot * 22),
      width: detailWidth,
      height: detailHeight,
    };
    nodeWindows = [...nodeWindows, nextWindow];
  }

  function focusNode(node) {
    if (!node) return;
    const { bounds } = getNodeGeometry(node);
    camera.x = node.x + (bounds.minX + bounds.maxX) / 2;
    camera.y = node.y + (bounds.minY + bounds.maxY) / 2;
    camera.zoom = Math.max(camera.zoom, 1.5);

    const next = new Set(selectedNodeIds);
    next.add(node.id);
    selectedNodeIds = next;
    activeNodeId = node.id;
    ensureNodeWindow(node);
  }

  function toggleDefaultNodeWindowViewMode() {
    const modes = ['bug', 'diff', 'source'];
    const current = modes.indexOf(defaultNodeWindowViewMode);
    defaultNodeWindowViewMode = modes[(current + 1) % modes.length];
  }

  function closeNodeWindow(nodeId) {
    const remaining = nodeWindows.filter((w) => w.nodeId !== nodeId);
    nodeWindows = remaining;

    const next = new Set(selectedNodeIds);
    next.delete(nodeId);
    selectedNodeIds = next;

    if (activeNodeId === nodeId) {
      activeNodeId = remaining.length > 0 ? remaining[remaining.length - 1].nodeId : null;
    }
  }

  function moveNodeWindow(nodeId, detail) {
    nodeWindows = nodeWindows.map((w) =>
      w.nodeId === nodeId ? { ...w, x: detail.x, y: detail.y } : w
    );
  }

  function resizeNodeWindow(nodeId, detail) {
    nodeWindows = nodeWindows.map((w) =>
      w.nodeId === nodeId ? { ...w, width: detail.width, height: detail.height } : w
    );
  }

  function focusNodeWindow(nodeId) {
    const node = findNodeById(nodeId);
    if (node) {
      const next = new Set(selectedNodeIds);
      next.add(nodeId);
      selectedNodeIds = next;
    }
    activeNodeId = nodeId;
    const existing = nodeWindows.find((w) => w.nodeId === nodeId);
    if (!existing) return;
    nodeWindows = [
      ...nodeWindows.filter((w) => w.nodeId !== nodeId),
      existing,
    ];
  }

  let sourceReferencesBySourceId = $derived.by(() => {
    const index = new Map();
    const seenBySource = new Map();
    const edges = graph.edges || [];
    const nodesById = graphNodesById;

    for (const edge of edges) {
      const sourceNode = nodesById.get(edge.source);
      const targetNode = nodesById.get(edge.target);
      if (!sourceNode || !targetNode || !targetNode.file || targetNode.file === sourceNode.file) continue;

      let refs = index.get(sourceNode.id);
      if (!refs) {
        refs = [];
        index.set(sourceNode.id, refs);
      }

      let seen = seenBySource.get(sourceNode.id);
      if (!seen) {
        seen = new Set();
        seenBySource.set(sourceNode.id, seen);
      }

      const candidates = [
        edge.target,
        getShortName(edge.target),
        getShortName(targetNode.file),
      ];

      for (const candidate of candidates) {
        if (!candidate || seen.has(candidate)) continue;
        seen.add(candidate);
        refs.push({
          token: candidate,
          nodeId: targetNode.id,
          file: targetNode.file,
          title: getShortName(targetNode.id),
        });
      }
    }

    return index;
  });

  function getSourceReferenceLinksForNode(nodeId) {
    return sourceReferencesBySourceId.get(nodeId) || [];
  }

  async function openSourceWindow(file, title) {
    if (!file) return;
    const normalized = normalizeFilePath(file);
    if (!normalized) return;

    const existing = sourceWindows.find((w) => normalizeFilePath(w.file) === normalized);
    if (existing) {
      sourceWindows = [
        ...sourceWindows.filter((w) => w.id !== existing.id),
        existing,
      ];
      return;
    }

    const slot = sourceWindows.length % 4;
    const fileVersion = getFileContentVersion(normalized);
    const requestId = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}|${contentVersion}|${fileVersion}`;
    const sourceWindow = {
      id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      file: normalized,
      title: title || getShortName(normalized),
      content: '',
      loading: true,
      requestId,
      versionToken: `${contentVersion}|${fileVersion}`,
      x: Math.max(20, 28 + slot * 34),
      y: Math.max(20, 28 + slot * 26),
      width: Math.min(780, Math.max(420, width * 0.62)),
      height: Math.min(680, Math.max(320, height * 0.74)),
    };

    sourceWindows = [...sourceWindows, sourceWindow];

    try {
      const source = await getFileSource(normalized);
      sourceWindows = sourceWindows.map((w) =>
        w.id === sourceWindow.id && w.requestId === requestId
          ? { ...w, content: source || '', loading: false }
          : w
      );
    } catch (e) {
      sourceWindows = sourceWindows.map((w) =>
        w.id === sourceWindow.id && w.requestId === requestId
          ? { ...w, content: `Failed to load source for ${normalized}`, loading: false }
          : w
      );
    }
  }

  async function openSourceWindowForReference(ref) {
    if (!ref?.file) return;
    await openSourceWindow(ref.file, ref.title || getShortName(ref.file));
  }

  function closeSourceWindow(windowId) {
    sourceWindows = sourceWindows.filter((w) => w.id !== windowId);
  }

  function moveSourceWindow(windowId, detail) {
    sourceWindows = sourceWindows.map((w) =>
      w.id === windowId ? { ...w, x: detail.x, y: detail.y } : w
    );
  }

  function resizeSourceWindow(windowId, detail) {
    sourceWindows = sourceWindows.map((w) =>
      w.id === windowId ? { ...w, width: detail.width, height: detail.height } : w
    );
  }

  function focusSourceWindow(windowId) {
    const existing = sourceWindows.find((w) => w.id === windowId);
    if (!existing) return;
    sourceWindows = [
      ...sourceWindows.filter((w) => w.id !== windowId),
      existing,
    ];
  }

  $effect(() => {
    const version = contentVersion;
    const openWindows = sourceWindows;
    if (!version || openWindows.length === 0 || !getFileSource) return;

    for (const sourceWindow of openWindows) {
      const fileVersion = getFileContentVersion(sourceWindow.file);
      const versionToken = `${version}|${fileVersion}`;
      if (sourceWindow.versionToken === versionToken) continue;
      const requestId = `${sourceWindow.id}|${versionToken}`;

      sourceWindows = sourceWindows.map((w) =>
        w.id === sourceWindow.id ? { ...w, loading: true, requestId, versionToken } : w
      );

      getFileSource(sourceWindow.file)
        .then((source) => {
          sourceWindows = sourceWindows.map((w) =>
            w.id === sourceWindow.id && w.requestId === requestId
              ? { ...w, content: source || '', loading: false }
              : w
          );
        })
        .catch(() => {
          sourceWindows = sourceWindows.map((w) =>
            w.id === sourceWindow.id && w.requestId === requestId
              ? { ...w, content: `Failed to load source for ${sourceWindow.file}`, loading: false }
              : w
          );
        });
    }
  });

  function tileFloatingWindows() {
    const total = nodeWindows.length + sourceWindows.length;
    if (total === 0 || width <= 0 || height <= 0) return;

    const margin = 16;
    const gap = 10;
    const timelineReserve = 120;
    const usableWidth = Math.max(220, width - margin * 2);
    const usableHeight = Math.max(220, height - margin * 2 - timelineReserve);
    const cols = Math.max(1, Math.ceil(Math.sqrt(total)));
    const rows = Math.max(1, Math.ceil(total / cols));
    const cellWidth = Math.max(260, Math.floor((usableWidth - gap * (cols - 1)) / cols));
    const cellHeight = Math.max(220, Math.floor((usableHeight - gap * (rows - 1)) / rows));

    let cursor = 0;
    nodeWindows = nodeWindows.map((w) => {
      const col = cursor % cols;
      const row = Math.floor(cursor / cols);
      cursor += 1;
      return {
        ...w,
        x: margin + col * (cellWidth + gap),
        y: margin + row * (cellHeight + gap),
        width: cellWidth,
        height: cellHeight,
      };
    });

    sourceWindows = sourceWindows.map((w) => {
      const col = cursor % cols;
      const row = Math.floor(cursor / cols);
      cursor += 1;
      return {
        ...w,
        x: margin + col * (cellWidth + gap),
        y: margin + row * (cellHeight + gap),
        width: cellWidth,
        height: cellHeight,
      };
    });
  }


  let filteredNodes = $derived.by(() => {
    const q = searchQuery.toLowerCase();
    const nodes = simNodes.slice().sort((a, b) => a.id.localeCompare(b.id));
    if (!q) return nodes;
    return nodes.filter(n => n.id.toLowerCase().includes(q));
  });

  let nodeProvenanceById = $derived.by(() => {
    const entries = graphMeta?.nodeProvenance || [];
    return new Map(entries.map((entry) => [entry.nodeId, entry.reason]));
  });

  function buildEdgeKey(edge) {
    return `${edge.source}::${edge.target}::${edge.type || ''}`;
  }

  function countSetIntersection(left, right) {
    let count = 0;
    const [small, big] = left.size <= right.size ? [left, right] : [right, left];
    for (const value of small) {
      if (big.has(value)) count++;
    }
    return count;
  }

  function initSimulation() {
    const setupStart = performance.now();
    scenePositionRevision++;
    regionCache.clear();
    if (!graph.nodes || graph.nodes.length === 0) {
      simNodes = [];
      simEdges = [];
      return;
    }

    const degreeById = Object.fromEntries(graph.nodes.map((node) => [node.id, 0]));
    for (const edge of graph.edges || []) {
      if (degreeById[edge.source] !== undefined) degreeById[edge.source] += 1;
      if (degreeById[edge.target] !== undefined) degreeById[edge.target] += 1;
    }

    const clusters = {};
    for (const node of graph.nodes) {
      const key = getNodeClusterKey(node);
      if (!clusters[key]) clusters[key] = [];
      clusters[key].push(node);
    }

    const nodeJitter = Math.max(70, Math.min(160, 90 + Math.sqrt(graph.nodes.length) * 5));
    const clusterTargets = new Map(buildPathClusterTargets(graph.nodes).map((target) => [target.id, target]));

    const prevNodes = untrack(() => simNodes);
    const prevEdges = untrack(() => simEdges);
    const prevNodesById = new Map(prevNodes.map((node) => [node.id, node]));
    const prevIdSet = new Set(prevNodesById.keys());
    const nextIdSet = new Set(graph.nodes.map((node) => node.id));
    const overlapCount = countSetIntersection(prevIdSet, nextIdSet);
    const overlapRatio = overlapCount / Math.max(1, nextIdSet.size);
    const sameNodeSet = areSetsEqual(prevIdSet, nextIdSet);
    const prevEdgeSet = new Set(prevEdges.map(buildEdgeKey));
    const nextEdgeSet = new Set((graph.edges || []).map(buildEdgeKey));
    const sameEdgeSet = areSetsEqual(prevEdgeSet, nextEdgeSet);
    const structureChanged = !sameNodeSet || !sameEdgeSet;

    simNodes = graph.nodes.map((node) => {
      const clusterKey = getNodeClusterKey(node);
      const center = clusterTargets.get(clusterKey) || { x: 0, y: 0, boundaryRadius: 115 };
      const boundaryRadius = Math.max(center.boundaryRadius, 78 + Math.sqrt(clusters[clusterKey].length) * 58);
      const prev = prevNodesById.get(node.id);
      let x = center.x + (Math.random() - 0.5) * nodeJitter;
      let y = center.y + (Math.random() - 0.5) * nodeJitter;
      let vx = 0;
      let vy = 0;
      let pinned = false;

      if (prev) {
        const clusterSwitched = prev.cluster_key && prev.cluster_key !== clusterKey;
        if (clusterSwitched) {
          // Keep continuity but gently steer to new cluster if grouping changed.
          x = prev.x * 0.84 + center.x * 0.16;
          y = prev.y * 0.84 + center.y * 0.16;
        } else {
          x = prev.x;
          y = prev.y;
        }
        vx = prev.vx || 0;
        vy = prev.vy || 0;
        pinned = Boolean(prev.pinned);
      }

      return {
        ...node,
        organismGeometry: buildNodeOrganism(node),
        cluster_key: clusterKey,
        cluster_target_x: center.x,
        cluster_target_y: center.y,
        cluster_boundary_radius: boundaryRadius,
        static_degree: degreeById[node.id] || 0,
        file_ext: getNodeExtension(node),
        figure_variant: hashString(`${node.id}|${node.file || ''}`) % FIGURINE_VARIANTS.length,
        mass: getNodeMass(node),
        x,
        y,
        vx,
        vy,
        pinned,
      };
    });

    simEdges = (graph.edges || []).map(e => ({ ...e }));
    if (structureChanged) {
      simulationIterations = 0;
      animating = true;
    }

    const firstInit = prevNodes.length === 0;
    const majorReset = prevNodes.length > 0 && overlapRatio < 0.35;
    pendingAutoCenter = firstInit || majorReset;
    sceneMetrics.preparation = performance.now() - setupStart;
  }

  function stepSimulation() {
    const result = stepScene(simNodes, simEdges, simulationIterations++);
    animating = result.active;
    sceneMetrics.treeVisits = result.treeVisits;
    sceneMetrics.collisionTests = result.collisionTests;
    scenePositionRevision++;
    if (sceneMetrics.frames % 6 === 0) sceneMotionVersion++;
  }

  function screenToWorld(sx, sy) {
    return {
      x: (sx - width / 2) / camera.zoom + camera.x,
      y: (sy - height / 2) / camera.zoom + camera.y,
    };
  }

  function worldToScreen(wx, wy) {
    return {
      x: (wx - camera.x) * camera.zoom + width / 2,
      y: (wy - camera.y) * camera.zoom + height / 2,
    };
  }

  function isNodeVisible(node) {
    const { bounds } = getNodeGeometry(node);
    const topLeft = worldToScreen(node.x + bounds.minX, node.y + bounds.minY);
    const bottomRight = worldToScreen(node.x + bounds.maxX, node.y + bounds.maxY);
    return bottomRight.x > 0 && topLeft.x < width && bottomRight.y > 0 && topLeft.y < height;
  }

  function findNodeAt(sx, sy) {
    const world = screenToWorld(sx, sy);
    for (let i = simNodes.length - 1; i >= 0; i--) {
      const node = simNodes[i];
      const { bounds } = getNodeGeometry(node);
      const x = world.x - node.x;
      const y = world.y - node.y;
      if (x >= bounds.minX && x <= bounds.maxX && y >= bounds.minY && y <= bounds.maxY) return node;
    }
    return null;
  }

  function getConnectedNodeIds(nodeId) {
    const ids = new Set();
    for (const edge of simEdges) {
      if (edge.source === nodeId) ids.add(edge.target);
      if (edge.target === nodeId) ids.add(edge.source);
    }
    return ids;
  }

  function drawPathRegionLayer(ctx, regions, alpha, showLabels, alreadyDrawn = new Set()) {
    for (const region of regions) {
      if (alreadyDrawn.has(region.id) || region.polygon.length < 3) continue;
      alreadyDrawn.add(region.id);
      const screenPoints = region.polygon.map((point) => worldToScreen(point.x, point.y));
      const path = new Path2D();
      path.moveTo(screenPoints[0].x, screenPoints[0].y);
      for (let index = 1; index < screenPoints.length; index += 1) {
        path.lineTo(screenPoints[index].x, screenPoints[index].y);
      }
      path.closePath();

      const seed = hashString(region.id);
      const primary = parseHexColor(canvasColors.primary);
      const secondary = parseHexColor(canvasColors.secondary);
      const bg = parseHexColor(canvasColors.bg100);
      const base = mixRgb(seed % 2 ? primary : secondary, bg, 0.58 + ((seed >> 3) % 12) / 100);
      ctx.fillStyle = rgbToCss(base, alpha);
      ctx.strokeStyle = rgbToCss(mixRgb(base, parseHexColor(canvasColors.text), 0.18), Math.min(0.7, alpha * 3.4));
      ctx.lineWidth = 1;
      ctx.lineJoin = 'miter';
      ctx.fill(path);
      ctx.stroke(path);

      if (showLabels) {
        const anchor = screenPoints.reduce((best, point) => {
          if (!best || point.y < best.y || (point.y === best.y && point.x < best.x)) return point;
          return best;
        }, null);
        ctx.fillStyle = rgbToCss(mixRgb(base, parseHexColor(canvasColors.text), 0.5), 0.88);
        ctx.font = 'bold 10px monospace';
        ctx.textAlign = 'left';
        ctx.textBaseline = 'bottom';
        ctx.fillText(region.id, anchor.x + 5, anchor.y - 5);
      }
    }
    return alreadyDrawn;
  }

  function drawPhenotypeNode(ctx, node) {
    const geometry = getNodeGeometry(node);
    const screen = worldToScreen(node.x, node.y);
    const projection = { x: screen.x, y: screen.y, scale: camera.zoom };
    return paintFileOrganism(ctx, geometry, { ...projection, showCallableLabels: false, detail: 'screen' });
  }

  function drawFigurineNode(ctx, node, x0, y0, w, h, showExtensionText) {
    const palette = getNodePalette(node);
    const geometry = buildFigurineGeometry(x0, y0, w, h);
    const path = geometry.path;
    const fillRgb = parseHexColor(palette.fill);
    const edgeRgb = parseHexColor(palette.edge);
    const accentRgb = parseHexColor(palette.accent);
    const bgRgb = parseHexColor(canvasColors.bg100);
    const textRgb = parseHexColor(canvasColors.text);

    const paperTop = mixRgb(textRgb, fillRgb, 0.22);
    const paperBottom = mixRgb(bgRgb, fillRgb, 0.30);
    const paperGradient = ctx.createLinearGradient(geometry.docX, geometry.docY, geometry.docX, geometry.docY + geometry.docH);
    paperGradient.addColorStop(0, rgbToCss(paperTop, 0.98));
    paperGradient.addColorStop(1, rgbToCss(paperBottom, 0.98));

    ctx.fillStyle = paperGradient;
    ctx.fill(path);
    ctx.lineWidth = 1.3;
    ctx.strokeStyle = palette.edge;
    ctx.stroke(path);

    // Folded corner detail.
    if (geometry.foldCorner) {
      const fc = geometry.foldCorner;
      const foldPath = new Path2D();
      foldPath.moveTo(fc.x, fc.y);
      foldPath.lineTo(fc.x + fc.w, fc.y + fc.h);
      foldPath.lineTo(fc.x, fc.y + fc.h);
      foldPath.closePath();
      ctx.fillStyle = rgbToCss(mixRgb(paperTop, textRgb, 0.36), 0.96);
      ctx.fill(foldPath);
      ctx.strokeStyle = rgbToCss(mixRgb(edgeRgb, textRgb, 0.18), 0.9);
      ctx.stroke(foldPath);
    }

    // "Text lines" inside document.
    ctx.save();
    ctx.clip(path);
    const lineColor = rgbToCss(mixRgb(bgRgb, edgeRgb, 0.34), 0.45);
    const startX = geometry.docX + geometry.docW * 0.12;
    const endX = geometry.docX + geometry.docW * 0.86;
    const firstY = geometry.docY + geometry.docH * 0.34;
    const spacing = Math.max(4, geometry.docH * 0.11);
    ctx.strokeStyle = lineColor;
    ctx.lineWidth = 1;
    for (let i = 0; i < 5; i++) {
      const ly = firstY + i * spacing;
      ctx.beginPath();
      ctx.moveTo(startX, ly);
      ctx.lineTo(endX - (i % 2) * geometry.docW * 0.08, ly);
      ctx.stroke();
    }
    // Extension accent strip near top.
    ctx.fillStyle = rgbToCss(mixRgb(accentRgb, fillRgb, 0.15), 0.35);
    ctx.fillRect(
      geometry.docX + geometry.docW * 0.12,
      geometry.docY + geometry.docH * 0.20,
      geometry.docW * 0.46,
      Math.max(3, geometry.docH * 0.07)
    );
    ctx.restore();

    if (showExtensionText) {
      const rawExt = (palette.ext || 'misc').toUpperCase();
      const ext = rawExt.length > 4 ? rawExt.slice(0, 4) : rawExt;
      const tagW = Math.max(22, ext.length * 6 + 8);
      const tagH = 12;
      let tagX = geometry.tagX;
      if (geometry.tagAlign === 'center') tagX -= tagW / 2;
      const tagY = geometry.tagY;
      const tagBg = mixRgb(parseHexColor(canvasColors.bg100), parseHexColor(palette.fill), 0.30);
      const tagText = mixRgb(parseHexColor(canvasColors.text), parseHexColor(palette.accent), 0.25);

      ctx.fillStyle = rgbToCss(tagBg, 0.72);
      ctx.fillRect(tagX, tagY, tagW, tagH);
      ctx.strokeStyle = palette.accent;
      ctx.lineWidth = 1;
      ctx.strokeRect(tagX, tagY, tagW, tagH);
      ctx.fillStyle = rgbToCss(tagText, 0.95);
      ctx.font = 'bold 9px monospace';
      ctx.textAlign = 'left';
      ctx.textBaseline = 'middle';
      ctx.fillText(ext, tagX + 4, tagY + tagH / 2 + 0.5);
    }

    return path;
  }

  function render() {
    if (!canvas || width === 0 || height === 0) return;

    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    ctx.clearRect(0, 0, width, height);

    if (simNodes.length === 0) {
      ctx.fillStyle = '#888888';
      ctx.font = '14px monospace';
      ctx.textAlign = 'center';
      ctx.fillText('No graph data', width / 2, height / 2);
      return;
    }

    const hoveredId = hoveredNode ? hoveredNode.id : null;
    const selectedIds = selectedNodeIds;
    const focusContext = hoveredId ? new Set([hoveredId]) : selectedIds;
    const hasFocusContext = focusContext.size > 0;
    const connectedIds = new Set();
    if (hoveredId) {
      for (const id of getConnectedNodeIds(hoveredId)) connectedIds.add(id);
    } else {
      for (const id of selectedIds) {
        connectedIds.add(id);
        for (const neighborId of getConnectedNodeIds(id)) connectedIds.add(neighborId);
      }
    }
    const pulseSeconds = performance.now() * 0.001;
    const heatRuntime = buildHeatRuntimeIndex(Date.now());
    const detail = getGraphDetailLevel(camera.zoom);

    ctx.save();

    const drawnRegions = drawPathRegionLayer(
      ctx,
      sceneRegions(1),
      detail.name === 'repository' ? 0.18 : 0.09,
      true,
    );
    if (detail.regionDepth !== 1) {
      drawPathRegionLayer(
        ctx,
        sceneRegions(detail.regionDepth),
        detail.name === 'anatomy' ? 0.12 : 0.1,
        detail.name !== 'repository',
        drawnRegions,
      );
    }

    const nodeById = {};
    for (const n of simNodes) nodeById[n.id] = n;

    for (const edge of simEdges) {
      const source = nodeById[edge.source];
      const target = nodeById[edge.target];
      if (!source || !target) continue;

      const s = worldToScreen(source.x, source.y);
      const t = worldToScreen(target.x, target.y);

      if (
        Math.max(s.x, t.x) < 0 || Math.min(s.x, t.x) > width ||
        Math.max(s.y, t.y) < 0 || Math.min(s.y, t.y) > height
      ) continue;

      const edgeHighlighted = focusContext.has(edge.source) || focusContext.has(edge.target);

      ctx.beginPath();
      ctx.moveTo(s.x, s.y);
      ctx.lineTo(t.x, t.y);

      if (hasFocusContext && !edgeHighlighted) {
        ctx.strokeStyle = 'rgba(80, 80, 80, 0.15)';
        ctx.lineWidth = 1;
      } else {
        ctx.strokeStyle = EDGE_COLORS[edge.type] || DEFAULT_EDGE_COLOR;
        ctx.lineWidth = edgeHighlighted ? 2 : 1;
      }

      ctx.stroke();
    }

    for (const node of simNodes) {
      if (!isNodeVisible(node)) continue;

      const geometry = getNodeGeometry(node);
      const screen = worldToScreen(node.x, node.y);
      const w = geometry.bounds.width * camera.zoom;
      const h = geometry.bounds.height * camera.zoom;
      const x0 = screen.x + geometry.bounds.minX * camera.zoom;
      const y0 = screen.y + geometry.bounds.minY * camera.zoom;

      const isHighlighted = focusContext.has(node.id);
      const isConnected = connectedIds.has(node.id);
      const isDimmed = hasFocusContext && !isHighlighted && !isConnected;

      const cs = node.change_status;
      const isNeighbor = cs === 'neighbor';
      const heatInfo = node.file ? heatRuntime.index.get(node.file) : null;
      const heat = heatInfo ? heatInfo.intensity : 0;
      const hasHeat = Boolean(heatInfo);
      const heatPulse = hasHeat
        ? (0.5 + 0.5 * Math.sin(
            pulseSeconds * Math.PI * 2 * heatInfo.pulseFrequency + (node.x + node.y) * 0.004
          ))
        : 0;

      const palette = getNodePalette(node);
      let alpha = isDimmed ? 0.15 : isNeighbor ? 0.4 : 1;
      if (hasHeat && isDimmed) alpha = Math.max(alpha, 0.55);

      ctx.globalAlpha = alpha;

      const bodyPath = drawPhenotypeNode(ctx, node);

      if (hasHeat) {
        ctx.save();
        // Pulsing warm overlay so touched nodes are obvious even without zoom.
        const overlayAlpha = 0.14 + 0.16 * heat * heatPulse;
        ctx.fillStyle = `rgba(255, 160, 0, ${overlayAlpha})`;
        ctx.fill(bodyPath);
        ctx.restore();
      }

      if (isHighlighted) {
        ctx.strokeStyle = canvasColors.text;
        ctx.lineWidth = 2;
        ctx.stroke(bodyPath);
      } else if (isConnected) {
        ctx.strokeStyle = canvasColors.text;
        ctx.globalAlpha = 0.5;
        ctx.lineWidth = 1;
        ctx.stroke(bodyPath);
        ctx.globalAlpha = alpha;
      }

      // Heat Highlight (Recently touched): animated glow + pulse outline.
      if (hasHeat) {
        ctx.save();
        const glowAlpha = 0.25 + 0.75 * heat * (0.45 + 0.55 * heatPulse);
        const glowBlur = 12 + 34 * heat * (0.4 + 0.6 * heatPulse);
        ctx.shadowBlur = glowBlur;
        ctx.shadowColor = `rgba(255, 130, 20, ${glowAlpha})`;
        ctx.strokeStyle = `rgba(255, 200, 80, ${0.45 + 0.55 * heatPulse})`;
        ctx.lineWidth = 2.5 + 4.5 * heat * (0.45 + 0.55 * heatPulse);
        ctx.stroke(bodyPath);

        // Secondary ring to make pulse timing explicit.
        ctx.shadowBlur = 0;
        ctx.strokeStyle = `rgba(255, 235, 150, ${0.15 + 0.45 * heatPulse})`;
        ctx.lineWidth = 1.2 + 1.8 * heat;
        ctx.stroke(bodyPath);
        ctx.restore();
      }

      if (detail.showFileLabels) {
        const fontSize = 11;
        ctx.font = `bold ${fontSize}px monospace`;
        ctx.fillStyle = canvasColors.text;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'bottom';
        const label = node.label || getShortName(node.id);
        ctx.fillText(label, screen.x, y0 + h - 5);

        const trait = getNodeGeometry(node).interpretation?.role;
        if (trait) {
          ctx.font = '9px monospace';
          ctx.fillStyle = canvasColors.text;
          ctx.fillText(`Jev: ${trait}`, screen.x, y0 - 3);
        }

        if (detail.showAnatomyMarks) {
          ctx.font = '8px monospace';
          ctx.globalAlpha = Math.min(alpha, 0.68);
          const phenotype = buildOverviewPhenotype(node, node.static_degree, camera.zoom);
          const callableCount = phenotype.callablesKnown ? `${phenotype.capturedCallableCount} functions` : 'functions unknown';
          ctx.fillText(callableCount, screen.x, y0 + h + 7);
        }
      }

      ctx.globalAlpha = 1;
    }

    ctx.restore();
  }

  function zoomAt(screenX, screenY, factor) {
    const worldBefore = screenToWorld(screenX, screenY);
    camera.zoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, camera.zoom * factor));
    const worldAfter = screenToWorld(screenX, screenY);
    camera.x -= worldAfter.x - worldBefore.x;
    camera.y -= worldAfter.y - worldBefore.y;
  }

  function zoomAtCenter(factor) {
    zoomAt(width / 2, height / 2, factor);
  }

  function handleWheel(event) {
    event.preventDefault();

    // Mouse wheel pans the graph; pinch/modified wheel zooms.
    if (!(event.ctrlKey || event.metaKey)) {
      camera.x += event.deltaX / camera.zoom;
      camera.y += event.deltaY / camera.zoom;
      return;
    }

    const rect = canvas.getBoundingClientRect();
    const mx = event.clientX - rect.left;
    const my = event.clientY - rect.top;
    const clampedDelta = Math.max(-80, Math.min(80, event.deltaY));
    const zoomFactor = Math.exp(-clampedDelta * 0.0025);
    zoomAt(mx, my, zoomFactor);
  }

  function handleKeyDown(event) {
    if (!(event.metaKey || event.ctrlKey)) return;

    const target = event.target;
    if (
      target instanceof HTMLElement &&
      (target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.isContentEditable)
    ) {
      return;
    }

    if (event.key.toLowerCase() === 't') {
      event.preventDefault();
      tileFloatingWindows();
      return;
    }

    if (event.key === '=' || event.key === '+' || event.key === 'NumpadAdd') {
      event.preventDefault();
      zoomAtCenter(1.14);
      return;
    }

    if (event.key === '-' || event.key === '_' || event.key === 'NumpadSubtract') {
      event.preventDefault();
      zoomAtCenter(1 / 1.14);
    }
  }

  function handleMouseDown(event) {
    if (event.button !== 0) return;

    const rect = canvas.getBoundingClientRect();
    const mx = event.clientX - rect.left;
    const my = event.clientY - rect.top;

    const node = findNodeAt(mx, my);
    if (node) {
      const world = screenToWorld(mx, my);
      draggedNode = node;
      dragOffset = { x: node.x - world.x, y: node.y - world.y };
      dragStart = { x: event.clientX, y: event.clientY };
      node.pinned = true;
      node.vx = 0;
      node.vy = 0;
      animating = true;
      return;
    }

    dragging = true;
    dragStart = { x: event.clientX, y: event.clientY };
    dragCameraStart = { x: camera.x, y: camera.y };
  }

  function handleMouseMove(event) {
    const rect = canvas.getBoundingClientRect();
    const mx = event.clientX - rect.left;
    const my = event.clientY - rect.top;

    if (draggedNode) {
      const world = screenToWorld(mx, my);
      draggedNode.x = world.x + dragOffset.x;
      draggedNode.y = world.y + dragOffset.y;
      draggedNode.vx = 0;
      draggedNode.vy = 0;
      scenePositionRevision++;
      sceneMotionVersion++;
      frameScheduler?.invalidate();
      return;
    }

    if (dragging) {
      const dx = (event.clientX - dragStart.x) / camera.zoom;
      const dy = (event.clientY - dragStart.y) / camera.zoom;
      camera.x = dragCameraStart.x - dx;
      camera.y = dragCameraStart.y - dy;
      return;
    }

    hoveredNode = findNodeAt(mx, my);
  }

  function handleMouseUp(event) {
    if (draggedNode) {
      const dx = event.clientX - dragStart.x;
      const dy = event.clientY - dragStart.y;
      const moved = Math.sqrt(dx * dx + dy * dy);

      draggedNode.pinned = false;
      if (moved < 5) {
        focusNode(draggedNode);
        if (event.detail >= 2 && draggedNode.file) {
          openSourceWindow(draggedNode.file, getShortName(draggedNode.id));
        }
      }
      draggedNode = null;
      return;
    }
    if (dragging) {
      const dx = event.clientX - dragStart.x;
      const dy = event.clientY - dragStart.y;
      const moved = Math.sqrt(dx * dx + dy * dy);
      if (moved < 5) {
        activeNodeId = null;
      }
    }
    dragging = false;
  }


  function handleMouseLeave() {
    if (draggedNode) {
      draggedNode.pinned = false;
      draggedNode = null;
    }
    dragging = false;
    hoveredNode = null;
  }

  $effect(() => {
    const nodeIdSet = new Set(simNodes.map((node) => node.id));
    const filteredSelected = new Set([...selectedNodeIds].filter((id) => nodeIdSet.has(id)));
    if (!areSetsEqual(filteredSelected, selectedNodeIds)) {
      selectedNodeIds = filteredSelected;
    }

    const filteredWindows = nodeWindows.filter((w) => nodeIdSet.has(w.nodeId));
    if (filteredWindows.length !== nodeWindows.length) {
      nodeWindows = filteredWindows;
    }

    if (activeNodeId && !nodeIdSet.has(activeNodeId)) {
      activeNodeId = filteredWindows.length > 0 ? filteredWindows[filteredWindows.length - 1].nodeId : null;
    }
  });

  $effect(() => {
    initSimulation();
  });

  $effect(() => {
    const _pending = pendingAutoCenter;
    const _w = width;
    const _h = height;
    const _nodes = simNodes;
    if (!_pending || _w <= 0 || _h <= 0 || _nodes.length === 0) return;

    centerCameraOnNodes(_nodes);
    pendingAutoCenter = false;
  });

  $effect(() => {
    const _nodes = simNodes;
    const _edges = simEdges;
    const _w = width;
    const _h = height;
    const _cam = camera;
    const _hov = hoveredNode;
    const _sel = selectedNodeIds;
    const _active = activeNodeId;
    const _anim = animating;
    const _drag = draggedNode;
    const _windows = nodeWindows;
    const _heat = heatmapData;
    const _theme = theme;
    const _zoom = camera.zoom;
    const _cameraX = camera.x;
    const _cameraY = camera.y;

    if (!canvas || _w === 0 || _h === 0) return;

    frameScheduler = createFrameScheduler({ request: callback => requestAnimationFrame(callback), cancel: id => cancelAnimationFrame(id), frame() {
      if (document.hidden) return false;
      const profileStart = performance.now();
      if (animating) stepSimulation();
      const profilePhysicsEnd = performance.now();
      render();
      sceneMetrics.physics = profilePhysicsEnd - profileStart;
      sceneMetrics.render = performance.now() - profilePhysicsEnd;
      sceneMetrics.frames++;
      if (sceneMetrics.frames % 20 === 0) {
        canvas.dataset.physicsMs = sceneMetrics.physics.toFixed(2);
        canvas.dataset.renderMs = sceneMetrics.render.toFixed(2);
        canvas.dataset.treeVisits = String(sceneMetrics.treeVisits);
        canvas.dataset.collisionTests = String(sceneMetrics.collisionTests);
        canvas.dataset.preparationMs = sceneMetrics.preparation.toFixed(2);
      }
      return animating || Boolean(draggedNode) || heatmapData.size > 0;
    } });
    const scheduler = frameScheduler;
    const resume = () => { if (!document.hidden) scheduler.invalidate(); };
    document.addEventListener('visibilitychange', resume);
    scheduler.invalidate();
    return () => { scheduler.dispose(); document.removeEventListener('visibilitychange', resume); };
  });
  function isTickActive(change, index, total) {
    if (!since) return false;
    const parts = since.split(' | ').map(s => s.trim());
    if (parts.includes(change.id)) return true;
    if (parts.includes('@') && index === total - 1) return true;
    // Check if since parts contain a bookmark pointing here
    if (bookmarks.some(b => parts.includes(b.name) && b.id === change.id)) return true;
    return false;
  }

  function getTickBookmarks(changeId) {
    return bookmarks.filter(b => b.id === changeId).map(b => b.name);
  }
</script>

<svelte:window onkeydown={handleKeyDown} />

<div class="eyeloss-container" bind:clientWidth={width} bind:clientHeight={height}>
  <canvas
    class="eyeloss-graph-canvas"
    aria-label="Repository organism graph"
    aria-description={`${organismGraphStats.files} files in ${organismGraphStats.pathRegions} path regions with ${organismGraphStats.staticLinks} static links`}
    data-renderer="organism-regions"
    data-file-organism-engine="shared-world"
    data-scene-runtime="spatial"
    data-graph-seed={graphMeta?.seedLabel || ''}
    data-camera-x={camera.x}
    data-camera-y={camera.y}
    data-camera-zoom={camera.zoom}
    data-file-count={organismGraphStats.files}
    data-path-region-count={organismGraphStats.pathRegions}
    data-static-link-count={organismGraphStats.staticLinks}
    data-detail-level={getGraphDetailLevel(camera.zoom).name}
    data-phenotype-grammar="shared-callable-metrics"
    data-callable-count={simNodes.reduce((total, node) => total + nodeCallableCount(node), 0)}
    data-callable-coverage={simNodes.filter(nodeHasKnownCallables).length}
    data-callable-unknown-files={simNodes.filter((node) => !nodeHasKnownCallables(node)).length}
    data-metric-anatomy-count={simNodes.filter((node) => Array.isArray(node.anatomy?.callables)).length}
    data-lod-mode="nonlinear-semantic"
    bind:this={canvas}
    onwheel={handleWheel}
    onmousedown={handleMouseDown}
    onmousemove={handleMouseMove}
    onmouseup={handleMouseUp}
    onmouseleave={handleMouseLeave}
    style:cursor={draggedNode ? 'grabbing' : hoveredNode ? 'grab' : dragging ? 'move' : 'default'}
  ></canvas>

  <div class="eyeloss-nav-panel" class:eyeloss-nav-panel--collapsed={panelCollapsed}>
    <div style="display: flex;">
      <button
        class="eyeloss-nav-panel__toggle"
        type="button"
        onclick={() => panelCollapsed = !panelCollapsed}
        style="flex: 1;"
      >
        {panelCollapsed ? '>' : '<'} Nodes ({simNodes.length})
      </button>
      {#if !panelCollapsed}
        <button
          class="eyeloss-nav-panel__toggle"
          type="button"
          onclick={toggleDefaultNodeWindowViewMode}
          title="Default mode for newly opened node windows"
          style="border-left: 1px solid var(--bg-300); width: 88px; text-align: center;"
        >
          Open: {defaultNodeWindowViewMode === 'bug' ? 'Bug' : defaultNodeWindowViewMode === 'source' ? 'Source' : 'Diff'}
        </button>
        <button
          class="eyeloss-nav-panel__toggle"
          type="button"
          onclick={tileFloatingWindows}
          title="Tile floating windows (Cmd/Ctrl+T)"
          style="border-left: 1px solid var(--bg-300); width: 74px; text-align: center;"
        >
          Tile
        </button>
      {/if}
    </div>
    {#if !panelCollapsed}
      <div class="eyeloss-nav-panel__search">
        <input
          bind:this={searchInput}
          type="text"
          placeholder="Filter nodes..."
          bind:value={searchQuery}
          class="eyeloss-nav-panel__input"
        />
      </div>
      <ul class="eyeloss-nav-panel__list">
        {#each filteredNodes as node, i}
          {@const workerSessionId = nodeWorkerSessionId(node)}
          {@const verificationState = nodeVerificationState(node)}
          <li>
            <button
              class="eyeloss-nav-panel__item"
              class:eyeloss-nav-panel__item--active={selectedNodeIds.has(node.id) || highlightedSearchIndex === i}
              data-node-id={normalizeFilePath(node.file || node.id)}
              data-worker-session-id={workerSessionId || undefined}
              data-verification-state={verificationState || undefined}
              type="button"
              title={nodeProvenanceById.get(node.id) ? `${getShortName(node.id)} — ${nodeProvenanceById.get(node.id)}` : getShortName(node.id)}
              onclick={() => { focusNode(node); highlightedSearchIndex = i; }}
              ondblclick={() => openSourceWindow(node.file, getShortName(node.id))}
            >
              <span class="eyeloss-nav-panel__dot" data-ns={getNamespace(node.id)}></span>
              <span class="eyeloss-nav-panel__body">
                <span class="eyeloss-nav-panel__name">{getNavNodeLabel(node)}</span>
                {#if nodeProvenanceById.get(node.id)}
                  <span class="eyeloss-nav-panel__meta">{nodeProvenanceById.get(node.id)}</span>
                {/if}
                {#if workerSessionId}
                  <span class="eyeloss-nav-panel__meta">worker {workerSessionId}</span>
                {/if}
                {#if verificationState}
                  <span class="eyeloss-nav-panel__meta">verification {verificationState}</span>
                {/if}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  {#if changes.length > 0}
    <div class="eyeloss-timeline">
      <div class="eyeloss-timeline__status">{timelineStatus}</div>
      <div class="eyeloss-timeline__description" aria-live="polite">
        {#if timelineFocusChange}
          <span class="eyeloss-timeline__description-id">{timelineFocusChange.id}</span>
          <span class="eyeloss-timeline__description-text">{timelineFocusDescription}</span>
        {/if}
      </div>
      <button
        class="eyeloss-timeline__nav eyeloss-timeline__nav--back"
        type="button"
        aria-label="Older commits page"
        title="Older commits page"
        onclick={moveTimelineBack}
        disabled={loadingMore || timelinePaging || !timelinePage.canMoveBack}
      ></button>
      <div class="eyeloss-timeline__track" bind:this={timelineTrackEl}>
        {#if loadingMore || timelinePaging}
          <div class="eyeloss-timeline__loader">...</div>
        {/if}
        {#each timelinePage.items as change, i}
          {@const globalIndex = timelinePage.start + i}
          {@const tickHeat = (globalIndex + 1) / (timelinePage.total || 1)}
          {@const ts = (change.timestamp || '').trim().split(/\s+/)}
          {@const tsDate = ts[0] || ''}
          {@const tsTime = ts.length >= 2 ? ts.slice(1).join(' ') : ''}
          <button
            class="eyeloss-timeline__tick"
            class:eyeloss-timeline__tick--active={isTickActive(change, globalIndex, timelinePage.total)}
            class:eyeloss-timeline__tick--wc={globalIndex === timelinePage.total - 1}
            type="button"
            onclick={(e) => onSelectSince && onSelectSince(change.id, e)}
            onmouseenter={() => timelineHoverId = change.id}
            onmouseleave={() => { if (timelineHoverId === change.id) timelineHoverId = null; }}
            onfocus={() => timelineHoverId = change.id}
            onblur={() => { if (timelineHoverId === change.id) timelineHoverId = null; }}
            style:--tick-heat={tickHeat}
          >
            <span class="eyeloss-timeline__dot"></span>
            <div class="eyeloss-timeline__tick-info">
              <span class="eyeloss-timeline__tick-id">{change.id}</span>
              <span class="eyeloss-timeline__tick-time">
                <span>{tsDate}</span>
                {#if tsTime}
                  <span>{tsTime}</span>
                {/if}
              </span>
            </div>
          </button>
        {/each}
      </div>
      <button
        class="eyeloss-timeline__nav eyeloss-timeline__nav--forward"
        type="button"
        aria-label="Newer commits page"
        title="Newer commits page"
        onclick={moveTimelineForward}
        disabled={!timelinePage.canMoveForward}
      ></button>
    </div>
  {/if}

  {#each nodeWindows as nodeWindow (nodeWindow.id)}
    {@const nodeData = findNodeById(nodeWindow.nodeId)}
    {#if nodeData}
      <NodeDetailsWindow
        node={nodeData}
        {width}
        {height}
        x={nodeWindow.x}
        y={nodeWindow.y}
        windowWidth={nodeWindow.width}
        windowHeight={nodeWindow.height}
        active={activeNodeId === nodeWindow.nodeId}
        {theme}
        {contentVersion}
        {fileContentVersions}
        {comparisonSelection}
        {getNodeHeat}
        {getFileDiff}
        {getFileSource}
        {getFileAnatomy}
        {getFileAnatomyComparison}
        jevAssessment={jevByFile[normalizeFilePath(nodeData.file)]}
        {invokeJev}
        {saveFile}
        initialViewMode={nodeWindow.initialViewMode || 'bug'}
        sourceReferences={getSourceReferenceLinksForNode(nodeWindow.nodeId)}
        onOpenReference={openSourceWindowForReference}
        onclose={() => closeNodeWindow(nodeWindow.nodeId)}
        onfocus={() => focusNodeWindow(nodeWindow.nodeId)}
        ondragEnd={(e) => moveNodeWindow(nodeWindow.nodeId, e.detail)}
        onresizeEnd={(e) => resizeNodeWindow(nodeWindow.nodeId, e.detail)}
      />
    {/if}
  {/each}

  <CoordinatorWindow
    {coordinatorState}
    {missionState}
    visible={coordinatorWindow.visible}
    loading={coordinatorLoading}
    sending={coordinatorSending}
    error={coordinatorError}
    bind:x={coordinatorWindow.x}
    bind:y={coordinatorWindow.y}
    bind:windowWidth={coordinatorWindow.width}
    bind:windowHeight={coordinatorWindow.height}
    containerWidth={width}
    containerHeight={height}
    onclose={() => { coordinatorWindow.visible = false; }}
    onSendMessage={onCoordinatorSend}
    onApprovePlan={onCoordinatorApprovePlan}
    onLaunchWorkers={onCoordinatorLaunchWorkers}
    onAskReport={onCoordinatorAskReport}
    onNudgeLatestWorker={onCoordinatorNudgeLatestWorker}
    onStartThread={onCoordinatorStartThread}
    onResumeThread={onCoordinatorResumeThread}
  />

  <ReviewerPresentationWindow
    {graph}
    {graphMeta}
    {reviewTarget}
    {getFileSource}
    {getReviewerTargetContext}
    {generateReviewerElm}
    {compileReviewerElm}
    onReviewTargetChange={onReviewTargetChange}
    visible={reviewerWindow.visible}
    bind:x={reviewerWindow.x}
    bind:y={reviewerWindow.y}
    bind:windowWidth={reviewerWindow.width}
    bind:windowHeight={reviewerWindow.height}
    containerWidth={width}
    containerHeight={height}
    onclose={() => { reviewerWindow.visible = false; }}
  />

  {#if !coordinatorWindow.visible || !reviewerWindow.visible}
    <nav class="eyeloss-window-dock" aria-label="Window dock">
      {#if !coordinatorWindow.visible}
        <button
          type="button"
          class="eyeloss-window-dock__button"
          onclick={() => { coordinatorWindow.visible = true; }}
        >
          <span class="eyeloss-window-dock__title">Coordinator</span>
          <span class="eyeloss-window-dock__state">{coordinatorDockStateLabel()}</span>
        </button>
      {/if}

      {#if !reviewerWindow.visible}
        <button
          type="button"
          class="eyeloss-window-dock__button"
          onclick={() => { reviewerWindow.visible = true; }}
        >
          <span class="eyeloss-window-dock__title">Review</span>
          <span class="eyeloss-window-dock__state">{reviewerDockStateLabel()}</span>
        </button>
      {/if}
    </nav>
  {/if}

  {#each sourceWindows as sourceWindow (sourceWindow.id)}
    <Window
      title={`Source: ${sourceWindow.title}`}
      width={sourceWindow.width}
      height={sourceWindow.height}
      x={sourceWindow.x}
      y={sourceWindow.y}
      containerWidth={width}
      containerHeight={height}
      onclose={() => closeSourceWindow(sourceWindow.id)}
      onfocus={() => focusSourceWindow(sourceWindow.id)}
      ondragEnd={(e) => moveSourceWindow(sourceWindow.id, e.detail)}
      onresizeEnd={(e) => resizeSourceWindow(sourceWindow.id, e.detail)}
    >
      <div class="eyeloss-node-details__body eyeloss-node-details__body--window" onwheel={(e) => e.stopPropagation()}>
        <div class="eyeloss-node-details__meta" style="margin-bottom: 8px; font-size: 11px;">
          <span>{sourceWindow.file}</span>
        </div>
        {#if sourceWindow.loading}
          <div class="eyeloss-node-details__meta" style="margin-top: 16px;">Loading source...</div>
        {:else}
          <Editor
            value={sourceWindow.content}
            file={sourceWindow.file}
            {theme}
            readOnly={true}
          />
        {/if}
      </div>
    </Window>
  {/each}

  <svg style="position: absolute; top: 0; left: 0; width: 100%; height: 100%; pointer-events: none; z-index: 95;">
    <Robot
      targetX={robotTarget.x}
      targetY={robotTarget.y}
      active={robotTarget.active}
      instanceId="file-touch"
    />
    {#each normalizedMissionRobotTargets as target}
      <Robot
        targetX={target.x}
        targetY={target.y}
        active={target.active}
        robotState={target.state}
        instanceId={target.instanceId}
      />
    {/each}
  </svg>
</div>
