import { describe, expect, it } from 'vitest';
import {
  buildFileOrganismGeometry,
  createFileOrganismBodyPath,
  hitTestFileOrganism,
  hitTestFileOrganismState,
  paintFileOrganism,
} from './fileOrganism.ts';

function anatomy(path: string, count: number, bodyLineSpan: number | null = 8, decisionCount: number | null = 0) {
  return {
    file: path,
    language: 'rust',
    sourceId: `snapshot:${path}`,
    profileId: 'rust:callable-anatomy-v1',
    status: 'partial',
    firstSeenAt: '2020-01-01T00:00:00Z',
    ageAsOf: '2024-01-01T00:00:00Z',
    ageDays: 1460,
    ageReason: null,
    callables: Array.from({ length: count }, (_, index) => ({
      id: `${path}::callable-${index}`,
      matchKey: `Fixture::function::callable-${index}`,
      name: `callable_${index}`,
      kind: 'function',
      visibility: index % 3 === 0 ? 'private' : 'public',
      arity: 1,
      startLine: index + 1,
      endLine: index + 8,
      bodyLineSpan,
      decisionCount,
    })),
  };
}

const inside = (bounds: { minX: number; minY: number; maxX: number; maxY: number }, point: { x: number; y: number }) =>
  point.x >= bounds.minX && point.x <= bounds.maxX && point.y >= bounds.minY && point.y <= bounds.maxY;

