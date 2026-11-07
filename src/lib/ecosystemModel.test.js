import { describe, expect, it } from 'vitest';
import {
  buildBugSpecimen,
  buildEcosystem,
  buildOverviewPhenotype,
  buildPathClusterTargets,
  buildPathRegions,
  classifyPathRole,
  evolveBugSpecimen,
  getGraphDetailLevel,
  getPathClusterKey,
  mapAnatomyMetric,
  getSemanticGlyphLod,
} from './ecosystemModel.js';

describe('buildEcosystem', () => {
  it('groups every file exactly once by normalized top-level path', () => {
    const graph = {
      nodes: [
        { id: 'src/a.js', file: './src/a.js', line_count: 20, functions: [{ name: 'a' }] },
        { id: 'lib/b.ex', file: 'lib/b.ex', line_count: 10, functions: [] },
        { id: 'README.md', file: 'README.md', line_count: 5, functions: [] },
      ],
      edges: [{ source: 'src/a.js', target: 'lib/b.ex', type: 'import' }],
    };

    const ecosystem = buildEcosystem(graph);

    expect(ecosystem.fileCount).toBe(3);
    expect(ecosystem.habitats.map((habitat) => habitat.id)).toEqual(['root', 'lib', 'src']);
    expect(ecosystem.habitats.flatMap((habitat) => habitat.organisms).map((item) => item.file)).toEqual([
      'README.md',
      'lib/b.ex',
      'src/a.js',
    ]);
    expect(ecosystem.habitats.find((habitat) => habitat.id === 'src').organisms[0].staticDegree).toBe(1);
  });
});

describe('path-constrained graph geometry', () => {
  const nodes = [
    { id: 'src/lib/a.js', file: 'src/lib/a.js', x: -40, y: 10 },
    { id: 'src/lib/b.js', file: 'src/lib/b.js', x: 30, y: 20 },
    { id: 'src/ui/c.js', file: 'src/ui/c.js', x: 80, y: -20 },
    { id: 'README.md', file: 'README.md', x: 0, y: 90 },
  ];

  it('Given a large file organism, When its folder region is projected, Then the region contains the shared full organ bounds', () => {
    const node = {
      id: 'src/large.rs', file: 'src/large.rs', x: 100, y: 50,
      organismGeometry: { bounds: { minX: -240, maxX: 310, minY: -1800, maxY: 1800 } },
    };
    const [region] = buildPathRegions([node], 1);
    expect(Math.min(...region.polygon.map(point => point.x))).toBeLessThanOrEqual(-140);
    expect(Math.max(...region.polygon.map(point => point.x))).toBeGreaterThanOrEqual(410);
    expect(Math.min(...region.polygon.map(point => point.y))).toBeLessThanOrEqual(-1750);
    expect(Math.max(...region.polygon.map(point => point.y))).toBeGreaterThanOrEqual(1850);
  });

  it('uses the real parent folder as the simulation constraint', () => {
    expect(nodes.map(getPathClusterKey)).toEqual(['src/lib', 'src/lib', 'src/ui', 'root']);
    expect(buildPathClusterTargets(nodes)).toEqual(buildPathClusterTargets(structuredClone(nodes)));
    expect(buildPathClusterTargets(nodes).map((target) => target.id)).toEqual(['root', 'src/lib', 'src/ui']);
  });

  it('keeps dense sibling-folder anchors far enough apart for overview glyphs', () => {
    const dense = Array.from({ length: 24 }, (_, index) => ({
      id: `src/feature_${index}/index.ts`,
      file: `src/feature_${index}/index.ts`,
    }));
    const targets = buildPathClusterTargets(dense);
    let nearest = Infinity;
    for (let left = 0; left < targets.length; left += 1) {
      for (let right = left + 1; right < targets.length; right += 1) {
        nearest = Math.min(nearest, Math.hypot(targets[left].x - targets[right].x, targets[left].y - targets[right].y));
      }
    }
    expect(nearest).toBeGreaterThanOrEqual(95);
  });

  it('builds angular top-level and nested regions from current floating coordinates', () => {
    const repository = buildPathRegions(nodes, 1);
    const folders = buildPathRegions(nodes, Infinity);

    expect(repository.map((region) => region.id)).toEqual(['root', 'src']);
    expect(folders.map((region) => region.id)).toEqual(['root', 'src/lib', 'src/ui']);
    expect(repository.find((region) => region.id === 'src').nodeIds).toEqual([
      'src/lib/a.js',
      'src/lib/b.js',
      'src/ui/c.js',
    ]);
    expect(repository.every((region) => region.polygon.length >= 6)).toBe(true);
  });

  it('reveals detail through zoom without switching graph scenes', () => {
    expect(getGraphDetailLevel(0.4)).toMatchObject({ name: 'repository', regionDepth: 1, showFileLabels: false });
    expect(getGraphDetailLevel(0.9)).toMatchObject({ name: 'files', regionDepth: 2, showFileLabels: true });
    expect(getGraphDetailLevel(1.8)).toMatchObject({ name: 'anatomy', regionDepth: Infinity, showAnatomyMarks: true });
  });
});

