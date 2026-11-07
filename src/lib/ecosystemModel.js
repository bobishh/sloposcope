const SHELL_PALETTES = [
  ['#ff6b35', '#ffd166', '#3a0f2b'],
  ['#7bdff2', '#b2f7ef', '#17324d'],
  ['#c77dff', '#80ffdb', '#32174d'],
  ['#f15bb5', '#fee440', '#24102f'],
  ['#90be6d', '#f9844a', '#162a20'],
];

export function normalizeRepositoryPath(value) {
  return String(value || '')
    .replaceAll('\\', '/')
    .replace(/^\.\/+/, '')
    .replace(/^\/+/, '')
    .replace(/\/{2,}/g, '/');
}

const CONFIG_BASENAMES = new Set([
  'cargo.toml',
  'deno.json',
  'deno.jsonc',
  'elm.json',
  'eslint.config.js',
  'eslint.config.mjs',
  'jsconfig.json',
  'mix.exs',
  'package.json',
  'tsconfig.json',
  'wrangler.json',
  'wrangler.jsonc',
  'wrangler.toml',
]);
const DOC_EXTENSIONS = new Set(['md', 'mdx', 'rst', 'adoc', 'txt']);
const DATA_EXTENSIONS = new Set([
  'csv', 'gif', 'icns', 'ico', 'jpeg', 'jpg', 'json', 'lock', 'png', 'svg', 'toml', 'tsv', 'webp', 'xml', 'yaml', 'yml',
]);

function fileExtension(file) {
  const base = normalizeRepositoryPath(file).split('/').pop() || '';
  const index = base.lastIndexOf('.');
  return index > 0 && index < base.length - 1 ? base.slice(index + 1).toLowerCase() : 'file';
}

export function classifyPathRole(node) {
  const file = normalizeRepositoryPath(node?.file || node?.id);
  const lower = file.toLowerCase();
  const parts = lower.split('/').filter(Boolean);
  const base = parts.at(-1) || '';
  const extension = fileExtension(file);

  if (base.includes('.config.') || CONFIG_BASENAMES.has(base) || base.startsWith('.eslintrc') || base.startsWith('.prettierrc')) {
    return { role: 'config', evidence: `path:config:${base}` };
  }
  if (
    parts.some((part) => ['e2e', 'spec', 'specs', 'test', 'tests'].includes(part)) ||
    /(?:^|[._-])(test|spec)\.[^.]+$/.test(base) ||
    /_(test|spec)\.[^.]+$/.test(base)
  ) {
    return { role: 'test', evidence: `path:test:${file}` };
  }
  if (parts.some((part) => ['doc', 'docs'].includes(part)) || DOC_EXTENSIONS.has(extension)) {
    return { role: 'docs', evidence: `path:docs:${extension}` };
  }
  if (DATA_EXTENSIONS.has(extension)) {
    return { role: 'data', evidence: `extension:data:${extension}` };
  }
  return { role: 'source', evidence: `path:source:${extension}` };
}

export function getSemanticGlyphLod(zoom) {
  const value = Math.max(0.05, Number(zoom || 0));
  const progress = Math.max(0, Math.min(1, (value - 0.2) / 1.8));
  return {
    shellScale: 0.72 + 0.32 * Math.pow(progress, 0.35),
    organScale: 0.3 + 1.15 * Math.pow(progress, 0.85),
    showRolePattern: true,
  };
}

export const ANATOMY_METRIC_MAPPING_LABEL = 'Function length and branching use shared logarithmic scales; inspect raw values.';

/** Return a shared display dimension in pixels, or null when the measure is unknown. */
export function mapAnatomyMetric(value, metric) {
  if (value == null || !Number.isFinite(Number(value)) || Number(value) < 0) return null;
  const raw = Number(value);
  if (metric === 'bodyLineSpan') return 8 + 4 * Math.log2(raw + 1);
  if (metric === 'decisionCount') return 2 + 1.5 * Math.log2(raw + 1);
  throw new TypeError(`Unknown anatomy metric: ${metric}`);
}

