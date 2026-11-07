const callableList = (anatomy) => Array.isArray(anatomy?.callables) ? anatomy.callables : [];
const keyFor = (callable) => String(callable?.matchKey || '').trim();
const ownerKind = (callable) => {
  const keyParts = keyFor(callable).split('::');
  if (keyParts.length >= 3) return keyParts.slice(0, -1).join('::');
  const owner = String(callable?.owner || '');
  const kind = String(callable?.kind || '');
  return owner || kind ? `${owner}\u0000${kind}` : null;
};
const kindAndName = (callable) => {
  const keyParts = keyFor(callable).split('::');
  if (keyParts.length >= 3) return keyParts.slice(-2).join('::');
  const kind = String(callable?.kind || '');
  const name = String(callable?.name || '');
  return kind && name ? `${kind}\u0000${name}` : null;
};
const measured = (value) => Number.isFinite(value) && value >= 0;

function indexByMatchKey(callables) {
  const index = new Map();
  for (const callable of callables) {
    const key = keyFor(callable);
    if (!key) continue;
    const matches = index.get(key) || [];
    matches.push(callable);
    index.set(key, matches);
  }
  return index;
}

function compatibleDeltas(before, after, profileCompatible) {
  if (!before || !after || !profileCompatible) return null;
  const deltas = {};
  for (const metric of ['bodyLineSpan', 'decisionCount']) {
    const left = before[metric];
    const right = after[metric];
    deltas[metric] = measured(left) && measured(right) ? right - left : null;
  }
  return deltas;
}

function possibleRenameOrMove(callable, candidates) {
  const ownerIdentity = ownerKind(callable);
  const nameIdentity = kindAndName(callable);
  return candidates.some((candidate) =>
    (ownerIdentity !== null && ownerKind(candidate) === ownerIdentity)
    || (nameIdentity !== null && kindAndName(candidate) === nameIdentity)
  );
}

function rowFor(before, after, slotKey, status, profileCompatible) {
  const deltas = compatibleDeltas(before, after, profileCompatible);
  const metricsChanged = deltas && Object.values(deltas).some((delta) => delta !== null && delta !== 0);
  const visibilityChanged = before && after && before.visibility !== after.visibility;
  return {
    slotKey,
    status: status === 'matched' && (metricsChanged || visibilityChanged) ? 'changed' : status,
    before,
    after,
    beforeId: before?.id || null,
    afterId: after?.id || null,
    deltas: status === 'matched' || status === 'changed' ? deltas : null,
  };
}

function endpointState(endpoint, anatomy) {
  if (endpoint?.status === 'pending') return 'pending';
  if (endpoint?.status === 'absent') return 'absent';
  if (endpoint?.status === 'unavailable' || anatomy?.status === 'unavailable') return 'unavailable';
  if (endpoint?.status === 'available' || anatomy) return 'available';
  return 'unavailable';
}

