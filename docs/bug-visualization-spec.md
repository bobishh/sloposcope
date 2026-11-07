# Bug Visualization Specification

## Status and purpose

This document specifies target behavior, not a claim that every metric or view is implemented.

Sloposcope should make structural changes in generated code visible before the user reads source or a textual diff. A bug represents a file; its anatomy represents measured structure, not an assessment invented by a renderer or model.

The main distinction is between many small functions and a few large, branch-heavy functions. Neither shape alone proves good or bad architecture.

This extends the existing repository map and file inspector, not a separate ecosystem scene. Source and textual Diff views remain available. Controls and panels follow the repository's existing angular UI; organic anatomy does not justify rounded interface containers.

## Shared anatomy

The repository map and file inspector consume the same file anatomy and visual grammar. They are projections of the same facts, not independently designed creatures.

| Visual feature | Meaning |
| --- | --- |
| One bug | One file in one identified snapshot |
| One limb | One recognized callable declaration, including methods and private functions |
| Limb length | Measured function-body size |
| Limb thickness | Measured branching under an explicit language profile |
| Glove or folded terminal | Captured non-public visibility, distinguished by the legend |
| Named body spot | One captured stored-state slot |
| Shell pattern or file-kind marker | Declared file classification, such as source, test, or configuration |

Connections between files remain graph edges. Static dependency degree must not create additional limbs. Decorative head, eyes, antennae, and shell features must not imply uncaptured state or authorship.

Names and numbers explain anatomy when inspected. They must not be the only way to notice differences in function size and branching. A detached function list is not a substitute for informative geometry.

### Measurement rules

- The baseline size measure is the inclusive source-line span of the recognized function body, including blank and comment lines inside that span. The language profile identifies the body boundary, including expression-bodied functions. This is a source-span measure, not a count of executable statements.
- The baseline branching measure is `decision_count`, not an unspecified universal complexity score. Each language profile enumerates counted constructs and their contributions. Nested callable bodies belong to their own callable records and must not inflate the enclosing callable's branching measure.
- Do not label `decision_count` as cyclomatic or cognitive complexity without implementing and identifying that specific metric's rules.
- Length and thickness use stable, monotonic mappings shared by the map and inspector. Equal measured values under the same mapping produce equal base dimensions. A larger value must not produce a smaller dimension.
- Do not normalize dimensions independently per file or per side of a comparison. A joint fit or camera scale may resize an entire view. Any nonlinear metric mapping must be disclosed in the legend; inspection shows raw values.
- Selection must not overwrite metric-bearing thickness. Use a separate highlight or outline.
- Unknown size, branching, visibility, or state remains explicitly unknown. Missing measurements must not look like measured zero or imply a small, simple function.

### Language independence and evidence

The rendering contract is language-independent. Extraction rules are language-specific and must cover Rust, Ruby, JavaScript/TypeScript, Elixir, and Elm before those languages are advertised as supporting measured anatomy.

Each profile identifies its version, recognized callable forms, body boundaries, branching rules, and unsupported cases. Rules for clauses, overloads, and nested declarations must be explicit. Repeated names must not silently merge distinct declarations. Raw query captures must not be presented as complete semantic coverage when the profile is partial.

Tree-sitter and deterministic analysis provide structural facts. Linter findings and model assessments may provide separate, attributed inspection layers; they must not silently replace measured dimensions or manufacture missing metrics. Basic anatomy and its comparison require no model request.

Each anatomy result identifies its file, source snapshot, and extraction profile. Each measured value must be traceable to its rule and source declaration. An unavailable or partial profile is shown as such.

## Repository map: one snapshot

The map shows exactly one bug per represented file in the selected snapshot. It does not paint a previous silhouette, a historical twin, or a before/after pair for every file.

- Glyphs use the shared anatomy, including callable count, function size, and branching where measured.
- Zoom may hide labels and inspection details. It must not add, remove, sample, or mirror callable limbs, or substitute an unrelated icon grammar at a threshold.
- At small scales, adjacent features may merge into a silhouette because of pixel resolution. Their underlying anatomy and metric mappings remain unchanged.
- Folder regions constrain placement and show filesystem grouping. They do not imply runtime boundaries or change the meaning of anatomy.
- Existing activity and changed-file indicators remain distinct from the anatomy's metric and visibility channels.

The map needs anatomy only for its selected snapshot. It must not request or render every file's baseline anatomy merely to support possible inspection later.

## File inspector: on-demand bug diff

Clicking a file opens its anatomical comparison: **Before -> After**. The default comparison uses two separate, aligned bugs, not overlapping historical silhouettes.

### Comparison endpoints

Before and After are the two endpoints of the selected comparison, not necessarily adjacent commits. The inspector labels both endpoints and the relevant file paths. An explicit range, selected commit, and working-copy comparison must retain their own endpoint semantics.