export function buildOverviewPhenotype(node, staticDegree = 0, zoom = 1) {
  const roleFact = classifyPathRole(node);
  const anatomyProvided = node?.anatomy != null || node?.anatomyEndpointState != null;
  const anatomy = anatomyProvided ? node.anatomy : null;
  const endpointStateUnavailable = ['absent', 'unavailable'].includes(String(node?.anatomyEndpointState || '').toLowerCase());
  const anatomyCallablesKnown = Array.isArray(anatomy?.callables)
    && anatomy?.status !== 'unavailable'
    && !endpointStateUnavailable;
  const fallbackFunctionsKnown = !anatomyProvided && Array.isArray(node?.functions);
  const functions = anatomyCallablesKnown
    ? anatomy.callables
    : fallbackFunctionsKnown ? node.functions.map((callable) => ({
      ...callable,
      bodyLineSpan: null,
      decisionCount: null,
    })) : [];
  const capturedCallableCount = functions.length;
  const degree = Math.max(0, Number(staticDegree || 0));
  const lod = getSemanticGlyphLod(zoom);

  return {
    role: roleFact.role,
    roleEvidence: roleFact.evidence,
    archetype: 'file',
    language: fileExtension(node?.file || node?.id),
    lineCount: Math.max(0, Number(node?.line_count || 0)),
    staticDegree: degree,
    anatomyStatus: anatomy?.status || node?.anatomyEndpointState || (fallbackFunctionsKnown ? 'partial' : 'unavailable'),
    anatomyReason: node?.anatomyEndpointReason || anatomy?.reason || null,
    sourceId: anatomy?.sourceId || null,
    profileId: anatomy?.profileId || null,
    callables: functions,
    callablesKnown: Boolean(anatomyCallablesKnown || fallbackFunctionsKnown),
    capturedCallableCount: anatomyCallablesKnown || fallbackFunctionsKnown ? capturedCallableCount : null,
    drawnCallableCount: capturedCallableCount,
    ...lod,
  };
}

