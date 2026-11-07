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

export function createDefaultReviewTarget() {
  return {
    mode: 'working_copy',
    since: '@',
    base: '',
    head: '',
    includeWorkingCopy: true,
    label: 'Working copy',
  };
}

export function normalizeReviewTarget(reviewTarget, fallback = createDefaultReviewTarget()) {
  const mode = String(reviewTarget?.mode || fallback.mode || 'working_copy').trim() || 'working_copy';
  const includeWorkingCopy =
    reviewTarget?.includeWorkingCopy === undefined
      ? Boolean(fallback.includeWorkingCopy)
      : Boolean(reviewTarget.includeWorkingCopy);

  const normalized = {
    ...fallback,
    ...(reviewTarget || {}),
    mode,
    since: String(reviewTarget?.since ?? fallback.since ?? '@').trim() || '@',
    base: String(reviewTarget?.base ?? fallback.base ?? '').trim(),
    head: String(reviewTarget?.head ?? fallback.head ?? '').trim(),
    includeWorkingCopy,
    label: String(reviewTarget?.label ?? fallback.label ?? '').trim(),
  };

  if (!normalized.label) {
    if (mode === 'range') {
      const baseLabel = normalized.base || '(base)';
      const headLabel = normalized.head || '(head)';
      normalized.label = `${baseLabel}..${headLabel}${normalized.includeWorkingCopy ? ' + wc' : ''}`;
    } else if (mode === 'since') {
      normalized.label = `${normalized.since}${normalized.includeWorkingCopy ? ' + wc' : ''}`;
    } else {
      normalized.label = normalized.includeWorkingCopy ? 'Working copy' : 'Repository';
    }
  }

  return normalized;
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
    initialSelectionPending: true,
    reviewTarget: createDefaultReviewTarget(),
    heatmapData: new Map(),
    heatCounter: 0,
    fileContentVersions: new Map(),
    loadingMore: false,
  };
}

/** Resolve the initial timeline selection once, before asking for any graph. */
export function initializeProjectSelection(project) {
  if (!project.initialSelectionPending) return;
  const latest = project.changes.find(change => change.id && change.id !== '@'
    && !/^\(?working copy\)?$/i.test(String(change.description || '').trim()));
  if (project.since === '@' && project.graphViewMode === 'investigation' && latest) project.since = latest.id;
  project.initialSelectionPending = false;
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
  project.reviewTarget = normalizeReviewTarget(globals.reviewTarget, project.reviewTarget || createDefaultReviewTarget());
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
    reviewTarget: normalizeReviewTarget(project.reviewTarget, createDefaultReviewTarget()),
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
