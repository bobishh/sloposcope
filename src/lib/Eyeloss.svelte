<script>
  import { onMount, onDestroy } from 'svelte';
  import Window from './Window.svelte';
  import Editor from './Editor.svelte';

  let { 
    graph = { nodes: [], edges: [] }, 
    since = null, 
    changes = [], 
    bookmarks = [], 
    getFileDiff, 
    getFileSource, 
    saveFile, 
    theme = 'midnight', 
    touchHeat = new Map(),
    onSelectSince 
  } = $props();

  function getNodeHeat(nodeId) {
    if (touchHeat.size === 0) return 0;
    const val = touchHeat.get(nodeId);
    if (val === undefined) return 0;
    
    // Calculate rank
    const sortedValues = Array.from(touchHeat.values()).sort((a, b) => a - b);
    const rank = sortedValues.indexOf(val);
    return (rank + 1) / sortedValues.length;
  }

  let canvas;
  let width = $state(0);
  let height = $state(0);

  let camera = $state({ x: 0, y: 0, zoom: 1 });

  let simNodes = $state([]);
  let simEdges = $state([]);
  let animating = $state(true);
  let selectedNode = $state(null);
  let hoveredNode = $state(null);

  let fileDiff = $state(null);
  let fileSource = $state(null);
  let viewMode = $state('diff'); // 'diff' | 'source'
  let diffFile = $state(null);
  let diffLoading = $state(false);
  let sourceLoading = $state(false);
  let isSaving = $state(false);
  let diffExpanded = $state(false);
  let sidePanelOpen = $state(false);

  async function handleSave() {
    if (!selectedNode || !selectedNode.file || !fileSource) return;
    isSaving = true;
    try {
      await saveFile(selectedNode.file, fileSource);
      console.log('File saved successfully');
    } catch (e) {
      console.error('Failed to save file', e);
    } finally {
      isSaving = false;
    }
  }

  let panelCollapsed = $state(false);
  let searchQuery = $state('');
  let showSignatures = $state(false);

  let dragging = $state(false);
  let dragStart = $state({ x: 0, y: 0 });
  let dragCameraStart = $state({ x: 0, y: 0 });

  let draggedNode = $state(null);
  let dragOffset = $state({ x: 0, y: 0 });

  let rafId = null;
  let simulationIterations = 0;
  const MAX_INITIAL_ITERATIONS = 300;
  const SETTLING_ITERATIONS = 50;

  let diffLines = $derived.by(() => {
    if (!fileDiff) return [];
    return fileDiff.split('\n').filter(line =>
      !line.startsWith('diff --git') &&
      !line.startsWith('index ') &&
      !line.startsWith('--- ') &&
      !line.startsWith('+++ ')
    );
  });

  let visibleDiffLines = $derived(diffExpanded ? diffLines : diffLines.slice(0, 60));

  let splitDiffLines = $derived.by(() => {
    if (!fileDiff) return [];
    
    let leftNum = 0;
    let rightNum = 0;
    let pairs = [];
    
    for (let i = 0; i < diffLines.length; i++) {
      const line = diffLines[i];
      if (line.startsWith('@@')) {
        const match = line.match(/@@ -(\d+),?\d* \+(\d+),?\d* @@/);
        if (match) {
          leftNum = parseInt(match[1], 10);
          rightNum = parseInt(match[2], 10);
        }
        pairs.push({ type: 'meta', leftNum: '', rightNum: '', text: line });
      } else if (line.startsWith('-')) {
        pairs.push({ type: 'del', leftNum: leftNum++, rightNum: '', text: line.substring(1) });
      } else if (line.startsWith('+')) {
        pairs.push({ type: 'add', leftNum: '', rightNum: rightNum++, text: line.substring(1) });
      } else {
        pairs.push({ type: 'context', leftNum: leftNum++, rightNum: rightNum++, text: line.substring(1) });
      }
    }
    
    let aligned = [];
    for (let i = 0; i < pairs.length; i++) {
      if (pairs[i].type === 'del') {
        let delLines = [pairs[i]];
        let j = i + 1;
        while (j < pairs.length && pairs[j].type === 'del') {
          delLines.push(pairs[j]);
          j++;
        }
        
        let addLines = [];
        while (j < pairs.length && pairs[j].type === 'add') {
          addLines.push(pairs[j]);
          j++;
        }
        
        const maxLen = Math.max(delLines.length, addLines.length);
        for (let k = 0; k < maxLen; k++) {
          aligned.push({
            left: delLines[k] || null,
            right: addLines[k] || null
          });
        }
        i = j - 1;
      } else if (pairs[i].type === 'add') {
        let addLines = [pairs[i]];
        let j = i + 1;
        while (j < pairs.length && pairs[j].type === 'add') {
          addLines.push(pairs[j]);
          j++;
        }
        for (let k = 0; k < addLines.length; k++) {
          aligned.push({ left: null, right: addLines[k] });
        }
        i = j - 1;
      } else {
        aligned.push({ left: pairs[i], right: pairs[i] });
      }
    }
    return aligned;
  });

  let diffChunks = $derived.by(() => {
    let chunks = [];
    let currentChunk = null;
    
    for (const line of splitDiffLines) {
      const metaText = line.left?.type === 'meta' ? line.left.text : (line.right?.type === 'meta' ? line.right.text : null);
      
      if (metaText) {
        if (currentChunk) chunks.push(currentChunk);
        
        const contextMatch = metaText.match(/@@[^{]*@@(.*)/);
        let context = contextMatch && contextMatch[1] ? contextMatch[1].trim() : "";
        if (!context) context = metaText.trim();
        
        currentChunk = { context, lines: [] };
      } else {
        if (!currentChunk) {
          currentChunk = { context: "Header", lines: [] };
        }
        currentChunk.lines.push(line);
      }
    }
    if (currentChunk) chunks.push(currentChunk);
    return chunks;
  });

  let expandedChunks = $state(new Set());
  
  $effect(() => {
    if (fileDiff) {
      const newSet = new Set();
      diffChunks.forEach((_, i) => newSet.add(i));
      expandedChunks = newSet;
    }
  });

  function toggleChunk(index) {
    const newSet = new Set(expandedChunks);
    if (newSet.has(index)) newSet.delete(index);
    else newSet.add(index);
    expandedChunks = newSet;
  }

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

  function getNodeSize(node) {
    const lc = node.line_count || 50;
    const base = 40;
    const scale = Math.sqrt(lc / 50);
    return { w: base * scale + 60, h: base * scale * 0.6 + 30 };
  }

  function focusNode(node) {
    camera.x = node.x;
    camera.y = node.y;
    camera.zoom = Math.max(camera.zoom, 1.5);
    selectedNode = node;
    diffExpanded = false;
  }


  let filteredNodes = $derived.by(() => {
    const q = searchQuery.toLowerCase();
    const nodes = simNodes.slice().sort((a, b) => a.id.localeCompare(b.id));
    if (!q) return nodes;
    return nodes.filter(n => n.id.toLowerCase().includes(q));
  });

  function initSimulation() {
    if (!graph.nodes || graph.nodes.length === 0) {
      simNodes = [];
      simEdges = [];
      return;
    }

    const clusters = {};
    for (const node of graph.nodes) {
      const key = getClusterKey(node.id);
      if (!clusters[key]) clusters[key] = [];
      clusters[key].push(node);
    }

    const clusterKeys = Object.keys(clusters);
    const clusterSpread = Math.sqrt(clusterKeys.length) * 1200;

    const clusterCenters = {};
    clusterKeys.forEach((key, i) => {
      const angle = (i / clusterKeys.length) * Math.PI * 2;
      const radius = clusterSpread * 0.5;
      clusterCenters[key] = {
        x: Math.cos(angle) * radius,
        y: Math.sin(angle) * radius,
      };
    });

    simNodes = graph.nodes.map((node) => {
      const center = clusterCenters[getClusterKey(node.id)];
      return {
        ...node,
        x: center.x + (Math.random() - 0.5) * 200,
        y: center.y + (Math.random() - 0.5) * 200,
        vx: 0,
        vy: 0,
        pinned: false,
      };
    });

    simEdges = (graph.edges || []).map(e => ({ ...e }));
    simulationIterations = 0;
    animating = true;
  }

  function stepSimulation() {
    const nodes = simNodes;
    const edges = simEdges;
    if (nodes.length === 0) return;

    const nodeMap = {};
    for (const n of nodes) {
      nodeMap[n.id] = n;
    }

    const repulsionStrength = 35000;
    const springStrength = 0.003;
    const springLength = 450;
    const centerGravity = 0.0008;
    const clusterGravity = 0.015;
    const damping = 0.82;
    const maxVelocity = 20;

    const clusterSums = {};
    for (const node of nodes) {
      const key = getClusterKey(node.id);
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
    const interClusterRepulsion = 150000;
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

        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let dist = Math.sqrt(dx * dx + dy * dy);
        
        if (dist < 1) {
          dx = (Math.random() - 0.5) * 2;
          dy = (Math.random() - 0.5) * 2;
          dist = Math.sqrt(dx * dx + dy * dy);
        }

        const sameCluster = getClusterKey(a.id) === getClusterKey(b.id);
        const rep = sameCluster ? repulsionStrength : repulsionStrength * 1.5;
        let force = rep / (dist * dist);

        const fx = (dx / dist) * force;
        const fy = (dy / dist) * force;

        if (!a.pinned) { a.vx -= fx; a.vy -= fy; }
        if (!b.pinned) { b.vx += fx; b.vy += fy; }

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

      const sameCluster = getClusterKey(source.id) === getClusterKey(target.id);
      const sl = sameCluster ? springLength * 0.6 : springLength;
      const ss = sameCluster ? springStrength * 1.5 : springStrength * 0.3;

      const displacement = dist - sl;
      const force = ss * displacement;
      const fx = (dx / dist) * force;
      const fy = (dy / dist) * force;

      if (!source.pinned) { source.vx += fx; source.vy += fy; }
      if (!target.pinned) { target.vx -= fx; target.vy -= fy; }
    }

    for (const node of nodes) {
      if (node.pinned) continue;

      const key = getClusterKey(node.id);
      const cc = clusterCenters[key];
      const cf = clusterForces[key];

      node.vx -= (node.x - cc.x) * clusterGravity;
      node.vy -= (node.y - cc.y) * clusterGravity;

      node.vx += cf.x;
      node.vy += cf.y;

      node.vx -= node.x * centerGravity;
      node.vy -= node.y * centerGravity;

      node.vx *= damping;
      node.vy *= damping;

      const speed = Math.sqrt(node.vx * node.vx + node.vy * node.vy);
      if (speed > maxVelocity) {
        node.vx = (node.vx / speed) * maxVelocity;
        node.vy = (node.vy / speed) * maxVelocity;
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
    const selectedId = selectedNode ? selectedNode.id : null;
    const highlightId = hoveredId || selectedId;
    const connectedIds = highlightId ? getConnectedNodeIds(highlightId) : new Set();

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

      let edgeHighlighted = false;
      if (highlightId) {
        edgeHighlighted = edge.source === highlightId || edge.target === highlightId;
      }

      ctx.beginPath();
      ctx.moveTo(s.x, s.y);
      ctx.lineTo(t.x, t.y);

      if (highlightId && !edgeHighlighted) {
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

      const isHighlighted = node.id === highlightId;
      const isConnected = connectedIds.has(node.id);
      const isDimmed = highlightId && !isHighlighted && !isConnected;

      const cs = node.change_status;
      const isNeighbor = cs === 'neighbor';

      let fillColor = getNodeColor(node.id);
      let alpha = isDimmed ? 0.15 : isNeighbor ? 0.4 : 1;

      ctx.globalAlpha = alpha;
      
      // Node background
      ctx.fillStyle = fillColor;
      ctx.fillRect(x0, y0, w, h);

      // Heat Highlight (Recently Touched)
      const heat = getNodeHeat(node.file);
      if (heat > 0) {
        ctx.save();
        // Bright Orange Heat Glow
        const heatAlpha = 0.3 + 0.5 * heat;
        ctx.shadowBlur = 15 * heat;
        ctx.shadowColor = `rgba(255, 100, 0, ${heatAlpha})`;
        ctx.strokeStyle = `rgba(255, 140, 0, ${heatAlpha})`;
        ctx.lineWidth = 1 + 3 * heat;
        // Stroke slightly outside the node
        ctx.strokeRect(x0 - 1.5, y0 - 1.5, w + 3, h + 3);
        ctx.restore();
      }

      if ((cs === 'added' || cs === 'modified') && !isDimmed) {
        ctx.strokeStyle = cs === 'added' ? '#4a7c44' : '#b87333';
        ctx.lineWidth = 1;
        ctx.strokeRect(x0, y0, w, h);
      } else if (isHighlighted) {
        ctx.strokeStyle = canvasColors.text;
        ctx.lineWidth = 1;
        ctx.strokeRect(x0, y0, w, h);
      } else if (isConnected) {
        ctx.strokeStyle = canvasColors.text;
        ctx.globalAlpha = 0.5;
        ctx.lineWidth = 1;
        ctx.strokeRect(x0, y0, w, h);
        ctx.globalAlpha = alpha;
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

  function handleWheel(event) {
    event.preventDefault();

    const rect = canvas.getBoundingClientRect();
    const mx = event.clientX - rect.left;
    const my = event.clientY - rect.top;

    const worldBefore = screenToWorld(mx, my);

    const zoomFactor = event.deltaY > 0 ? 0.9 : 1.1;
    camera.zoom = Math.max(0.05, Math.min(10, camera.zoom * zoomFactor));

    const worldAfter = screenToWorld(mx, my);
    camera.x -= worldAfter.x - worldBefore.x;
    camera.y -= worldAfter.y - worldBefore.y;
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
        if (!selectedNode || draggedNode.id !== selectedNode.id) {
          showSignatures = false;
          diffExpanded = false;
        }
        selectedNode = draggedNode;
      }
      draggedNode = null;
      return;
    }
    if (dragging) {
      const dx = event.clientX - dragStart.x;
      const dy = event.clientY - dragStart.y;
      const moved = Math.sqrt(dx * dx + dy * dy);
      if (moved < 5) {
        selectedNode = null;
        showSignatures = false;
        fileDiff = null;
        diffFile = null;
        diffLoading = false;
        diffExpanded = false;
        sidePanelOpen = false;
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
    if (selectedNode && selectedNode.file) {
      if (viewMode === 'diff' && !fileDiff) {
        diffLoading = true;
        diffFile = selectedNode.file;
        getFileDiff(selectedNode.file).then(diff => {
          if (selectedNode && diffFile === selectedNode.file) {
            fileDiff = diff;
            diffLoading = false;
          }
        });
      } else if (viewMode === 'source' && !fileSource) {
        sourceLoading = true;
        diffFile = selectedNode.file;
        getFileSource(selectedNode.file).then(source => {
          if (selectedNode && diffFile === selectedNode.file) {
            fileSource = source;
            sourceLoading = false;
          }
        });
      }
    } else {
      fileDiff = null;
      fileSource = null;
      diffFile = null;
      diffLoading = false;
      sourceLoading = false;
    }
  });

  // Reset when selectedNode changes
  $effect(() => {
    if (selectedNode) {
      fileDiff = null;
      fileSource = null;
    }
  });

  $effect(() => {
    initSimulation();
  });

  $effect(() => {
    const _nodes = simNodes;
    const _edges = simEdges;
    const _w = width;
    const _h = height;
    const _cam = camera;
    const _hov = hoveredNode;
    const _sel = selectedNode;
    const _anim = animating;
    const _drag = draggedNode;
    const _diff = fileDiff;
    const _sigs = showSignatures;
    const _heat = touchHeat;

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
    <button class="eyeloss-nav-panel__toggle" type="button" onclick={() => panelCollapsed = !panelCollapsed}>
      {panelCollapsed ? '>' : '<'} Nodes ({simNodes.length})
    </button>
    {#if !panelCollapsed}
      <div class="eyeloss-nav-panel__search">
        <input
          type="text"
          placeholder="Filter nodes..."
          bind:value={searchQuery}
          class="eyeloss-nav-panel__input"
        />
      </div>
      <ul class="eyeloss-nav-panel__list">
        {#each filteredNodes as node}
          <li>
            <button
              class="eyeloss-nav-panel__item"
              class:eyeloss-nav-panel__item--active={selectedNode && selectedNode.id === node.id}
              type="button"
              onclick={() => focusNode(node)}
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
      <div class="eyeloss-timeline__track">
        {#each [...changes].reverse() as change, i}
          {@const tickHeat = (i + 1) / changes.length}
          <button
            class="eyeloss-timeline__tick"
            class:eyeloss-timeline__tick--active={isTickActive(change, i, changes.length)}
            class:eyeloss-timeline__tick--wc={i === changes.length - 1}
            type="button"
            onclick={(e) => onSelectSince && onSelectSince(change.id, e)}
            style:--tick-heat={tickHeat}
          >
            {#if change.description && change.description.trim() !== ''}
              <div class="eyeloss-timeline__tick-meta">
                <span class="eyeloss-timeline__tick-desc">{change.description}</span>
              </div>
            {/if}
            <span class="eyeloss-timeline__dot"></span>
            <div class="eyeloss-timeline__tick-info">
              <span class="eyeloss-timeline__tick-id">{change.id}</span>
              <span class="eyeloss-timeline__tick-time">
                {change.timestamp.split(' ')[0]}<br/>
                {change.timestamp.split(' ')[1] || ''}
              </span>
            </div>
          </button>
        {/each}
      </div>
    </div>
  {/if}

  {#if selectedNode}
    <Window
      title={getShortName(selectedNode.id)}
      width={Math.min(900, width * 0.8)}
      height={Math.min(800, height * 0.9)}
      x={Math.max(16, width - Math.min(900, width * 0.8) - 16)}
      y={16}
      containerWidth={width}
      containerHeight={height}
      onclose={() => { selectedNode = null; diffExpanded = false; }}
    >
      <div class="eyeloss-node-details__body eyeloss-node-details__body--window" onwheel={(e) => e.stopPropagation()}>
        <div class="eyeloss-node-details__meta" style="margin-bottom: 8px; font-size: 11px; display: flex; justify-content: space-between; align-items: center;">
          <span>{selectedNode.file || 'unknown'}  |  {selectedNode.line_count || '--'} lines</span>
          <div class="tabs" style="display: flex; gap: 4px;">
            <button class="btn {viewMode === 'diff' ? 'btn-primary' : ''}" style="font-size: 0.6rem; padding: 2px 6px;" onclick={() => viewMode = 'diff'}>Diff</button>
            <button class="btn {viewMode === 'source' ? 'btn-primary' : ''}" style="font-size: 0.6rem; padding: 2px 6px;" onclick={() => viewMode = 'source'}>Source</button>
          </div>
        </div>
        
        {#if selectedNode.functions && selectedNode.functions.length > 0}
          <div class="eyeloss-node-details__section-title" onclick={(e) => { e.stopPropagation(); showSignatures = !showSignatures; }} style="cursor: pointer; padding: 4px; background: rgba(255,255,255,0.05);">
            <span>{showSignatures ? 'v' : '>'} Signatures ({selectedNode.functions.length})</span>
          </div>
          {#if showSignatures}
            <ul class="eyeloss-node-details__sigs" style="padding: 8px;">
              {#each selectedNode.functions as fn}
                <li class="eyeloss-node-details__sig">
                  <span class="eyeloss-node-details__sig-type">{fn.kind}</span>
                  <span class="eyeloss-node-details__sig-name">{fn.name}/{fn.arity}</span>
                </li>
              {/each}
            </ul>
          {/if}
        {/if}

        {#if viewMode === 'diff'}
          {#if diffLoading}
            <div class="eyeloss-node-details__meta" style="margin-top: 16px;">Loading diff...</div>
          {:else if fileDiff}
            <div class="eyeloss-node-details__section-title" style="margin-top: 16px; padding-bottom: 4px; border-bottom: 1px solid rgba(255,255,255,0.1);">
              <span>Diff Changes</span>
            </div>
            
            <div class="eyeloss-split-diff" style:filter={`brightness(${1 + getNodeHeat(selectedNode.file) * 0.5})`}>
              {#each diffChunks as chunk, i}
                <div class="diff-chunk-wrapper">
                  <button 
                    class="diff-chunk-header" 
                    type="button" 
                    onclick={() => toggleChunk(i)}
                  >
                    <span class="diff-chunk-icon">{expandedChunks.has(i) ? 'v' : '>'}</span>
                    <span class="diff-chunk-context">{chunk.context}</span>
                  </button>
                  
                  {#if expandedChunks.has(i)}
                    <div class="diff-chunk-content">
                      <div class="split-diff-half">
                        {#each chunk.lines as line}
                          <div class="split-diff-line {line.left ? 'type-' + line.left.type : 'type-empty'}">
                            <span class="split-diff-num">{line.left ? line.left.leftNum : ''}</span>
                            <span class="split-diff-text">{line.left ? line.left.text : ' '}</span>
                          </div>
                        {/each}
                      </div>
                      <div class="split-diff-half">
                        {#each chunk.lines as line}
                          <div class="split-diff-line {line.right ? 'type-' + line.right.type : 'type-empty'}">
                            <span class="split-diff-num">{line.right ? line.right.rightNum : ''}</span>
                            <span class="split-diff-text">{line.right ? line.right.text : ' '}</span>
                          </div>
                        {/each}
                      </div>
                    </div>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        {:else}
          {#if sourceLoading}
            <div class="eyeloss-node-details__meta" style="margin-top: 16px;">Loading source...</div>
          {:else if fileSource !== null}
            <div class="eyeloss-node-details__section-title" style="margin-top: 16px; padding-bottom: 4px; display: flex; justify-content: space-between; align-items: center;">
              <span>Full Source Code</span>
              <button 
                class="btn {isSaving ? 'btn-ghost' : 'btn-primary'}" 
                style="font-size: 0.6rem; padding: 2px 8px;"
                onclick={handleSave}
                disabled={isSaving}
              >
                {isSaving ? 'Saving...' : 'Save Changes'}
              </button>
            </div>
            <Editor bind:value={fileSource} file={selectedNode.file} {theme} />
          {/if}
        {/if}
      </div>
    </Window>
  {/if}
</div>