The map bug corresponds to the comparison's After snapshot. The inspector must not silently compare a historical map bug with today's file contents. If the current selection does not define a baseline, show single-snapshot anatomy with an explicit "No comparison selected" state rather than guessing one.

Resolve endpoint identities once for each request. A working-copy edit creates a new source identity; it must not mutate an already displayed comparison without an explicit refresh or update indication.

### Alignment and change markings

- Both sides use the same metric mappings and display scale. Do not fit each bug independently to the same bounding box.
- Matched callables retain corresponding attachment slots across the two sides. Do not reorder them independently by size, complexity, or label length.
- Added and removed callables keep a reserved counterpart slot with an absence marker. The absence marker is not a zero-size limb or a fabricated function.
- Mark additions, removals, and measured changes separately from visibility and file-kind styling. A diff must not turn a public function into a private-looking function by reusing its color channel.
- Selecting a limb inspects its identity, available source range, measured before/after values, and their differences. Names and facts remain attached to the relevant organ; no duplicate function list is required.
- Matching must account for declaration identity and lexical ownership, not name alone. Renames or moves may be paired only with sufficient evidence. Ambiguous correspondence is explicitly unresolved, not a confident match or a proven deletion/addition.

Changing another part of the file must not change an untouched function's measured dimensions. Formatting edits may legitimately change its source-span length without changing branching; the two channels must remain independent.

### Loading and failure states

- Request baseline anatomy only for the opened file comparison. Reuse an available After anatomy only when file, source identity, and profile match.
- Reserve both comparison panels and their controls while loading. Pending indicators, empty states, and failures must not move surrounding controls or collapse one side.
- A newly added file shows "File did not exist" on the Before side. A removed file shows that state on the After side. Neither is represented as an empty creature.
- A removed file remains inspectable through the comparison's changed-file entry; opening it must not require a fabricated bug in the After map.
- A missing historical version, unsupported profile, or failed read shows an explicit unavailable state. It must not be interpreted as a deleted file, zero functions, or zero complexity.
- Partial metrics remain partial on either side. Show a numeric delta only when both values were measured under compatible rules.
- Changing the selected file, comparison, or source identity invalidates in-flight results for the previous selection. Never attach a stale response to the new comparison.

## Acceptance scenarios

### Shape communicates structure without names

- Given one file with many short, low-branching functions and another with a few long, branch-heavy functions under supported profiles,
- When labels are hidden and both bugs use the same display scale,
- Then their limb counts, lengths, and thicknesses visibly distinguish those distributions without asserting which file is better.

### Map contains no historical twins

- Given a comparison with many changed files,
- When the map displays its After snapshot,
- Then each file has one bug and no previous silhouette; baseline anatomy is not requested for every map node.

### Zoom preserves anatomy

- Given a file with seventeen recognized callable declarations,
- When the user zooms between repository and detailed views,
- Then its anatomy still has seventeen limbs, with the same underlying size and branching values; only display scale and detail visibility change.

### Opening a file reveals the selected comparison

- Given an explicit Before/After selection,
- When the user opens a file,
- Then the inspector shows labeled, aligned anatomy for those endpoints, using a common scale and no per-side normalization.

### Growth affects only its measured channels

- Given a matched function whose body grows while its branching count stays unchanged,
- When its comparison opens,
- Then its After limb is longer but not thicker; selecting it reveals both measurements and their deltas.

### Matching preserves distinct declarations

- Given two classes with same-named methods and a change to only one method,
- When the comparison is constructed,
- Then the methods remain distinct and the changed method is paired with its own predecessor rather than the other class's method.

### Added, absent, and unavailable are different

- Given a genuinely new file and a different file whose historical read fails,
- When each comparison opens,
- Then the former shows "File did not exist" and the latter shows unavailable history; neither uses a fabricated empty bug.

### Pending and stale responses preserve context

- Given a pending comparison for one file,
- When the user selects a different file before the request completes,
- Then panel geometry stays stable and the earlier response cannot populate the newly selected comparison.

### Unknown metrics are not low metrics

- Given a function with an unavailable branching measurement,
- When it appears on the map or in a comparison,
- Then its branching is marked unknown, no zero is asserted, and no numeric branching delta is shown against it.

## Extraction and rendering prerequisites

Captured names and kinds alone are insufficient for measured anatomy. Supported profiles must also extract body boundaries and branching measures; unavailable arity, source ranges, visibility, or state must remain explicit rather than being fabricated by the renderer.

Viewport-derived limb length, selection-derived thickness, zoom-dependent limb-count caps, and dependency-degree limbs do not satisfy this contract. Updating this document does not implement their replacement or advertise a language profile as complete.
