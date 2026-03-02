<script>
  import { onMount, untrack } from 'svelte';
  import Window from './Window.svelte';
  import Editor from './Editor.svelte';
  import NodeDetailsWindow from './NodeDetailsWindow.svelte';
  import Robot from './Robot.svelte';
  import { setupShortcuts } from './shortcuts.js';

  let { 
    graph = { nodes: [], edges: [] }, 
    since = null, 
    changes = [], 
    bookmarks = [], 
    getFileDiff, 
    getFileSource, 
    saveFile, 
    theme = 'midnight', 
    heatmapData = new Map(),
    onSelectSince,
    onLoadMoreChanges,
    onProjectChange,
    loadingMore = false,
    hasMoreChanges = true
  } = $props();

  function normalizeFilePath(path) {
    if (!path) return '';
    return String(path)
      .replaceAll('\\', '/')
      .replace(/^\.\/+/, '')
      .replace(/^\/+/, '');
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
    const fallbackNode =
      simNodes.find((n) => n.change_status === 'modified' || n.change_status === 'added') ||
      simNodes[0] ||
      null;
    const targetNode = lastEditedNode || fallbackNode;
    if (!targetNode) return { x: 0, y: 0, active: false };
    const screen = worldToScreen(targetNode.x, targetNode.y);
    const size = getNodeSize(targetNode);
    // Keep robot on the edited document (upper-right area), not floating far away.
    const desiredX = screen.x + size.w * 0.22;
    const desiredY = screen.y - size.h * 0.28;
    if (width <= 0 || height <= 0) {
      return { x: desiredX, y: desiredY, active: true };
    }

    const marginX = 56;
    const marginY = 56;
    return {
      x: Math.max(marginX, Math.min(width - marginX, desiredX)),
      y: Math.max(marginY, Math.min(height - marginY, desiredY)),
      active: true,
    };
  });

  let searchInput = $state(null);
  let highlightedSearchIndex = $state(-1);

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

  let simNodes = $state([]);
  let simEdges = $state([]);
  let animating = $state(true);
  let selectedNodeIds = $state(new Set());
  let activeNodeId = $state(null);
  let nodeWindows = $state([]);
  let hoveredNode = $state(null);
  let graphNodesById = $derived.by(() => new Map((graph.nodes || []).map((node) => [node.id, node])));

  const diffCache = new Map();
  const sourceCache = new Map();
  let sourceWindows = $state([]);

  let panelCollapsed = $state(false);
  let searchQuery = $state('');

  let dragging = $state(false);
  let dragStart = $state({ x: 0, y: 0 });
  let dragCameraStart = $state({ x: 0, y: 0 });

  let draggedNode = $state(null);
  let dragOffset = $state({ x: 0, y: 0 });

  let rafId = null;
  let simulationIterations = 0;
  const MAX_INITIAL_ITERATIONS = 300;
  const SETTLING_ITERATIONS = 50;
  const MIN_ZOOM = 0.05;
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

  function getNodeClusterKey(node, degreeById) {
    const degree = degreeById[node.id] || 0;
    if (degree === 0) {
      return `ext:${getNodeExtension(node)}`;
    }
    return getClusterKey(node.id);
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

  function getNodeSize(node) {
    const lc = node.line_count || 50;
    const scale = Math.pow(Math.max(1, lc) / 40, 0.4);
    return {
      w: Math.min(220, 68 + 56 * scale),
      h: Math.min(180, 52 + 50 * scale),
    };
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
      const size = getNodeSize(node);
      minX = Math.min(minX, node.x - size.w / 2);
      maxX = Math.max(maxX, node.x + size.w / 2);
      minY = Math.min(minY, node.y - size.h / 2);
      maxY = Math.max(maxY, node.y + size.h / 2);
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
    camera.zoom = Math.max(0.25, Math.min(1.4, fitZoom));
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
      x: Math.max(16, 24 + slot * 28),
      y: Math.max(16, 20 + slot * 22),
      width: detailWidth,
      height: detailHeight,
    };
    nodeWindows = [...nodeWindows, nextWindow];
  }

  function focusNode(node) {
    if (!node) return;
    camera.x = node.x;
    camera.y = node.y;
    camera.zoom = Math.max(camera.zoom, 1.5);

    const next = new Set(selectedNodeIds);
    next.add(node.id);
    selectedNodeIds = next;
    activeNodeId = node.id;
    ensureNodeWindow(node);
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
    const sourceWindow = {
      id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      file: normalized,
      title: title || getShortName(normalized),
      content: '',
      loading: true,
      x: Math.max(20, 28 + slot * 34),
      y: Math.max(20, 28 + slot * 26),
      width: Math.min(780, Math.max(420, width * 0.62)),
      height: Math.min(680, Math.max(320, height * 0.74)),
    };

    sourceWindows = [...sourceWindows, sourceWindow];

    try {
      const cached = sourceCache.get(normalized);
      const source = cached !== undefined ? cached : await getFileSource(normalized);
      sourceCache.set(normalized, source || '');
      sourceWindows = sourceWindows.map((w) =>
        w.id === sourceWindow.id ? { ...w, content: source || '', loading: false } : w
      );
    } catch (e) {
      sourceWindows = sourceWindows.map((w) =>
        w.id === sourceWindow.id
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
    if (!graph.nodes || graph.nodes.length === 0) {
      simNodes = [];
      simEdges = [];
      return;
    }

    const degreeById = {};
    for (const node of graph.nodes) degreeById[node.id] = 0;
    for (const edge of graph.edges || []) {
      if (degreeById[edge.source] !== undefined) degreeById[edge.source]++;
      if (degreeById[edge.target] !== undefined) degreeById[edge.target]++;
    }

    const clusters = {};
    for (const node of graph.nodes) {
      const key = getNodeClusterKey(node, degreeById);
      if (!clusters[key]) clusters[key] = [];
      clusters[key].push(node);
    }

    const clusterKeys = Object.keys(clusters);
    const clusterCount = clusterKeys.length;
    const clusterRadius = clusterCount <= 1
      ? 0
      : Math.min(460, 110 + clusterCount * 65);
    const nodeJitter = Math.max(70, Math.min(160, 90 + Math.sqrt(graph.nodes.length) * 5));

    const clusterCenters = {};
    clusterKeys.forEach((key, i) => {
      const angle = clusterCount <= 1 ? 0 : (i / clusterCount) * Math.PI * 2;
      clusterCenters[key] = {
        x: Math.cos(angle) * clusterRadius,
        y: Math.sin(angle) * clusterRadius,
      };
    });

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
      const clusterKey = getNodeClusterKey(node, degreeById);
      const center = clusterCenters[clusterKey];
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
        cluster_key: clusterKey,
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
  }

  function stepSimulation() {
    const nodes = simNodes;
    const edges = simEdges;
    if (nodes.length === 0) return;
    const heatRuntime = buildHeatRuntimeIndex(Date.now());
    const heatIndex = heatRuntime.index;

    const nodeMap = {};
    for (const n of nodes) {
      nodeMap[n.id] = n;
    }

    const repulsionStrength = 35000;
    const springStrength = 0.003;
    const springLength = 320;
    const centerGravity = 0.0008;
    const clusterGravity = 0.015;
    const damping = 0.82;
    const maxVelocity = 20;

    const clusterSums = {};
    for (const node of nodes) {
      const key = node.cluster_key || getClusterKey(node.id);
      if (!clusterSums[key]) clusterSums[key] = { x: 0, y: 0, count: 0 };
      clusterSums[key].x += node.x;
      clusterSums[key].y += node.y;
      clusterSums[key].count++;
    }
    const clusterCenters = {};
    for (const key in clusterSums) {
      const s = clusterSums[key];
      clusterCenters[key] = { x: s.x / s.count, y: s.y / s.count };
    }

    const cKeys = Object.keys(clusterCenters);
    const interClusterRepulsion = 90000;
    const clusterForces = {};
    for (const key of cKeys) clusterForces[key] = { x: 0, y: 0 };

    for (let i = 0; i < cKeys.length; i++) {
      for (let j = i + 1; j < cKeys.length; j++) {
        const ca = clusterCenters[cKeys[i]];
        const cb = clusterCenters[cKeys[j]];
        let dx = cb.x - ca.x;
        let dy = cb.y - ca.y;
        let dist = Math.sqrt(dx * dx + dy * dy);
        if (dist < 10) dist = 10;

        const force = interClusterRepulsion / (dist * dist);
        const fx = (dx / dist) * force;
        const fy = (dy / dist) * force;

        clusterForces[cKeys[i]].x -= fx;
        clusterForces[cKeys[i]].y -= fy;
        clusterForces[cKeys[j]].x += fx;
        clusterForces[cKeys[j]].y += fy;
      }
    }

    for (let i = 0; i < nodes.length; i++) {
      const a = nodes[i];
      const sizeA = getNodeSize(a);

      for (let j = i + 1; j < nodes.length; j++) {
        const b = nodes[j];
        if (a.pinned && b.pinned) continue;
        const sizeB = getNodeSize(b);
        const invMassA = 1 / (a.mass || 1);
        const invMassB = 1 / (b.mass || 1);

        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let dist = Math.sqrt(dx * dx + dy * dy);
        
        if (dist < 1) {
          dx = (Math.random() - 0.5) * 2;
          dy = (Math.random() - 0.5) * 2;
          dist = Math.sqrt(dx * dx + dy * dy);
        }

        const sameCluster = (a.cluster_key || getClusterKey(a.id)) === (b.cluster_key || getClusterKey(b.id));
        const rep = sameCluster ? repulsionStrength : repulsionStrength * 1.5;
        let force = rep / (dist * dist);

        const fx = (dx / dist) * force;
        const fy = (dy / dist) * force;

        if (!a.pinned) { a.vx -= fx * invMassA; a.vy -= fy * invMassA; }
        if (!b.pinned) { b.vx += fx * invMassB; b.vy += fy * invMassB; }

        const minDx = (sizeA.w + sizeB.w) / 2 + 16; 
        const minDy = (sizeA.h + sizeB.h) / 2 + 16;
        
        if (Math.abs(dx) < minDx && Math.abs(dy) < minDy) {
          const overlapX = minDx - Math.abs(dx);
          const overlapY = minDy - Math.abs(dy);
          
          let pushRatioA = 0.5;
          let pushRatioB = 0.5;
          
          if (a.pinned && !b.pinned) { pushRatioA = 0; pushRatioB = 1; }
          else if (!a.pinned && b.pinned) { pushRatioA = 1; pushRatioB = 0; }
          else if (a.pinned && b.pinned) { pushRatioA = 0; pushRatioB = 0; }
          
          const correctionLerp = 0.3;
          
          if (overlapX < overlapY) {
            const pushX = (dx > 0 ? overlapX : -overlapX) * correctionLerp;
            a.x -= pushX * pushRatioA;
            b.x += pushX * pushRatioB;
          } else {
            const pushY = (dy > 0 ? overlapY : -overlapY) * correctionLerp;
            a.y -= pushY * pushRatioA;
            b.y += pushY * pushRatioB;
          }
        }
      }
    }

    for (const edge of edges) {
      const source = nodeMap[edge.source];
      const target = nodeMap[edge.target];
      if (!source || !target) continue;

      let dx = target.x - source.x;
      let dy = target.y - source.y;
      let dist = Math.sqrt(dx * dx + dy * dy);
      if (dist < 1) dist = 1;

      const sameCluster = (source.cluster_key || getClusterKey(source.id)) === (target.cluster_key || getClusterKey(target.id));
      const sl = sameCluster ? springLength * 0.6 : springLength;
      const ss = sameCluster ? springStrength * 1.5 : springStrength * 0.3;

      const displacement = dist - sl;
      const force = ss * displacement;
      const fx = (dx / dist) * force;
      const fy = (dy / dist) * force;
      const invMassSource = 1 / (source.mass || 1);
      const invMassTarget = 1 / (target.mass || 1);

      if (!source.pinned) { source.vx += fx * invMassSource; source.vy += fy * invMassSource; }
      if (!target.pinned) { target.vx -= fx * invMassTarget; target.vy -= fy * invMassTarget; }
    }

    for (const node of nodes) {
      if (node.pinned) continue;

      const key = node.cluster_key || getClusterKey(node.id);
      const cc = clusterCenters[key];
      const cf = clusterForces[key];
      const invMass = 1 / (node.mass || 1);

      node.vx -= (node.x - cc.x) * clusterGravity * invMass;
      node.vy -= (node.y - cc.y) * clusterGravity * invMass;

      // Keep clusters vertically neutral; no artificial downward drift.

      node.vx += cf.x * invMass;
      node.vy += cf.y * invMass;

      node.vx -= node.x * centerGravity * invMass;
      node.vy -= node.y * centerGravity * invMass;

      node.vx *= damping;
      node.vy *= damping;

      const speed = Math.sqrt(node.vx * node.vx + node.vy * node.vy);
      const heavyMaxVelocity = Math.max(4.5, maxVelocity / Math.sqrt(node.mass || 1));
      if (speed > heavyMaxVelocity) {
        node.vx = (node.vx / speed) * heavyMaxVelocity;
        node.vy = (node.vy / speed) * heavyMaxVelocity;
      }

      node.x += node.vx;
      node.y += node.vy;
    }

    simulationIterations++;

    if (simulationIterations > MAX_INITIAL_ITERATIONS + SETTLING_ITERATIONS) {
      let totalEnergy = 0;
      for (const node of nodes) {
        if (!node.pinned) {
          totalEnergy += node.vx * node.vx + node.vy * node.vy;
        }
      }
      if (totalEnergy / nodes.length < 0.01) {
        animating = false;
      }
    }

    simNodes = nodes;
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
    const size = getNodeSize(node);
    const screen = worldToScreen(node.x, node.y);
    const hw = size.w / 2;
    const hh = size.h / 2;
    return (
      screen.x + hw > 0 &&
      screen.x - hw < width &&
      screen.y + hh > 0 &&
      screen.y - hh < height
    );
  }

  function findNodeAt(sx, sy) {
    const world = screenToWorld(sx, sy);
    const z = camera.zoom;

    for (let i = simNodes.length - 1; i >= 0; i--) {
      const node = simNodes[i];
      const size = getNodeSize(node);
      const hw = size.w / 2 / z;
      const hh = size.h / 2 / z;
      if (
        world.x >= node.x - hw &&
        world.x <= node.x + hw &&
        world.y >= node.y - hh &&
        world.y <= node.y + hh
      ) {
        return node;
      }
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

    ctx.save();

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

    const showFunctions = camera.zoom > 1.5;

    for (const node of simNodes) {
      if (!isNodeVisible(node)) continue;

      const size = getNodeSize(node);
      const screen = worldToScreen(node.x, node.y);
      const w = size.w;
      const h = size.h;
      const x0 = screen.x - w / 2;
      const y0 = screen.y - h / 2;

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

      const bodyPath = drawFigurineNode(ctx, node, x0, y0, w, h, camera.zoom > 0.55);

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

      const fontSize = 12;
      ctx.font = `bold ${fontSize}px monospace`;
      ctx.fillStyle = canvasColors.text;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';

      const label = node.label || getShortName(node.id);

      if (showFunctions && node.functions && node.functions.length > 0) {
        ctx.fillText(label, screen.x, y0 + fontSize + 4);

        const fnFontSize = 9;
        ctx.font = `${fnFontSize}px monospace`;
        ctx.fillStyle = canvasColors.text;
        ctx.globalAlpha = 0.7;

        const maxFns = Math.floor((h - fontSize - 12) / (fnFontSize + 2));
        const fns = node.functions.slice(0, maxFns);
        for (let i = 0; i < fns.length; i++) {
          const fn = fns[i];
          const fnLabel = `${fn.name}/${fn.arity}`;
          const fy = y0 + fontSize + 16 + i * (fnFontSize + 2);
          if (fy + fnFontSize > y0 + h) break;
          ctx.fillText(fnLabel, screen.x, fy);
        }
      } else {
        ctx.fillText(label, screen.x, screen.y);
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
      simNodes = simNodes;
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

    if (!canvas || _w === 0 || _h === 0) return;

    if (_heat.size > 0) animating = true;

    function loop() {
      if (animating) {
        stepSimulation();
      }
      render();
      rafId = requestAnimationFrame(loop);
    }

    rafId = requestAnimationFrame(loop);

    return () => {
      if (rafId) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
    };
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
          <li>
            <button
              class="eyeloss-nav-panel__item"
              class:eyeloss-nav-panel__item--active={selectedNodeIds.has(node.id) || highlightedSearchIndex === i}
              type="button"
              onclick={() => { focusNode(node); highlightedSearchIndex = i; }}
              ondblclick={() => openSourceWindow(node.file, getShortName(node.id))}
            >
              <span class="eyeloss-nav-panel__dot" data-ns={getNamespace(node.id)}></span>
              <span class="eyeloss-nav-panel__name">{getShortName(node.id)}</span>
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
        {getNodeHeat}
        {getFileDiff}
        {getFileSource}
        {saveFile}
        {diffCache}
        {sourceCache}
        sourceReferences={getSourceReferenceLinksForNode(nodeWindow.nodeId)}
        onOpenReference={openSourceWindowForReference}
        onclose={() => closeNodeWindow(nodeWindow.nodeId)}
        onfocus={() => focusNodeWindow(nodeWindow.nodeId)}
        ondragEnd={(e) => moveNodeWindow(nodeWindow.nodeId, e.detail)}
        onresizeEnd={(e) => resizeNodeWindow(nodeWindow.nodeId, e.detail)}
      />
    {/if}
  {/each}

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
    />
  </svg>
</div>
