## Why

Repository exploration must work entirely from local facts. Optional model interpretation should enrich a selected file only after an explicit, bounded request, without replacing the existing workspace UI.

## What Changes

- Add an analysis action inside file inspection, with a one-file preview and explicit send confirmation.
- Keep repository opening, file opening, camera movement and source updates free of model requests.
- Attach cached interpretations only to matching source snapshots, including comparison endpoints.
- Extend the shared organism geometry with a recognizable head and optional interpreted markings for responsibility mixing and information hiding. Unknown interpretation remains unknown; probability does not become a responsibility count.
- Preserve measured body size, callable limbs, age, windows, timeline and ordinary diff.

## Capabilities

### New Capabilities

- `on-demand-file-traits`: Explicit file-scoped analysis and source-bound visual enrichment.

### Modified Capabilities

None.

## Impact

Existing Jev inventory/approval/cache, file inspection, shared map/detail geometry, Rust source evidence, isolated tests. No new model provider, ambient credentials or automatic sends.
