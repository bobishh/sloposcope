export function isCurrentGraphSnapshotRequest(requestId, latestRequestId, request, currentRequest) {
  return requestId === latestRequestId && JSON.stringify(request) === JSON.stringify(currentRequest);
}
