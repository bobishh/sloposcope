import { describe, expect, it } from 'vitest';
import { buildFileOrganismGeometry, hitTestFileOrganism, hitTestFileOrganismState } from './fileOrganism.ts';
import { buildSharedSpecimenGeometries, buildSingleSpecimenGeometry } from './specimenGeometry.js';

const makeAnatomy = (file, count, overrides = {}) => ({
  status: 'partial', file, language: 'typescript', sourceId: `snapshot:${file}`, profileId: 'typescript:v1',
  callables: Array.from({ length: count }, (_, index) => ({
    id: `${file}:callable:${index}`, matchKey: `Module::function::work_${index}`,
    name: `work_${index}`, displayName: `work_${index}`, kind: 'function', visibility: 'public', arity: 1,
    startLine: index + 1, endLine: index + 3, bodyLineSpan: index + 1, decisionCount: index % 4,
  })),
  stateSlots: [],
  ...overrides,
});

describe('shared specimen geometry adapter', () => {
  it('Given identical Before and After facts at narrow and wide stages, When comparison geometry is built, Then viewport width changes projection only', () => {
    const before = makeAnatomy('src/shared.ts', 8, {
      lineCount: 2638,
      callables: Array.from({ length: 8 }, (_, index) => ({
        id: `same:${index}`, matchKey: `owner::function::callable_${index}`,
        name: index === 0 ? 'aVeryLongCallableIdentifier'.repeat(5) : `callable_${index}`,
        displayName: index === 0 ? 'aVeryLongCallableIdentifier'.repeat(5) : `callable_${index}`,
        kind: 'function', visibility: 'public', arity: 2, bodyLineSpan: index ? 8 : 240, decisionCount: index ? 1 : 24,
      })),
    });
    const after = structuredClone(before);
    const slotOrder = before.callables.map(callable => callable.matchKey);
    const options = {
      slotOrder,
      beforeSlotKeys: slotOrder,
      afterSlotKeys: slotOrder,
      roleFact: 'source',
    };
    const geometries = [1000, 490, 180].map(width =>
      buildSharedSpecimenGeometries(before, after, width, options),
    );
    const mapGeometry = buildFileOrganismGeometry({ anatomy: before, lineCount: before.lineCount, roleFact: 'source' });
    const signature = geometry => ({
      bounds: geometry.bounds,
      body: geometry.body,
      rowPitch: geometry.rowPitch,
      limbs: geometry.limbs.map(({ start, end, length, thickness, dataLayoutY, labelRegion }) => ({ start, end, length, thickness, dataLayoutY, labelRegion })),
    });

    expect(signature(geometries[0].before)).toEqual(signature(geometries[1].before));
    expect(signature(geometries[1].before)).toEqual(signature(geometries[2].before));
    expect(signature(geometries[0].after)).toEqual(signature(mapGeometry));
  });

  it('Given small, medium and large endpoint sizes, When the shared organism engine builds them, Then silhouette width scales and each endpoint retains its own line count', () => {
    const widths = [3, 2638, 9145].map(lineCount => buildFileOrganismGeometry({
      anatomy: makeAnatomy('src/file.ts', 0), lineCount, roleFact: 'source',
    }).body.width);
    expect(widths[0]).toBeLessThan(widths[1]);
    expect(widths[1]).toBeLessThan(widths[2]);

    const oldPayload = buildSingleSpecimenGeometry(makeAnatomy('src/old.ts', 1), { roleFact: 'source' });
    expect(oldPayload.lineCount).toBeNull();
  });

  it.each([300, 1000])('Given %i callables, When shared file geometry is built, Then every callable has one attached hit region and label region inside bounds', count => {
    const geometry = buildSingleSpecimenGeometry(makeAnatomy('src/dense.ts', count, { lineCount: 9145 }), { roleFact: 'source' });
    expect(geometry.limbs).toHaveLength(count);
    expect(geometry.rowCount).toBe(Math.ceil(count / 2));
    expect(geometry.bounds.width).toBeGreaterThan(geometry.body.width);
    for (const limb of geometry.limbs) {
      expect(limb.hitRegion.minX).toBeGreaterThanOrEqual(geometry.bounds.minX);
      expect(limb.hitRegion.maxX).toBeLessThanOrEqual(geometry.bounds.maxX);
      expect(limb.labelRegion.minY).toBeGreaterThanOrEqual(geometry.bounds.minY);
      expect(limb.labelRegion.maxY).toBeLessThanOrEqual(geometry.bounds.maxY);
      expect(geometry.limbs.filter(other => other.id === limb.id)).toHaveLength(1);
    }
  });

  it('Given independent 300 and 1000 callable endpoints, When shared comparison geometry is built, Then endpoint facts and paired attachment rows stay aligned', () => {
    const beforeAnatomy = makeAnatomy('src/test.ts', 300, {
      lineCount: 2638, firstSeenAt: '2020-01-01T00:00:00Z', ageAsOf: '2022-09-27T00:00:00Z', ageDays: 1000, ageReason: null,
      callables: Array.from({ length: 300 }, (_, index) => ({
        id: `before:${index}`, matchKey: `owner::function::work_${index}`,
        name: index === 0 ? 'aVeryLongBeforeCallableName'.repeat(4) : `work_${index}`, displayName: index === 0 ? 'aVeryLongBeforeCallableName'.repeat(4) : `work_${index}`,
        kind: 'function', visibility: 'public', bodyLineSpan: index === 0 ? 300 : 8, decisionCount: index === 0 ? 40 : 1,
      })),
    });
    const afterAnatomy = makeAnatomy('src/test.ts', 1000, {
      lineCount: 9145, firstSeenAt: '2020-01-01T00:00:00Z', ageAsOf: '2026-10-27T00:00:00Z', ageDays: 2491, ageReason: null,
      callables: Array.from({ length: 1000 }, (_, index) => ({
        id: `after:${index}`, matchKey: `owner::function::work_${index}`, name: `work_${index}`, displayName: `work_${index}`,
        kind: 'function', visibility: 'public', bodyLineSpan: index === 0 ? 900 : 8, decisionCount: index === 0 ? 200 : 1,
      })),
    });
    const slotOrder = afterAnatomy.callables.map(callable => callable.matchKey);
    const beforeSlotKeys = beforeAnatomy.callables.map(callable => callable.matchKey);
    const afterSlotKeys = afterAnatomy.callables.map(callable => callable.matchKey);
    const beforeAbsentSlotKeys = slotOrder.filter(key => !beforeSlotKeys.includes(key));
    const aligned = buildSharedSpecimenGeometries(beforeAnatomy, afterAnatomy, 520, {
      slotOrder, beforeSlotKeys, afterSlotKeys, beforeAbsentSlotKeys, afterAbsentSlotKeys: [], roleFact: 'source',
    });

    expect(aligned.before.lineCount).toBe(2638);
    expect(aligned.after.lineCount).toBe(9145);
    expect(aligned.before.firstSeenAt).toBe('2020-01-01T00:00:00Z');
    expect(aligned.after.ageDays).toBe(2491);
    expect(aligned.before.limbs).toHaveLength(300);
    expect(aligned.after.limbs).toHaveLength(1000);
    expect(aligned.before.rowPitch).toBe(aligned.after.rowPitch);
    expect(aligned.before.limbs[0].dataLayoutY).toBe(aligned.after.limbs[0].dataLayoutY);
    expect(aligned.before.limbs[299].dataLayoutY).toBe(aligned.after.limbs[299].dataLayoutY);
    expect(aligned.before.absenceMarkers).toHaveLength(700);
    expect(aligned.before.appearance.shellColor).not.toBe(aligned.after.appearance.shellColor);
  });

  it('Given callable hit regions and stored state spots, When selected, Then shared geometry returns exact identity without merging their channels', () => {
    const geometry = buildSingleSpecimenGeometry(makeAnatomy('src/state.ts', 1, {
      lineCount: 30,
      stateSlots: [
        { name: 'cache', kind: 'field', visibility: 'private', startLine: 4, endLine: 4 },
        { name: 'ready', kind: 'field', visibility: 'public', startLine: 5, endLine: 5 },
      ],
    }), { roleFact: 'source' });
    const callable = geometry.limbs[0];
    const state = geometry.stateSlots[0];
    expect(hitTestFileOrganism(geometry, callable.end)).toBe(callable.id);
    expect(hitTestFileOrganismState(geometry, state)).toBe(state);
    expect(geometry.stateCoverage).toBe('known');
    expect(geometry.stateSlots).toHaveLength(2);
  });

  it('Given unknown callable and state coverage, When a specimen is built, Then it stays distinct from measured empty coverage', () => {
    const unknown = buildSingleSpecimenGeometry({ file: 'src/unknown.ts', status: 'partial', lineCount: null, callables: null, stateSlots: null }, { roleFact: 'source' });
    const empty = buildSingleSpecimenGeometry(makeAnatomy('src/empty.ts', 0, { lineCount: 0, stateSlots: [] }), { roleFact: 'source' });
    expect(unknown.callableCoverage).toBe('unknown');
    expect(unknown.stateCoverage).toBe('unknown');
    expect(unknown.limbs).toHaveLength(0);
    expect(unknown.stateSlots).toHaveLength(0);
    expect(empty.callableCoverage).toBe('known');
    expect(empty.stateCoverage).toBe('known');
    expect(empty.stateNote).toBe('No stored state slots');
  });
});
