import { expect, it } from 'vitest';
import { buildFileOrganismGeometry, getLimbPaintPlan } from './fileOrganism';

it('Given 1000 subpixel rows, When zoomed out, Then cached paint bins retain visibility diversity and metric extremes', () => {
  const geometry = buildFileOrganismGeometry({ lineCount: 9145, anatomy: { file: 'lib.rs', callables: Array.from({ length: 1000 }, (_, i) => ({ id: `${i}`, visibility: i % 3 ? 'public' : 'private', bodyLineSpan: i + 1, decisionCount: i % 20 })) } });
  const plan = getLimbPaintPlan(geometry, .001);
  expect(plan.length).toBeLessThan(100);
  expect(new Set(plan.map(limb => limb.visibility))).toEqual(new Set(['public', 'private']));
  expect(Math.max(...plan.map(limb => limb.length))).toBe(Math.max(...geometry.limbs.map(limb => limb.length)));
  expect(Math.max(...plan.map(limb => limb.thickness))).toBe(Math.max(...geometry.limbs.map(limb => limb.thickness)));
  expect(getLimbPaintPlan(geometry, .001)).toBe(plan);
  expect(getLimbPaintPlan(geometry, 1)).toBe(geometry.limbs);
  expect(geometry.limbs).toHaveLength(1000);
});
