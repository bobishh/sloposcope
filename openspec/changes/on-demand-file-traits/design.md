## Context

Local anatomy already measures file size, callable ranges, visibility, arity and syntax decisions. Jev has a separate explicit batch approval flow and cache, but its entry point replaces the graph and its results are textual. The classifier reports role, probability of mixed responsibilities, information hiding and context sufficiency; it does not identify responsibility groups.

## Goals / Non-Goals

**Goals:** File-local entry point; one selected source group; explicit request-count confirmation; immutable source-bound enrichment; one geometry and painter for map and comparison.

**Non-Goals:** Automatically calling a model; counting responsibilities from probability; complete taxonomy implementation; new extraction engines; rewriting windows or batch transport; class-level semantic resolution.

## Decisions

- Reuse the existing approval panel in a file-scoped mode inside the existing inspector. Opening the action builds a local index and preview only. Sending remains a separate explicit action, bounded to one request. Remove the global analysis navigation entry; retain the reusable panel's bulk mode internally.
- Export the full-source SHA-256 alongside the existing model/rubric cache identity. Backfill old cached assessments only when their current content-derived cache key matches. Geometry accepts interpretation only for the same source hash, file path, supported rubric, sufficient context and finite bounded answers.
- Keep interpretation projection in a small typed module. It owns evidence validation, not network activity or anatomy measurement. Extraction owns source hashes; geometry owns all shape coordinates; UI owns pending/failure/confirmation.
- Add a shield-shaped head, mandibles and articulated antennae to the shared geometry. Their reserved bounds remain constant across interpretation changes. A head suture indicates estimated responsibility mixing; inner shell ridges indicate estimated information hiding. Both remain explicitly labelled as estimates. No extra heads are fabricated. Decorative antennae do not claim a dependency metric until that metric is connected.
- Apply enrichment independently to matching Before and After snapshots. A working-copy result cannot leak into a different historical snapshot. Map updates reuse geometry without resetting camera/windows.

## Risks / Trade-offs

- Incomplete context → withhold interpreted morphology while retaining local facts.
- Source edits or project switches during requests → invalidate approvals/results at UI boundaries; retain backend single-use approval checks and source reindexing.
- Stale cache records → withhold without matching full-source evidence.
- Model uncertainty → show numeric probability/score and provenance, never exact responsibility counts or quality claims.
- A local index still scans eligible sources → no paid request amplification; only the selected file's content group enters the preview.
