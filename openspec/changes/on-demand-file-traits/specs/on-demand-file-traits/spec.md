## ADDED Requirements

### Requirement: Local exploration is model-free
The application SHALL render local file facts without Jev results and SHALL NOT send model requests when opening a repository or file, moving the camera, changing revisions or receiving file updates.

#### Scenario: Default file inspection
- **GIVEN** a repository with no requested interpretation
- **WHEN** a user opens a file inspector
- **THEN** measured anatomy is visible with an explicit file analysis action and no classifier send occurs

### Requirement: Explicit file-scoped approval
The inspector SHALL preview only the selected file's source group, disclose request count and payload, and require separate confirmation before sending at most one request. Credentials SHALL remain ephemeral. Pending, cancellation, exclusions and failure SHALL leave measured anatomy available. Failure SHALL NOT trigger automatic retry.

#### Scenario: Successful request
- **GIVEN** an eligible file
- **WHEN** the user requests analysis and confirms its preview
- **THEN** one bounded request is attempted and its matching result enriches the existing organism

#### Scenario: Failure or excluded source
- **GIVEN** a failing transport or a file excluded from the local inventory
- **WHEN** the user requests analysis
- **THEN** an actionable status appears, no unrelated source is selected, and the measured organism remains visible

### Requirement: Snapshot-bound interpretation
An interpreted trait SHALL require matching full-source hash and file path, supported rubric, sufficient context and valid numeric ranges. Missing evidence SHALL remain unknown. Cached results SHALL be reused without additional paid calls. Before and After SHALL be evaluated independently.

#### Scenario: Changed file or historical counterpart
- **GIVEN** an assessment for source A
- **WHEN** an organism represents different source B
- **THEN** no interpreted trait from A is attached to B

#### Scenario: Selected source differs before approval
- **GIVEN** a selected historical snapshot different from the current working copy
- **WHEN** file analysis is requested through the working-copy inventory
- **THEN** the preview is withheld and the user can explicitly select the working copy before sending

#### Scenario: Insufficient or malformed assessment
- **GIVEN** insufficient context, absent source evidence or an invalid score
- **WHEN** geometry is built
- **THEN** interpreted markings are withheld and measured size and limbs remain unchanged

### Requirement: Shared interpreted anatomy
The map and inspector SHALL use one geometry and painter. The organism SHALL have a recognizable head with mandibles and articulated antennae. Responsibility-mixing probability SHALL affect a labelled estimated head suture, not a count of responsibilities or heads. Information-hiding score SHALL affect labelled estimated inner shell ridges. Interpretation SHALL NOT alter measured limbs or shift the organism's full bounds.

#### Scenario: Enrichment across scales
- **GIVEN** valid matching cached interpretation
- **WHEN** the file is drawn on the map and in comparison
- **THEN** the same interpreted morphology is painted and all organs fit inside full world bounds

#### Scenario: Large and narrow inspection
- **GIVEN** 300 or 1000 callables and a narrow inspector
- **WHEN** interpretation is added or removed
- **THEN** limb spacing, selection, scrolling and snapshot comparison remain usable
