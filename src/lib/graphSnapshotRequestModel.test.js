import { describe, expect, it } from 'vitest';
import { isCurrentGraphSnapshotRequest } from './graphSnapshotRequestModel.js';

describe('isCurrentGraphSnapshotRequest', () => {
  const request = { mode: 'investigation', seed: { kind: 'working_copy' } };

  it('rejects an older same-selection anatomy batch after a newer refresh starts', () => {
    expect(isCurrentGraphSnapshotRequest(4, 5, request, structuredClone(request))).toBe(false);
  });

  it('rejects a response when the selected endpoint changed during loading', () => {
    expect(isCurrentGraphSnapshotRequest(
      5,
      5,
      request,
      { mode: 'investigation', seed: { kind: 'commit', revset: 'abc123' } },
    )).toBe(false);
  });

  it('accepts the latest response for its still-selected endpoint', () => {
    expect(isCurrentGraphSnapshotRequest(5, 5, request, structuredClone(request))).toBe(true);
  });
});