describe('universal overview phenotype', () => {
  it('keeps file role as metadata instead of changing callable geometry', () => {
    const config = buildOverviewPhenotype({ file: 'vitest.config.ts', functions: [], line_count: 40 }, 1, 0.4);
    const source = buildOverviewPhenotype({ file: 'src/runtime.ts', functions: [], line_count: 40 }, 1, 0.4);

    expect(config).toMatchObject({ role: 'config', archetype: 'file', language: 'ts' });
    expect(config.roleEvidence).toContain('config');
    expect(source).toMatchObject({ role: 'source', archetype: 'file', language: 'ts' });
  });

  it('classifies test, docs, and data roles from explicit path facts', () => {
    expect(classifyPathRole({ file: 'e2e/graph.test.js' }).role).toBe('test');
    expect(classifyPathRole({ file: 'docs/model.md' }).role).toBe('docs');
    expect(classifyPathRole({ file: 'fixtures/sample.json' }).role).toBe('data');
    expect(classifyPathRole({ file: 'src/lib/model.js' }).role).toBe('source');
  });

  it('preserves the same measured callable anatomy at every map zoom', () => {
    const node = {
      file: 'src/lib/backendBridge.js',
      line_count: 2500,
      anatomy: {
        sourceId: 'snapshot:current', profileId: 'javascript:v1',
        callables: Array.from({ length: 17 }, (_, index) => ({
          id: `callable:${index}`, matchKey: `module:f${index}`, name: `f${index}`,
          kind: 'function', visibility: 'public', arity: 0, startLine: index + 1, endLine: index + 3,
          bodyLineSpan: index + 1, decisionCount: index % 4,
        })),
      },
    };
    const zooms = [0.3, 0.8, 2].map((zoom) => buildOverviewPhenotype(node, 3, zoom));

    expect(zooms.map((phenotype) => phenotype.callables)).toEqual(zooms.map(() => node.anatomy.callables));
    expect(zooms.map((phenotype) => phenotype.capturedCallableCount)).toEqual([17, 17, 17]);
    expect(zooms.map((phenotype) => phenotype.drawnCallableCount)).toEqual([17, 17, 17]);
    expect(zooms.map((phenotype) => phenotype.callables.map((item) => item.bodyLineSpan))).toEqual(
      zooms.map(() => node.anatomy.callables.map((item) => item.bodyLineSpan)),
    );
  });

  it('keeps static degree separate from callable anatomy', () => {
    const phenotype = buildOverviewPhenotype({ file: 'src/router.rb', functions: [] }, 8, 0.8);
    expect(phenotype.staticDegree).toBe(8);
    expect(phenotype.callables).toEqual([]);
    expect(phenotype.drawnCallableCount).toBe(0);
  });

  it('keeps unavailable callable count distinct from a known empty file', () => {
    const unknown = buildOverviewPhenotype({ file: 'src/unknown.ts' }, 0, 1);
    const empty = buildOverviewPhenotype({ file: 'src/empty.ts', functions: [] }, 0, 1);
    expect(unknown.callablesKnown).toBe(false);
    expect(unknown.capturedCallableCount).toBeNull();
    expect(empty.callablesKnown).toBe(true);
    expect(empty.capturedCallableCount).toBe(0);
  });

  it('does not fall back to current functions for an unavailable selected endpoint', () => {
    const unavailable = buildOverviewPhenotype({
      file: 'src/historical.ts',
      functions: [{ name: 'currentOnly', arity: 0 }],
      anatomy: { status: 'unavailable', callables: null, sourceId: 'selected:old', profileId: 'typescript:v1' },
      anatomyEndpointState: 'unavailable',
      anatomyEndpointReason: 'Historical file read failed',
    }, 0, 1);
    const absent = buildOverviewPhenotype({
      file: 'src/removed.ts',
      functions: [{ name: 'currentOnly', arity: 0 }],
      anatomyEndpointState: 'absent',
    }, 0, 1);

    expect(unavailable).toMatchObject({ anatomyStatus: 'unavailable', anatomyReason: 'Historical file read failed', callablesKnown: false, capturedCallableCount: null, callables: [] });
    expect(absent).toMatchObject({ anatomyStatus: 'absent', callablesKnown: false, capturedCallableCount: null, callables: [] });
  });

  it('grows appendages faster than shells while retaining a distant silhouette', () => {
    const far = getSemanticGlyphLod(0.25);
    const near = getSemanticGlyphLod(2);

    expect(far.shellScale).toBeGreaterThanOrEqual(0.7);
    expect(near.organScale / far.organScale).toBeGreaterThan(near.shellScale / far.shellScale);
    expect(far.showRolePattern).toBe(true);
  });
});

