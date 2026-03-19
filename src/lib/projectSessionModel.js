export function shortRepoLabel(path) {
  if (!path) return '(unknown repo)';
  const normalized = String(path).replaceAll('\\', '/').replace(/\/+$/, '');
  const parts = normalized.split('/');
  return parts[parts.length - 1] || normalized;
}

export function createDefaultGraphMeta(mode = 'investigation') {
  return {
    mode,
    aggregate: 'file',
    approximateHistorical: false,
    seedLabel: mode === 'investigation' ? 'working_copy' : null,
    nodeProvenance: [],
    canonicalEntityCount: 0,
    canonicalEdgeCount: 0,
    clusteringMode: 'heuristic',
    clusterAssignments: [],
  };
}

export function createProjectSession(path, changesPageSize) {
  const normalizedPath = String(path || '');
  return {
    id: normalizedPath,
    path: normalizedPath,
    label: shortRepoLabel(normalizedPath),
    graph: { nodes: [], edges: [] },
    graphViewMode: 'investigation',
    graphMeta: createDefaultGraphMeta('investigation'),
    changes: [],
    changesLimit: changesPageSize,
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

export function getProjectById(projectTabs, id) {
  if (!id) return null;
  return projectTabs.find((p) => p.id === id) || null;
}

export function getProjectByPath(projectTabs, path) {
  if (!path) return null;
  const normalizedPath = String(path);
  return projectTabs.find((p) => p.path === normalizedPath) || null;
}

export function upsertProject(projectTabs, path, changesPageSize) {
  const existing = getProjectByPath(projectTabs, path);
  if (existing) {
    return { projectTabs, project: existing, created: false };
  }
  const createdProject = createProjectSession(path, changesPageSize);
  return {
    projectTabs: [...projectTabs, createdProject],
    project: createdProject,
    created: true,
  };
}

export function applyProjectGlobals(project, globals) {
  if (!project) return;
  project.path = globals.repoPath;
  project.label = shortRepoLabel(globals.repoPath);
  project.graph = globals.graph;
  project.graphViewMode = globals.graphViewMode || 'investigation';
  project.graphMeta = globals.graphMeta || createDefaultGraphMeta(project.graphViewMode);
  project.changes = globals.changes;
  project.changesLimit = globals.changesLimit;
  project.hasMoreChanges = globals.hasMoreChanges;
  project.bookmarks = globals.bookmarks;
  project.currentBranch = globals.currentBranch;
  project.currentRevision = globals.currentRevision;
  project.since = globals.since || '@';
  project.heatCounter = globals.heatCounter;
  project.heatmapData = new Map(globals.heatmapData);
  project.fileContentVersions = new Map(globals.fileContentVersions);
  project.loadingMore = globals.loadingMore;
}

export function projectToGlobals(project) {
  return {
    repoPath: project.path,
    graph: project.graph,
    graphViewMode: project.graphViewMode || 'investigation',
    graphMeta: project.graphMeta || createDefaultGraphMeta(project.graphViewMode || 'investigation'),
    changes: project.changes,
    changesLimit: project.changesLimit,
    hasMoreChanges: project.hasMoreChanges,
    bookmarks: project.bookmarks,
    currentBranch: project.currentBranch,
    currentRevision: project.currentRevision,
    since: project.since,
    heatCounter: project.heatCounter || 0,
    heatmapData: new Map(project.heatmapData || new Map()),
    fileContentVersions: new Map(project.fileContentVersions || new Map()),
    loadingMore: Boolean(project.loadingMore),
  };
}

export function uniqueChanges(list) {
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
