import { describe, expect, it } from 'vitest';
import { buildBugComparison } from './bugComparisonModel.js';

const callable = (id, name, owner, extras = {}) => ({
  id,
  matchKey: `${owner}|def|${name}`,
  owner,
  kind: 'def',
  name,
  bodyLineSpan: 4,
  decisionCount: 1,
  ...extras,
});

const anatomy = (callables, sourceId = 'source-a') => ({
  status: 'partial', file: 'src/example.ex', sourceId, profileId: 'ex-v1', callables,
});

describe('buildBugComparison', () => {
  it('matches only unique owner, kind, and name identities and keeps stable slots', () => {
    const result = buildBugComparison(
      anatomy([callable('b:run', 'run', 'Outer')]),
      anatomy([callable('a:run', 'run', 'Outer', { bodyLineSpan: 7 })]),
    );

    expect(result.rows).toEqual([expect.objectContaining({
      status: 'changed', beforeId: 'b:run', afterId: 'a:run',
      slotKey: 'Outer|def|run', deltas: { bodyLineSpan: 3, decisionCount: 0 },
    })]);
    expect(result.beforeSlotKeys).toEqual(['Outer|def|run']);
    expect(result.afterSlotKeys).toEqual(['Outer|def|run']);
  });

  it('does not pair repeated match keys or guess renamed declarations', () => {
    const result = buildBugComparison(
      anatomy([callable('b:run1', 'run', 'Outer'), callable('b:run2', 'run', 'Outer')]),
      anatomy([callable('a:execute', 'execute', 'Outer')]),
    );

    expect(result.rows.map(row => row.status)).toEqual(['unresolved', 'unresolved', 'unresolved']);
    expect(result.rows.every(row => row.deltas === null)).toBe(true);
  });

  it('distinguishes no baseline and unavailable anatomy from missing files', () => {
    expect(buildBugComparison(null, anatomy([]), { hasBaseline: false }).state).toBe('no_comparison');
    expect(buildBugComparison({ status: 'unavailable' }, anatomy([])).rows).toEqual([]);
    expect(buildBugComparison(null, anatomy([])).afterState).toBe('available');
  });

  it('marks a new file as added and withholds deltas across incompatible profiles', () => {
    const added = buildBugComparison(
      { status: 'absent', anatomy: null },
      { status: 'available', anatomy: anatomy([callable('a:run', 'run', 'M')]) },
      { hasBaseline: true },
    );
    expect(added.state).toBe('ready');
    expect(added.rows.map(row => row.status)).toEqual(['added']);

    const beforeProfile = anatomy([callable('b:run', 'run', 'M')]);
    const afterProfile = anatomy([callable('a:run', 'run', 'M', { bodyLineSpan: 9 })]);
    afterProfile.profileId = 'ex-v2';
    const incompatible = buildBugComparison(beforeProfile, afterProfile);
    expect(incompatible.rows[0].status).toBe('matched');
    expect(incompatible.rows[0].deltas).toBeNull();
  });

  it('uses qualified match keys when parser records omit a separate owner field', () => {
    const before = anatomy([{ id: 'b:old', matchKey: 'Outer::def::old', name: 'old', kind: 'def' }]);
    const after = anatomy([{ id: 'a:new', matchKey: 'Outer::def::new', name: 'new', kind: 'def' }]);
    expect(buildBugComparison(before, after).rows.map(row => row.status)).toEqual(['unresolved', 'unresolved']);
  });

  it('does not claim a same-named callable moved between owners as a deletion and addition', () => {
    const before = anatomy([{ id: 'b:run', matchKey: 'Outer::def::run', name: 'run', kind: 'def' }]);
    const after = anatomy([{ id: 'a:run', matchKey: 'Inner::def::run', name: 'run', kind: 'def' }]);
    expect(buildBugComparison(before, after).rows.map(row => row.status)).toEqual(['unresolved', 'unresolved']);
  });

  it('does not claim duplicate After identities as confident additions', () => {
    const after = anatomy([
      callable('a:run1', 'run', 'Outer'),
      callable('a:run2', 'run', 'Outer'),
    ]);
    const result = buildBugComparison(anatomy([]), after);
    expect(result.rows.map(row => row.status)).toEqual(['unresolved', 'unresolved']);
  });

  it('classifies additions and removals after exact matches are reserved', () => {
    const beforeWithFoo = anatomy([callable('b:foo', 'foo', 'Outer')]);
    const afterWithFooAndBar = anatomy([
      callable('a:foo', 'foo', 'Outer'),
      callable('a:bar', 'bar', 'Outer'),
    ]);
    const addition = buildBugComparison(beforeWithFoo, afterWithFooAndBar);
    expect(addition.rows.map(row => [row.after?.name || row.before?.name, row.status])).toEqual([
      ['foo', 'matched'], ['bar', 'added'],
    ]);

    const beforeWithFooAndBar = anatomy([
      callable('b:foo', 'foo', 'Outer'),
      callable('b:bar', 'bar', 'Outer'),
    ]);
    const removal = buildBugComparison(beforeWithFooAndBar, beforeWithFoo);
    expect(removal.rows.map(row => [row.before?.name || row.after?.name, row.status])).toEqual([
      ['foo', 'matched'], ['bar', 'removed'],
    ]);
  });
});
