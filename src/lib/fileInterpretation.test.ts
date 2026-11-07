import { describe, expect, it } from 'vitest';
import { projectFileInterpretation } from './fileInterpretation';
import { buildFileOrganismGeometry } from './fileOrganism';

const anatomy = { file: 'src/a.ts', sourceId: 'sha256:abc', callables: [], lineCount: 30 };
const assessment = {
  id: 'cached-a', paths: ['src/a.ts'], source_id: 'sha256:abc', model: 'jev-1.13.0', rubric: 'file-traits-v1',
  answers: { context: { choice: 'sufficient' }, role: { choice: 'orchestration' }, multiple: { noul: 0.8 }, hiding: { score: 1.5 } },
};

describe('snapshot-bound file interpretation', () => {
  it('Given matching evidence, When projected, Then preserves estimates without inventing responsibility counts', () => {
    expect(projectFileInterpretation(anatomy, assessment)).toMatchObject({ responsibilityMix: 0.8, informationHiding: 1.5, role: 'orchestration' });
    expect(projectFileInterpretation(anatomy, assessment)).not.toHaveProperty('responsibilityCount');
  });
  it.each([
    null,
    { ...assessment, source_id: 'sha256:old' },
    { ...assessment, paths: ['other.ts'] },
    { ...assessment, rubric: 'unknown' },
    { ...assessment, source_id: undefined },
    { ...assessment, answers: { ...assessment.answers, context: { choice: 'insufficient' } } },
    { ...assessment, answers: { ...assessment.answers, multiple: { noul: 2 } } },
    { ...assessment, answers: { ...assessment.answers, hiding: { score: NaN } } },
  ])('Given unusable evidence, When projected, Then interpretation stays unknown', value => {
    expect(projectFileInterpretation(anatomy, value)).toBeNull();
  });
  it.each([0, 300, 1000])('Given %i callables, When interpretation arrives, Then measured geometry and full bounds stay stable', count => {
    const snapshot = { ...anatomy, callables: Array.from({ length: count }, (_, index) => ({ id: `${index}`, bodyLineSpan: 8, decisionCount: 2 })) };
    const plain = buildFileOrganismGeometry({ anatomy: snapshot, lineCount: 9145 });
    const enriched = buildFileOrganismGeometry({ anatomy: snapshot, lineCount: 9145, assessment });
    expect(enriched.interpretation?.responsibilityMix).toBe(0.8);
    expect(enriched.head.bounds.minY).toBeGreaterThanOrEqual(enriched.bounds.minY);
    expect(enriched.head.bounds.maxX).toBeLessThanOrEqual(enriched.bounds.maxX);
    expect(enriched.bounds).toEqual(plain.bounds);
    expect(enriched.limbs).toEqual(plain.limbs);
    expect(enriched.body).toEqual(plain.body);
  });
});
