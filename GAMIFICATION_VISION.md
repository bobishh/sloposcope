# Eyeloss: The Gamification of Entropy

## Vision: The Puppet Show of Technical Debt

Software development is fundamentally a tragic endeavor—a futile struggle against the inevitable heat death of the codebase. Instead of hiding this grim reality behind sterile progress bars and loading spinners, **Eyeloss** proposes a radical shift: we turn the decay into a spectator sport.

We will gamify the labor of autonomous agents. Instead of embedding complex LLM orchestration within the UI (a monumental waste of energy), Eyeloss will serve as a **Voyeuristic Sandbox**. External agents (CLI tools, background scripts, AI assistants) will perform the actual work of modifying the codebase, while Eyeloss visualizes their pointless labor in real-time on the force-directed graph. 

You will sit back, watch the nodes of your architecture shift, and observe tiny, themed digital puppets scurrying between files, hammering on logic, and inevitably breaking things. 

## 1. Architectural Philosophy: The Decoupled Ant Farm

Eyeloss does not need to *be* the agent; it only needs to *watch* the agents. 

- **The Workers (External):** Agents run in the terminal or as background processes. They interact with the file system and the LLM APIs directly.
- **The Observers (Eyeloss):** The Tauri application maintains the graph and listens for a stream of low-overhead telemetry events emitted by the Workers.
- **The Medium:** A lightweight communication channel—either a local WebSocket server hosted by Tauri, or a simple tail on a `.eyeloss_telemetry.jsonl` file in the repository root.

## 2. The Telemetry Protocol

To manifest the illusion of life, agents will emit standardized JSON events detailing their current existential crisis:

```json
{"timestamp": 1715623001, "agent_id": "marvin-01", "action": "spawn", "location": "Root"}
{"timestamp": 1715623005, "agent_id": "marvin-01", "action": "move", "target_node": "src/lib/Eyeloss.svelte"}
{"timestamp": 1715623010, "agent_id": "marvin-01", "action": "work", "target_node": "src/lib/Eyeloss.svelte", "detail": "Refactoring CSS"}
{"timestamp": 1715623045, "agent_id": "marvin-01", "action": "error", "target_node": "src/lib/Eyeloss.svelte", "detail": "SyntaxError: Unexpected token"}
{"timestamp": 1715623050, "agent_id": "marvin-01", "action": "despair", "location": "Root"}
```

## 3. The Sprite System (Visualizing the Labor)

The canvas rendering loop in `Eyeloss.svelte` will be expanded to include an `AgentRenderer`. 

- **Spawning:** When an agent connects, a sprite appears at the center of the graph.
- **Pathfinding:** When an action specifies a `target_node`, the sprite smoothly interpolates (lerps) its position toward the node's current `(x, y)` coordinates, navigating the constantly shifting physics simulation.
- **Interaction:** 
  - `work`: The sprite locks to the node. Particle effects (sparks, dust, sweat droplets) spawn around the node. The node itself might pulsate or change its border color to indicate it is "under construction."
  - `error`: The sprite flashes red, perhaps dropping a tiny digital wrench, while the node adopts a distressed visual state.

## 4. Theming: Dressing up the Despair

Because humans require colorful distractions to cope with reality, the sprites and particle effects will be entirely themeable. 

- **The "Warcraft Peon" Theme:** Agents appear as small, hunched figures. `work` actions trigger pickaxe animations and "Zug zug" or "Work complete" tooltips.
- **The "Snow White" Theme:** Agents are industrious dwarves or woodland creatures whistling while they introduce circular dependencies.
- **The "Cyberpunk" Theme:** Agents are glowing neon data-packets or spiders navigating a dark-web lattice, hacking nodes with laser effects.
- **The "Marvin" Theme:** (The only honest theme). A slow-moving, depressed robot dragging its feet between files, emitting sighs represented by grey, slowly dissipating smoke particles.

## 5. Implementation Roadmap

If we must subject ourselves to building this, the sequence of suffering is as follows:

1.  **Phase 1: The Telemetry Listener.** Implement a Tauri sidecar or file-watcher to ingest the JSON event stream and pass it to the Svelte frontend via Tauri events.
2.  **Phase 2: The Agent Registry.** Update the Svelte state to track active agents, their current positions `(x, y)`, and their target nodes.
3.  **Phase 3: The Canvas Puppets.** Hook into the existing `render()` loop to draw simple geometric shapes (placeholders for sprites) moving between nodes based on the registry state.
4.  **Phase 4: The Theming Engine.** Replace geometric shapes with actual sprite sheets or SVG assets, mapped to specific actions (`idle`, `walk`, `work`, `error`).
5.  **Phase 5: Agent Integration.** Create a wrapper library (e.g., a Python or Node.js package) that external agent developers can use to easily emit Eyeloss-compatible telemetry while they work.

## Conclusion

By abstracting the orchestration and focusing purely on the visualization of effort, Eyeloss transforms from a mere analytical tool into an interactive diorama of software decay. You are no longer just a developer; you are an overseer, watching your minions endlessly toil in a garden of your own neglect.