function hashString(value) {
  let hash = 2166136261;
  for (const character of String(value || '')) {
    hash ^= character.charCodeAt(0);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
}

function habitatId(file) {
  const parts = normalizeRepositoryPath(file).split('/').filter(Boolean);
  return parts.length > 1 ? parts[0] : 'root';
}

function fileDirectory(file) {
  const parts = normalizeRepositoryPath(file).split('/').filter(Boolean);
  return parts.length > 1 ? parts.slice(0, -1) : [];
}

export function getPathClusterKey(node) {
  const directory = fileDirectory(node?.file || node?.id);
  return directory.length ? directory.join('/') : 'root';
}

function pathRegionKey(node, depth) {
  const directory = fileDirectory(node?.file || node?.id);
  if (!directory.length) return 'root';
  const limit = Number.isFinite(depth) ? Math.max(1, depth) : directory.length;
  return directory.slice(0, limit).join('/');
}

export function buildPathClusterTargets(nodes = []) {
  const foldersByTopLevel = new Map();
  for (const node of nodes) {
    const id = getPathClusterKey(node);
    const topLevel = id === 'root' ? 'root' : id.split('/')[0];
    if (!foldersByTopLevel.has(topLevel)) foldersByTopLevel.set(topLevel, new Set());
    foldersByTopLevel.get(topLevel).add(id);
  }

  const topLevels = [...foldersByTopLevel.keys()].sort((left, right) => {
    if (left === 'root') return -1;
    if (right === 'root') return 1;
    return left.localeCompare(right);
  });
  const maxFolderCount = Math.max(1, ...[...foldersByTopLevel.values()].map((folders) => folders.size));
  const maxFolderRadius = maxFolderCount <= 1 ? 0 : Math.min(620, 90 + maxFolderCount * 18);
  const topRadius = topLevels.length <= 1
    ? 0
    : Math.min(1900, 360 + topLevels.length * 120 + maxFolderRadius);
  const targets = [];
  topLevels.forEach((topLevel, topIndex) => {
    const topAngle = topLevels.length <= 1 ? 0 : (topIndex / topLevels.length) * Math.PI * 2 - Math.PI / 2;
    const topX = Math.cos(topAngle) * topRadius;
    const topY = Math.sin(topAngle) * topRadius;
    const folders = [...foldersByTopLevel.get(topLevel)].sort();
    const folderRadius = folders.length <= 1 ? 0 : Math.min(620, 90 + folders.length * 18);
    folders.forEach((id, folderIndex) => {
      const angle = folders.length <= 1 ? 0 : (folderIndex / folders.length) * Math.PI * 2 - Math.PI / 2;
      targets.push({
        id,
        topLevel,
        x: topX + Math.cos(angle) * folderRadius,
        y: topY + Math.sin(angle) * folderRadius,
        boundaryRadius: 115,
      });
    });
  });
  return targets.sort((left, right) => {
    if (left.id === 'root') return -1;
    if (right.id === 'root') return 1;
    return left.id.localeCompare(right.id);
  });
}

function cross(origin, left, right) {
  return (left.x - origin.x) * (right.y - origin.y) - (left.y - origin.y) * (right.x - origin.x);
}

function convexHull(points) {
  const sorted = [...points].sort((left, right) => left.x - right.x || left.y - right.y);
  if (sorted.length <= 2) return sorted;
  const lower = [];
  for (const point of sorted) {
    while (lower.length >= 2 && cross(lower.at(-2), lower.at(-1), point) <= 0) lower.pop();
    lower.push(point);
  }
  const upper = [];
  for (let index = sorted.length - 1; index >= 0; index -= 1) {
    const point = sorted[index];
    while (upper.length >= 2 && cross(upper.at(-2), upper.at(-1), point) <= 0) upper.pop();
    upper.push(point);
  }
  lower.pop();
  upper.pop();
  return lower.concat(upper);
}

export function buildPathRegions(nodes = [], depth = 1) {
  const grouped = new Map();
  for (const node of nodes) {
    const id = pathRegionKey(node, depth);
    if (!grouped.has(id)) grouped.set(id, []);
    grouped.get(id).push(node);
  }

  return [...grouped.entries()]
    .sort(([left], [right]) => {
      if (left === 'root') return -1;
      if (right === 'root') return 1;
      return left.localeCompare(right);
    })
    .map(([id, members]) => {
      const footprint = [];
      for (const node of members) {
        const x = Number(node?.x || 0);
        const y = Number(node?.y || 0);
        const bounds = node.organismGeometry?.bounds;
        if (bounds) {
          const padding = 24;
          footprint.push(
            { x: x + bounds.minX - padding, y: y + bounds.minY - padding },
            { x: x + bounds.maxX + padding, y: y + bounds.minY - padding },
            { x: x + bounds.maxX + padding, y: y + bounds.maxY + padding },
            { x: x + bounds.minX - padding, y: y + bounds.maxY + padding },
          );
          continue;
        }
        const rx = 76;
        const ry = 62;
        footprint.push(
          { x: x - rx, y: y - ry * 0.45 },
          { x: x - rx * 0.55, y: y - ry },
          { x: x + rx * 0.55, y: y - ry },
          { x: x + rx, y: y - ry * 0.45 },
          { x: x + rx, y: y + ry * 0.45 },
          { x: x + rx * 0.55, y: y + ry },
          { x: x - rx * 0.55, y: y + ry },
          { x: x - rx, y: y + ry * 0.45 },
        );
      }
      const polygon = convexHull(footprint);
      const nodeIds = members.map((node) => node.id).sort();
      return { id, nodeIds, polygon };
    });
}

export function getGraphDetailLevel(zoom) {
  const value = Number(zoom || 0);
  if (value < 0.55) {
    return { name: 'repository', regionDepth: 1, showFileLabels: false, showAnatomyMarks: false };
  }
  if (value < 1.35) {
    return { name: 'files', regionDepth: 2, showFileLabels: true, showAnatomyMarks: false };
  }
  return { name: 'anatomy', regionDepth: Infinity, showFileLabels: true, showAnatomyMarks: true };
}

export function buildEcosystem(graph = { nodes: [], edges: [] }) {
  const nodes = Array.isArray(graph?.nodes) ? graph.nodes : [];
  const edges = Array.isArray(graph?.edges) ? graph.edges : [];
  const degreeById = new Map();
  for (const edge of edges) {
    degreeById.set(edge.source, (degreeById.get(edge.source) || 0) + 1);
    degreeById.set(edge.target, (degreeById.get(edge.target) || 0) + 1);
  }

  const habitats = new Map();
  const seenFiles = new Set();
  for (const node of nodes) {
    const file = normalizeRepositoryPath(node?.file || node?.id);
    if (!file || seenFiles.has(file)) continue;
    seenFiles.add(file);
    const id = habitatId(file);
    if (!habitats.has(id)) {
      habitats.set(id, { id, organisms: [], lineCount: 0, callableCount: 0, staticLinkCount: 0 });
    }
    const habitat = habitats.get(id);
    const callableCount = Array.isArray(node?.functions) ? node.functions.length : 0;
    const lineCount = Number(node?.line_count || 0);
    const staticDegree = Number(degreeById.get(node.id) || 0);
    habitat.organisms.push({
      id: node.id,
      file,
      label: String(node?.label || node?.id || file),
      lineCount,
      callableCount,
      staticDegree,
      changeStatus: String(node?.change_status || 'unchanged'),
      node,
    });
    habitat.lineCount += lineCount;
    habitat.callableCount += callableCount;
    habitat.staticLinkCount += staticDegree;
  }

  const orderedHabitats = [...habitats.values()]
    .map((habitat) => ({
      ...habitat,
      organisms: habitat.organisms.sort((left, right) => left.file.localeCompare(right.file)),
    }))
    .sort((left, right) => {
      if (left.id === 'root') return -1;
      if (right.id === 'root') return 1;
      return left.id.localeCompare(right.id);
    });

  return {
    habitats: orderedHabitats,
    fileCount: seenFiles.size,
    staticRelationCount: edges.length,
    lineCount: orderedHabitats.reduce((total, habitat) => total + habitat.lineCount, 0),
  };
}

export function buildBugSpecimen(anatomy) {
  const status = String(anatomy?.status || 'unavailable');
  const file = normalizeRepositoryPath(anatomy?.file);
  const seed = hashString(file || 'unknown-file');
  const palette = SHELL_PALETTES[seed % SHELL_PALETTES.length];
  if (status === 'unavailable') {
    return {
      status,
      file,
      sourceId: anatomy?.sourceId || null,
      profileId: anatomy?.profileId || null,
      seed,
      palette,
      reason: anatomy?.reason || 'Anatomy unavailable',
      callables: null,
      stateSlots: null,
      exactCallableCount: null,
      exactStateCount: null,
      stateStatus: 'unknown',
      drawnCallables: [],
      drawnStateSlots: [],
    };
  }

  const callables = Array.isArray(anatomy?.callables) ? anatomy.callables.map((item) => ({ ...item })) : [];
  const stateKnown = Array.isArray(anatomy?.stateSlots);
  const stateSlots = stateKnown ? anatomy.stateSlots.map((item) => ({ ...item })) : null;
  return {
    status,
    file,
    sourceId: anatomy?.sourceId || null,
    profileId: anatomy?.profileId || null,
    seed,
    palette,
    reason: anatomy?.reason || null,
    evidence: anatomy?.evidence || 'unknown',
    callables,
    stateSlots,
    exactCallableCount: callables.length,
    exactStateCount: stateKnown ? stateSlots.length : null,
    stateStatus: stateKnown ? 'known' : 'unknown',
    exposedCallables: callables.filter((item) => item.visibility === 'public'),
    localCallables: callables.filter((item) => item.visibility === 'private' || item.visibility === 'protected'),
    unknownCallables: callables.filter((item) => !['public', 'private', 'protected'].includes(item.visibility)),
    drawnCallables: callables,
    drawnStateSlots: stateKnown ? stateSlots : [],
  };
}

function indexedTraits(items) {
  const occurrences = new Map();
  return (items || []).map((item) => {
    const name = String(item?.name || '');
    const occurrence = occurrences.get(name) || 0;
    occurrences.set(name, occurrence + 1);
    return { key: `${name}\u0000${occurrence}`, item };
  });
}

function traitChanged(before, after, fields) {
  return fields.some((field) => (before?.[field] ?? null) !== (after?.[field] ?? null));
}

function diffTraits(beforeItems, afterItems, fields) {
  const before = indexedTraits(beforeItems);
  const after = indexedTraits(afterItems);
  const beforeByKey = new Map(before.map((entry) => [entry.key, entry.item]));
  const afterByKey = new Map(after.map((entry) => [entry.key, entry.item]));
  return {
    added: after.filter((entry) => !beforeByKey.has(entry.key)).map((entry) => entry.item),
    removed: before.filter((entry) => !afterByKey.has(entry.key)).map((entry) => entry.item),
    changed: after
      .filter((entry) => beforeByKey.has(entry.key) && traitChanged(beforeByKey.get(entry.key), entry.item, fields))
      .map((entry) => ({ name: entry.item.name, before: beforeByKey.get(entry.key), after: entry.item })),
  };
}

export function evolveBugSpecimen(beforeAnatomy, afterAnatomy) {
  const before = beforeAnatomy ? buildBugSpecimen(beforeAnatomy) : null;
  const after = afterAnatomy ? buildBugSpecimen(afterAnatomy) : null;
  const sameLineage = before?.file && after?.file && before.file === after.file;
  const comparable = sameLineage && before.status !== 'unavailable' && after.status !== 'unavailable';
  if (!comparable) {
    return {
      status: 'uncompared',
      before,
      after,
      callables: { added: [], removed: [], changed: [] },
      stateSlots: null,
    };
  }

  const stateComparable = Array.isArray(before.stateSlots) && Array.isArray(after.stateSlots);
  return {
    status: 'compared',
    before,
    after,
    callables: diffTraits(before.callables, after.callables, [
      'kind',
      'visibility',
      'arity',
      'startLine',
      'endLine',
    ]),
    stateSlots: stateComparable
      ? diffTraits(before.stateSlots, after.stateSlots, ['kind', 'visibility', 'startLine', 'endLine'])
      : null,
  };
}
