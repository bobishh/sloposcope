# Experience Projection Vision

## Problem

Current surface leaks machinery.

- too many controls
- too many tabs
- too many raw runtime concepts
- user forced to think like operator of debug console

Wrong shape.

User should command mission.
System should expose pressure, pace, mess, drift, progress.

## Core principle

Do not hardcode one fantasy.

`fortress`, `space sim`, `factory`, `city`, `fleet` are skins.
They are not source of truth.

Source of truth is app architecture and runtime state.

UI should project same underlying system in different visual languages.

## Canonical model

These are internal product concepts. Stable. Theme-agnostic.

- `Repository`
  - identity across time
- `Map`
  - one checkout/worktree snapshot
- `CanonicalGraph`
  - code terrain for current map
- `Mission`
  - one user task on one map
- `Objective`
  - scoped unit inside mission
- `AgentProfile`
  - worker/planner/reviewer capability bundle
- `AgentSession`
  - running process or hosted session
- `Verification`
  - validation, gate, watchdog, review outputs
- `EventLedger`
  - replayable timeline
- `Risk`
  - drift, blast radius, conflicts, stale sessions

These should drive UI.
Not hardcoded labels like `galaxy`, `sector`, `room`.

## Architectural strata

Visual scale should map to system strata.

### Stratum 0: Substrate

Persistence and transport.

- SQLite stores
- search index
- event ledger
- profile config
- worktrees
- MCP transport

Theme examples:
- fortress: basement / archive / stockpile
- space sim: engine room / cargo bay / black box
- factory: foundation / storage / conveyor basement

### Stratum 1: Terrain

Code topology.

- repo
- directories
- files
- symbols
- clusters
- dependencies

Theme examples:
- fortress: rooms / shafts / districts
- space sim: hull / decks / subsystems / compartments
- factory: lines / cells / stations

### Stratum 2: Mission

User intent and plan.

- mission brief
- objectives
- acceptance checks
- priorities

Theme examples:
- fortress: work orders
- space sim: flight plan / operation order
- factory: production batch / run book

### Stratum 3: Session

Live worker activity.

- spawned sessions
- touched files
- heartbeats
- stuck state
- retries
- patches

Theme examples:
- fortress: dwarves / squads / haulers
- space sim: ships / drones / crews
- factory: robots / operators / carts

### Stratum 4: Control and risk

Judgment layer.

- watchdog
- validation
- patch gate
- merge/apply
- stale/conflict detection

Theme examples:
- fortress: overseer / inspectors / alarms
- space sim: command deck / radar / threat board
- factory: QA / safety / dispatch

## Semantic zoom

Zoom is not only camera scale.
Zoom changes ontology.

At each threshold user should see different objects.

### Far

Mission/system weather.

- active missions
- throughput
- blocked fronts
- total worker count
- drift pressure

### Mid

Subsystem pressure.

- clusters
- directories
- heat
- conflicts
- validation pulses

### Near

Concrete work.

- files
- symbols
- worker sessions
- patches
- ownership collisions

### Ledger mode

Time projection.

- event replay
- causality
- bursts
- dead time
- failure chains

## Projection modes

Same underlying state. Different readings.

### Topology

Code structure.

### Activity

What is moving now.

### Risk

Drift, blast radius, conflicts, stale workers, gate pressure.

### Flow

Mission/objective queue movement.

### History

Replay over time.

### Economics

Only when product becomes real vertical platform.
Not first shell.

Economics examples:

- token burn by mission / profile / provider
- wall-clock burn
- validation cost
- human-intervention count
- retry cost
- conflict cost
- acceptance latency
- model quality vs cost curves

Economics is a projection over canonical runtime facts.
Not a separate UI universe.

## Shell direction

Primary shell should be:

- `Coordinator chat`
- `World view`
- `Ledger`
- `Inspector`
- hidden `Machine Room`

### Coordinator chat

Main command surface.

User talks to coordinator.
Coordinator asks for missing mission facts, proposes plan, spawns workers, reports issues.

Current persona can be concrete without hardcoding metaphor:

- `Ruth`
- mid-40s
- terse
- ruthless about scope, validation, and drift

Persona is skin/content layer.
Thread, plan, report, nudge semantics stay architecture-driven.

### World view

Canvas for pressure and activity.

Not settings host.
Not giant form holder.

### Ledger

Bottom strip or pane.

- sessions
- objective transitions
- validation
- watchdog
- patch gate

### Inspector

Focused detail for selected mission/objective/file/worker.

### Machine Room

Advanced internals.

- profiles
- env JSON
- provider settings
- raw traces
- transport knobs
- MCP internals

## Skin system

Allow multiple thematic skins over same runtime model.

Examples:

- `fortress`
- `space_sim`
- `factory`

Skin can change:

- labels
- iconography
- colors
- motion language
- object sprites

Skin must not change:

- mission semantics
- graph semantics
- worker/session lifecycle
- verification model
- event ledger truth

## Immediate UI consequence

Do not keep growing current tabs.

Next shell pivot should:

- bury debug controls
- make coordinator chat primary
- make canvas a projection surface
- turn runs into ledger
- turn detail views into inspector

## Vertical platform consequence

If Sloposcope becomes true vertical platform:

- add economics as later projection
- add portfolio/organization scale above mission scale
- compare teams, models, harnesses, validation policies, cost curves

But do not force economics into first mission shell.
First shell should answer:

- what is happening
- where
- how fast
- how messy
- is it safe