describe('buildBugSpecimen', () => {
  it('maps raw measurements monotonically and preserves unknown separately from zero', () => {
    expect(mapAnatomyMetric(null, 'bodyLineSpan')).toBeNull();
    expect(mapAnatomyMetric(undefined, 'decisionCount')).toBeNull();
    expect(mapAnatomyMetric(0, 'decisionCount')).not.toBeNull();
    expect(mapAnatomyMetric(10, 'bodyLineSpan')).toBeGreaterThan(mapAnatomyMetric(2, 'bodyLineSpan'));
    expect(mapAnatomyMetric(4, 'decisionCount')).toBeGreaterThan(mapAnatomyMetric(1, 'decisionCount'));
  });

  it('is deterministic and draws every captured organ without a sample cap', () => {
    const anatomy = {
      status: 'partial',
      file: 'lib/sample.ex',
      evidence: 'tree_sitter',
      callables: Array.from({ length: 9 }, (_, index) => ({
        name: `call_${index}`,
        kind: index % 2 === 0 ? 'def' : 'defp',
        visibility: index % 2 === 0 ? 'public' : 'private',
        arity: null,
      })),
      stateSlots: null,
    };

    const first = buildBugSpecimen(anatomy);
    const second = buildBugSpecimen(structuredClone(anatomy));

    expect(first).toEqual(second);
    expect(first.seed).toBe(second.seed);
    expect(first.drawnCallables).toHaveLength(9);
    expect(first.callables).toHaveLength(9);
    expect(first.exactCallableCount).toBe(9);
    expect(first.stateStatus).toBe('unknown');
  });

  it('does not turn unavailable anatomy into zero facts', () => {
    const specimen = buildBugSpecimen({
      status: 'unavailable',
      file: 'notes.txt',
      evidence: 'tree_sitter',
      callables: [],
      stateSlots: null,
      reason: 'unsupported',
    });

    expect(specimen.status).toBe('unavailable');
    expect(specimen.exactCallableCount).toBe(null);
    expect(specimen.exactStateCount).toBe(null);
  });
});

describe('evolveBugSpecimen', () => {
  it('preserves lineage cosmetics and reports exact structural trait deltas', () => {
    const before = {
      status: 'partial',
      file: './lib/sample.ex',
      callables: [
        { name: 'keep', kind: 'def', visibility: 'public', arity: 1 },
        { name: 'gone', kind: 'defp', visibility: 'private', arity: 0 },
      ],
      stateSlots: [{ name: 'count', kind: 'field', visibility: 'private' }],
    };
    const after = {
      status: 'partial',
      file: 'lib/sample.ex',
      callables: [
        { name: 'keep', kind: 'defp', visibility: 'private', arity: 1 },
        { name: 'fresh', kind: 'def', visibility: 'public', arity: 2 },
      ],
      stateSlots: [{ name: 'total', kind: 'field', visibility: 'private' }],
    };

    const evolution = evolveBugSpecimen(before, after);

    expect(evolution.status).toBe('compared');
    expect(evolution.before.seed).toBe(evolution.after.seed);
    expect(evolution.callables.added.map((item) => item.name)).toEqual(['fresh']);
    expect(evolution.callables.removed.map((item) => item.name)).toEqual(['gone']);
    expect(evolution.callables.changed).toEqual([{ name: 'keep', before: before.callables[0], after: after.callables[0] }]);
    expect(evolution.stateSlots.added.map((item) => item.name)).toEqual(['total']);
    expect(evolution.stateSlots.removed.map((item) => item.name)).toEqual(['count']);
  });

  it('makes no mutation claim without comparable previous anatomy', () => {
    const evolution = evolveBugSpecimen(null, {
      status: 'partial',
      file: 'src/new.ts',
      callables: [{ name: 'run', kind: 'function', visibility: 'unknown', arity: null }],
      stateSlots: null,
    });

    expect(evolution.status).toBe('uncompared');
    expect(evolution.callables).toEqual({ added: [], removed: [], changed: [] });
    expect(evolution.stateSlots).toBe(null);
  });

  it('evolves several lineages independently under one external revision event', () => {
    const base = (file, names) => ({
      status: 'partial',
      file,
      callables: names.map((name) => ({ name, kind: 'function', visibility: 'unknown', arity: null })),
      stateSlots: [],
    });

    const left = evolveBugSpecimen(base('src/a.ts', ['a']), base('src/a.ts', ['a', 'b']));
    const right = evolveBugSpecimen(base('src/b.ts', ['x']), base('src/b.ts', []));

    expect(left.callables.added.map((item) => item.name)).toEqual(['b']);
    expect(right.callables.removed.map((item) => item.name)).toEqual(['x']);
    expect(left.after.seed).not.toBe(right.after.seed);
  });
});