describe('measured file organism geometry', () => {
  it('Given three measured file sizes, When geometry is built, Then body width grows monotonically with physical line count', () => {
    const widths = [3, 2638, 9145].map(lineCount => buildFileOrganismGeometry({ anatomy: anatomy('src/file.rs', 0), lineCount, roleFact: 'source' }).body.width);
    expect(widths[0]).toBeLessThan(widths[1]);
    expect(widths[1]).toBeLessThan(widths[2]);
  });

  it.each([300, 1000])('Given %i callables, When full geometry is built, Then every callable gets one separated attached limb inside complete bounds', count => {
    const geometry = buildFileOrganismGeometry({ anatomy: anatomy('src/dense.rs', count), lineCount: 9145, roleFact: 'source' });
    expect(geometry.limbs).toHaveLength(count);
    expect(geometry.rowCount).toBe(Math.ceil(count / 2));
    expect(geometry.bounds.width).toBeGreaterThan(geometry.body.width);
    let lastRowY = -Infinity;
    for (let index = 0; index < geometry.limbs.length; index += 1) {
      const limb = geometry.limbs[index];
      if (limb.row !== geometry.limbs[index - 1]?.row) {
        expect(limb.start.y).toBeGreaterThan(lastRowY);
        lastRowY = limb.start.y;
      }
      expect(limb.row).toBe(Math.floor(index / 2));
      expect(limb.start.y).toBe(limb.end.y);
      expect(Math.hypot(limb.start.x, limb.start.y)).toBeGreaterThan(0);
      expect(inside(geometry.bounds, limb.start)).toBe(true);
      expect(inside(geometry.bounds, limb.end)).toBe(true);
      expect(inside(geometry.bounds, { x: limb.hitRegion.minX, y: limb.hitRegion.minY })).toBe(true);
      expect(inside(geometry.bounds, { x: limb.hitRegion.maxX, y: limb.hitRegion.maxY })).toBe(true);
      const bodyX = Math.abs(limb.start.x);
      const normalizedY = (limb.start.y - geometry.body.height / 2) / (geometry.body.height / 2);
      const ellipseX = geometry.body.width / 2 * Math.sqrt(Math.max(0, 1 - normalizedY * normalizedY));
      expect(Math.abs(bodyX - ellipseX)).toBeLessThan(0.001);
      if (index >= 2 && index % 2 === 0) {
        const previousRow = geometry.limbs[index - 2];
        const clearance = Math.abs(limb.start.y - previousRow.start.y);
        expect(clearance).toBeGreaterThanOrEqual(geometry.rowPitch);
      }
    }
  });

  it('Given an odd callable count and aligned comparison slots, When both sides are built, Then no limb is mirrored and shared keys keep identical attachment rows', () => {
    const beforeAnatomy = anatomy('src/example.rs', 3);
    const afterAnatomy = anatomy('src/example.rs', 2);
    const slotOrder = ['a', 'b', 'c'];
    const before = buildFileOrganismGeometry({ anatomy: beforeAnatomy, lineCount: 40, roleFact: 'source' }, { slotOrder, callableSlotKeys: ['a', 'b', 'c'] });
    const after = buildFileOrganismGeometry({ anatomy: afterAnatomy, lineCount: 40, roleFact: 'source' }, { slotOrder, callableSlotKeys: ['a', 'b'], absenceSlotKeys: ['c'] });
    expect(before.limbs).toHaveLength(3);
    expect(before.limbs[2].side).toBe(-1);
    expect(after.limbs).toHaveLength(2);
    expect(after.absenceMarkers).toHaveLength(1);
    expect(after.limbs[0].start.y).toBe(before.limbs[0].start.y);
    expect(after.limbs[1].start.y).toBe(before.limbs[1].start.y);
    expect(after.absenceMarkers[0].row).toBe(before.limbs[2].row);
  });

  it('Given different endpoint callable counts and a reserved trailing slot, When geometries share slot order and pitch, Then paired rows stay aligned and body bounds reserve the union', () => {
    const before = buildFileOrganismGeometry({ anatomy: anatomy('src/shared.rs', 3), lineCount: 900, roleFact: 'source' }, {
      slotOrder: ['shared-0', 'shared-1', 'old-tail'], callableSlotKeys: ['shared-0', 'shared-1', 'old-tail'], rowPitch: 72,
    });
    const after = buildFileOrganismGeometry({ anatomy: anatomy('src/shared.rs', 2), lineCount: 1200, roleFact: 'source' }, {
      slotOrder: ['shared-0', 'shared-1', 'old-tail'], callableSlotKeys: ['shared-0', 'shared-1'], absenceSlotKeys: ['old-tail'], rowPitch: 72,
    });
    expect(after.limbs[0].dataLayoutY).toBe(before.limbs[0].dataLayoutY);
    expect(after.limbs[1].dataLayoutY).toBe(before.limbs[1].dataLayoutY);
    expect(after.absenceMarkers[0].y).toBe(before.limbs[2].dataLayoutY);
    expect(after.rowCount).toBe(before.rowCount);
  });

  it('Given known-zero and unknown measurements, When the same builder runs, Then raw and visual dimensions distinguish them', () => {
    const zero = buildFileOrganismGeometry({ anatomy: anatomy('src/zero.rs', 1, 0, 0), lineCount: 0, roleFact: 'source' });
    const unknown = buildFileOrganismGeometry({ anatomy: anatomy('src/unknown.rs', 1, null, null), lineCount: null, roleFact: 'source' });
    expect(zero.limbs[0].bodyLineSpan).toBe(0);
    expect(unknown.limbs[0].bodyLineSpan).toBeNull();
    expect(zero.limbs[0].length).toBeLessThan(unknown.limbs[0].length);
    expect(zero.limbs[0].decisionCount).toBe(0);
    expect(unknown.limbs[0].decisionCount).toBeNull();
    expect(zero.limbs[0].thickness).not.toBe(unknown.limbs[0].thickness);
    expect(hitTestFileOrganism(unknown, { x: unknown.limbs[0].end.x, y: unknown.limbs[0].end.y })).toBe(unknown.limbs[0].id);

    const unavailable = buildFileOrganismGeometry({ anatomy: { ...anatomy('src/unavailable.rs', 0), status: 'unavailable' }, lineCount: 12, roleFact: 'source' });
    expect(unavailable.callableCoverage).toBe('unknown');
    expect(unavailable.limbs).toHaveLength(0);
  });

  it('Given endpoint age and file roles, When geometry is built, Then role contour and age shell remain independent and unknown age stays distinct', () => {
    const oldSource = buildFileOrganismGeometry({ anatomy: anatomy('src/a.rs', 1), lineCount: 20, roleFact: 'source' });
    const testFile = buildFileOrganismGeometry({ anatomy: anatomy('test/a.rs', 1), lineCount: 20, roleFact: 'test' });
    const unknownAgeAnatomy = { ...anatomy('src/a.rs', 1), ageDays: null, ageReason: 'untracked' };
    const unknownAge = buildFileOrganismGeometry({ anatomy: unknownAgeAnatomy, lineCount: 20, roleFact: 'source' });
    expect(oldSource.body.shape).not.toBe(testFile.body.shape);
    expect(oldSource.appearance.shellColor).not.toBe(unknownAge.appearance.shellColor);
    expect(oldSource.appearance.shellColor).toBe(buildFileOrganismGeometry({ anatomy: anatomy('src/other.rs', 1), lineCount: 20, roleFact: 'source' }).appearance.shellColor);
    expect(oldSource.ageDays).toBe(1460);
    expect(unknownAge.ageDays).toBeNull();
    expect(unknownAge.ageReason).toBe('untracked');

    const youngAnatomy = { ...anatomy('src/a.rs', 1), ageDays: 10 };
    const youngSource = buildFileOrganismGeometry({ anatomy: youngAnatomy, lineCount: 20, roleFact: 'source' });
    expect(youngSource.appearance.bodyColor).not.toBe(oldSource.appearance.bodyColor);
    expect(youngSource.body.width).toBe(oldSource.body.width);
    expect(youngSource.rowPitch).toBe(oldSource.rowPitch);
    expect(youngSource.limbs.map(limb => [limb.id, limb.start, limb.end, limb.thickness])).toEqual(oldSource.limbs.map(limb => [limb.id, limb.start, limb.end, limb.thickness]));
    expect(unknownAge.appearance.bodyColor).not.toBe(oldSource.appearance.bodyColor);
  });

  it('Given angular, leaf, and rounded role shapes, When callable roots are generated, Then every limb attaches to its actual body contour', () => {
    const config = buildFileOrganismGeometry({ anatomy: anatomy('config.toml', 4), lineCount: 90, roleFact: 'config' });
    const docs = buildFileOrganismGeometry({ anatomy: anatomy('README.md', 4), lineCount: 90, roleFact: 'docs' });
    const data = buildFileOrganismGeometry({ anatomy: anatomy('values.json', 4), lineCount: 90, roleFact: 'data' });
    expect(config.body.shape).toBe('angular');
    expect(docs.body.shape).toBe('leaf');
    expect(data.body.shape).toBe('round');
    for (const geometry of [config, docs, data]) {
      const { body } = geometry;
      const y = geometry.limbs[0].start.y;
      const top = body.y;
      const height = body.height;
      const width = body.width;
      let expectedHalfWidth: number;
      if (body.shape === 'angular') {
        const cutY = Math.min(height * 0.12, 18);
        const cutX = Math.min(width * 0.18, 18);
        expectedHalfWidth = y < top + cutY ? width / 2 - cutX * (1 - (y - top) / cutY) : width / 2;
      } else if (body.shape === 'leaf') {
        const t = (y - top) / height;
        expectedHalfWidth = 2 * width * t * (1 - t);
      } else {
        const radius = Math.min(width, height) / 2;
        const cornerCenterY = y < top + radius ? top + radius : top + height - radius;
        expectedHalfWidth = y >= top + radius && y <= top + height - radius
          ? width / 2
          : width / 2 - radius + Math.sqrt(radius * radius - (y - cornerCenterY) ** 2);
      }
      expect(Math.abs(Math.abs(geometry.limbs[0].start.x) - expectedHalfWidth)).toBeLessThan(0.001);
    }
  });

  it('Given known, empty, and unknown state capture, When geometry is built, Then named body spots and absence states stay distinct', () => {
    const states = Array.from({ length: 15 }, (_, index) => ({ name: `state_${index}`, kind: 'field', visibility: 'private', startLine: index + 1, endLine: index + 1 }));
    const known = buildFileOrganismGeometry({ anatomy: { ...anatomy('src/state.rs', 1), stateSlots: states }, lineCount: 80, roleFact: 'source' });
    const empty = buildFileOrganismGeometry({ anatomy: { ...anatomy('src/empty.rs', 0), stateSlots: [] }, lineCount: 0, roleFact: 'source' });
    const unknown = buildFileOrganismGeometry({ anatomy: { ...anatomy('src/unknown.rs', 0), stateSlots: null }, lineCount: 1, roleFact: 'source' });
    expect(known.stateSlots).toHaveLength(15);
    expect(known.stateSlots.map(slot => slot.name)).toContain('state_14');
    expect(known.stateSlots.every(slot => inside(known.bounds, { x: slot.x, y: slot.y }))).toBe(true);
    expect(empty.stateSlots).toHaveLength(0);
    expect(empty.stateNote).toBe('No stored state slots');
    expect(unknown.stateSlots).toHaveLength(0);
    expect(unknown.stateNote).toBe('Stored state unknown');
    expect(hitTestFileOrganismState(known, { x: known.stateSlots[14].x, y: known.stateSlots[14].y })?.id).toBe(known.stateSlots[14].id);
  });

  it('Given short and long callable names, When intrinsic label bounds are built, Then full names fit without changing shared world row pitch', () => {
    const short = buildFileOrganismGeometry({ anatomy: anatomy('src/a.rs', 2), lineCount: 40, roleFact: 'source' });
    const longAnatomy = anatomy('src/b.rs', 2);
    longAnatomy.callables[0].displayName = 'very_long_module::nested_owner::very_long_function_name';
    const long = buildFileOrganismGeometry({ anatomy: longAnatomy, lineCount: 40, roleFact: 'source' });
    expect(long.rowPitch).toBe(short.rowPitch);
    expect(long.limbs[0].labelRegion.width).toBeGreaterThanOrEqual(long.limbs[0].displayName.length * 7 + 12);
    expect(inside(long.bounds, { x: long.limbs[0].labelRegion.minX, y: long.limbs[0].labelRegion.minY })).toBe(true);
    expect(inside(long.bounds, { x: long.limbs[0].labelRegion.maxX, y: long.limbs[0].labelRegion.maxY })).toBe(true);
  });

  it('Given selected callable anatomy, When the shared painter draws its body and limbs, Then projected contour is reused and metric thickness stays immutable', () => {
    const geometry = buildFileOrganismGeometry({ anatomy: anatomy('src/paint.rs', 4), lineCount: 300, roleFact: 'test' });
    const thicknesses = geometry.limbs.map(limb => limb.thickness);
    const originalPath2D = globalThis.Path2D;
    class PathRecorder {
      calls: unknown[][] = [];
      moveTo(...args: number[]) { this.calls.push(['moveTo', ...args]); }
      lineTo(...args: number[]) { this.calls.push(['lineTo', ...args]); }
      quadraticCurveTo(...args: number[]) { this.calls.push(['quadraticCurveTo', ...args]); }
      ellipse(...args: number[]) { this.calls.push(['ellipse', ...args]); }
      closePath() { this.calls.push(['closePath']); }
    }
    globalThis.Path2D = PathRecorder as unknown as typeof Path2D;
    try {
      const bodyPath = createFileOrganismBodyPath(geometry, { x: 25, y: 35, scale: 2 });
      expect(bodyPath.calls[0]).toEqual([
        'ellipse',
        25,
        35 + geometry.body.height,
        geometry.body.width,
        geometry.body.height,
        0,
        0,
        Math.PI * 2,
      ]);
      const context = new Proxy({ globalAlpha: 1 }, { get(target, key) { return key in target ? target[key as keyof typeof target] : () => {}; }, set(target, key, value) { (target as Record<PropertyKey, unknown>)[key] = value; return true; } });
      paintFileOrganism(context as unknown as CanvasRenderingContext2D, geometry, { selectedCallableId: geometry.limbs[0].id });
      expect(geometry.limbs.map(limb => limb.thickness)).toEqual(thicknesses);
    } finally {
      if (originalPath2D) globalThis.Path2D = originalPath2D;
      else delete (globalThis as typeof globalThis & { Path2D?: typeof Path2D }).Path2D;
    }
  });

  it('Given equally branchy callables with short and very long bodies, When either limb is selected, Then outline radius stays terminal-sized and stroke thickness stays measured', () => {
    const shortAnatomy = anatomy('src/short.rs', 1, 2, 3);
    const longAnatomy = anatomy('src/long.rs', 1, 9000, 3);
    const short = buildFileOrganismGeometry({ anatomy: shortAnatomy, lineCount: 900, roleFact: 'source' });
    const long = buildFileOrganismGeometry({ anatomy: longAnatomy, lineCount: 900, roleFact: 'source' });
    const originalPath2D = globalThis.Path2D;
    class PathRecorder {
      moveTo() {}
      lineTo() {}
      quadraticCurveTo() {}
      ellipse() {}
      closePath() {}
    }
    globalThis.Path2D = PathRecorder as unknown as typeof Path2D;
    const selectedRingRadius = (geometry) => {
      let strokeStyle = '';
      let ringRadius = null;
      const context = new Proxy({ globalAlpha: 1 }, {
        get(target, key) {
          if (key === 'strokeStyle') return strokeStyle;
          if (key === 'arc') return (_x, _y, radius) => { if (strokeStyle === '#test-selection') ringRadius = radius; };
          return key in target ? target[key as keyof typeof target] : () => {};
        },
        set(target, key, value) {
          if (key === 'strokeStyle') strokeStyle = String(value);
          else (target as Record<PropertyKey, unknown>)[key] = value;
          return true;
        },
      });
      const limb = geometry.limbs[0];
      const thickness = limb.thickness;
      paintFileOrganism(context as unknown as CanvasRenderingContext2D, geometry, { selectedCallableId: limb.id, selectionColor: '#test-selection' });
      expect(limb.thickness).toBe(thickness);
      return ringRadius;
    };
    try {
      const shortRadius = selectedRingRadius(short);
      const longRadius = selectedRingRadius(long);
      expect(long.limbs[0].length).toBeGreaterThan(short.limbs[0].length * 2);
      expect(longRadius).toBe(shortRadius);
      expect(longRadius).toBeLessThan(long.rowPitch / 2);
    } finally {
      if (originalPath2D) globalThis.Path2D = originalPath2D;
      else delete (globalThis as typeof globalThis & { Path2D?: typeof Path2D }).Path2D;
    }
  });

  it('Given an aligned absent callable, When geometry reserves its slot, Then its Not present label extent is included in full bounds', () => {
    const geometry = buildFileOrganismGeometry({ anatomy: anatomy('src/after.rs', 0), lineCount: 20, roleFact: 'source' }, {
      slotOrder: ['removed-callable'],
      absenceSlotKeys: ['removed-callable'],
    });
    const marker = geometry.absenceMarkers[0];
    expect(marker.labelRegion.width).toBeGreaterThan(50);
    expect(inside(geometry.bounds, { x: marker.labelRegion.minX, y: marker.labelRegion.minY })).toBe(true);
    expect(inside(geometry.bounds, { x: marker.labelRegion.maxX, y: marker.labelRegion.maxY })).toBe(true);
  });
});