/** Build unique callable correspondences for one already-resolved endpoint pair. */
export function buildBugComparison(beforeInput, afterInput, options = {}) {
  const beforeEndpoint = beforeInput && Object.hasOwn(beforeInput, 'anatomy') ? beforeInput : null;
  const afterEndpoint = afterInput && Object.hasOwn(afterInput, 'anatomy') ? afterInput : null;
  const before = beforeEndpoint ? beforeEndpoint.anatomy : beforeInput;
  const after = afterEndpoint ? afterEndpoint.anatomy : afterInput;
  const hasBaseline = options.hasBaseline ?? beforeInput != null;
  const beforeState = hasBaseline
    ? endpointState(beforeEndpoint, before)
    : 'absent';
  const afterState = endpointState(afterEndpoint, after);

  if (!hasBaseline) {
    return {
      state: 'no_comparison', hasBaseline: false, beforeState: 'absent', afterState,
      beforeEndpoint: beforeEndpoint || { status: 'absent', label: null, anatomy: null, reason: 'No comparison selected' },
      afterEndpoint: afterEndpoint || { status: afterState, label: null, anatomy: after },
      before, after, rows: [], beforeSlotKeys: [], afterSlotKeys: [],
      slotOrder: [], beforeAbsentSlotKeys: [], afterAbsentSlotKeys: [],
      callableChanges: {},
      beforeLabel: null, afterLabel: afterEndpoint?.label || null,
    };
  }

  if (['unavailable', 'pending'].includes(beforeState) || ['unavailable', 'pending'].includes(afterState)) {
    return {
      state: 'unavailable', hasBaseline: true, beforeState, afterState,
      beforeEndpoint: beforeEndpoint || { status: beforeState, label: null, anatomy: before },
      afterEndpoint: afterEndpoint || { status: afterState, label: null, anatomy: after },
      before, after, rows: [], beforeSlotKeys: [], afterSlotKeys: [],
      slotOrder: [], beforeAbsentSlotKeys: [], afterAbsentSlotKeys: [],
      callableChanges: {},
      beforeLabel: beforeEndpoint?.label || null, afterLabel: afterEndpoint?.label || null,
    };
  }

  const beforeCallables = callableList(before);
  const afterCallables = callableList(after);
  const profileCompatible = Boolean(before?.profileId && before.profileId === after?.profileId);
  const beforeIndex = indexByMatchKey(beforeCallables);
  const afterIndex = indexByMatchKey(afterCallables);
  const matchedBefore = new Map();
  const matchedAfter = new Set();
  const ambiguousBefore = new Set();
  const ambiguousAfter = new Set();
  const rows = [];

  const keys = new Set([...beforeIndex.keys(), ...afterIndex.keys()]);
  for (const key of keys) {
    const leftGroup = beforeIndex.get(key) || [];
    const rightGroup = afterIndex.get(key) || [];
    if (leftGroup.length === 1 && rightGroup.length === 1) {
      matchedBefore.set(leftGroup[0], rightGroup[0]);
      matchedAfter.add(rightGroup[0]);
    } else if (leftGroup.length > 1 || rightGroup.length > 1) {
      leftGroup.forEach((callable) => ambiguousBefore.add(callable));
      rightGroup.forEach((callable) => ambiguousAfter.add(callable));
    }
  }

  const unmatchedBefore = beforeCallables.filter((callable) => !matchedBefore.has(callable) && !ambiguousBefore.has(callable));
  const unmatchedAfter = afterCallables.filter((callable) => !matchedAfter.has(callable) && !ambiguousAfter.has(callable));

  for (const callable of beforeCallables) {
    const key = keyFor(callable);
    if (matchedBefore.has(callable)) {
      rows.push(rowFor(callable, matchedBefore.get(callable), key, 'matched', profileCompatible));
      continue;
    }
    if (ambiguousBefore.has(callable)) {
      rows.push(rowFor(callable, null, `${key}#before:${callable.id || rows.length}`, 'unresolved', profileCompatible));
      continue;
    }
    if (possibleRenameOrMove(callable, unmatchedAfter)) {
      rows.push(rowFor(callable, null, `${key || 'unresolved'}#before:${callable.id || rows.length}`, 'unresolved', profileCompatible));
      continue;
    }
    rows.push(rowFor(callable, null, key || `before:${callable.id || rows.length}`, 'removed', profileCompatible));
  }

  for (const callable of afterCallables) {
    if (matchedAfter.has(callable)) continue;
    const key = keyFor(callable);
    if (ambiguousAfter.has(callable)) {
      rows.push(rowFor(null, callable, `${key}#after:${callable.id || rows.length}`, 'unresolved', profileCompatible));
      continue;
    }
    // Ambiguous prior identities still make a same-owner declaration unresolved;
    // excluding them here would incorrectly label a likely rename as an addition.
    const possibleRename = possibleRenameOrMove(callable, [...unmatchedBefore, ...ambiguousBefore]);
    const status = possibleRename ? 'unresolved' : 'added';
    const slotKey = status === 'unresolved'
      ? `${key || 'unresolved'}#after:${callable.id || rows.length}`
      : key || `after:${callable.id || rows.length}`;
    rows.push(rowFor(null, callable, slotKey, status, profileCompatible));
  }

  const alignedBefore = rows.flatMap((row) => row.before ? [row.before] : []);
  const alignedAfter = rows.flatMap((row) => row.after ? [row.after] : []);
  return {
    state: 'ready',
    hasBaseline: true,
    beforeState,
    afterState,
    beforeEndpoint: beforeEndpoint || { status: beforeState, label: null, anatomy: before },
    afterEndpoint: afterEndpoint || { status: afterState, label: null, anatomy: after },
    before: before ? { ...before, callables: alignedBefore } : before,
    after: after ? { ...after, callables: alignedAfter } : after,
    rows,
    beforeSlotKeys: rows.flatMap((row) => row.before ? [row.slotKey] : []),
    afterSlotKeys: rows.flatMap((row) => row.after ? [row.slotKey] : []),
    slotOrder: rows.map((row) => row.slotKey),
    callableChanges: Object.fromEntries(rows.map((row) => [row.slotKey, {
      status: row.status,
      before: row.before || null,
      after: row.after || null,
      beforeProfileId: before?.profileId || null,
      afterProfileId: after?.profileId || null,
    }])),
    beforeAbsentSlotKeys: rows.filter((row) => !row.before && row.after).map((row) => row.slotKey),
    afterAbsentSlotKeys: rows.filter((row) => row.before && !row.after).map((row) => row.slotKey),
    beforeLabel: beforeEndpoint?.label || null,
    afterLabel: afterEndpoint?.label || null,
  };
}
