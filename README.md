# Sloposcope

<img width="1389" height="869" alt="Screenshot 2026-02-26 at 10 38 31" src="https://github.com/user-attachments/assets/e19868ad-0dc2-4e96-a307-672fb0e78b6f" />

Sloposcope is a local desktop codebase visualizer (Tauri + Svelte + Rust) for exploring file/module relationships, commit history, and live file activity.

## Product Specifications

- [Bug visualization](docs/bug-visualization-spec.md): shared metric-driven anatomy, single-snapshot map glyphs, and on-demand Before/After inspection. Defines target behavior and extraction/rendering prerequisites, not implementation completeness.

## What It Does

- Renders a force-directed map of parsed files/modules and call/import relations.
- Shows timeline/history from Git or Jujutsu (`jj`) repositories.
- Highlights touched files in near real time via filesystem watcher.
- Opens source/diff views for selected nodes.
- Clusters disconnected files by extension and scales node behavior by file size.

## Reactive Behavior (Git/JJ)

- Yes, it is reactive for both Git and JJ repos because updates come from a filesystem watcher, not from polling VCS logs.
- Practical meaning: edit/save files in the working copy -> graph + heat updates immediately.
- Current limitation: internal metadata paths (`.git/`, `.jj/`) are ignored by the watcher, so pure metadata-only VCS operations do not trigger UI updates until related working-copy files change.

## Run (Development)

1. Install dependencies:
   ```bash
   npm install
   ```

2. Start application:
   ```bash
   npm run tauri dev
   ```

## Build

Frontend bundle only:
```bash
npm run build
```

Desktop release build:
```bash
npm run tauri build
```

Built artifacts are produced under:
- `src-tauri/target/release/bundle/` (platform-specific installers/app bundles)

## Basic Usage

1. Launch app.
2. Open/select a repository path.
3. Use the timeline to move across commit windows.
4. Click nodes to inspect diff/source details.
5. Edit files locally and watch heat/highlight updates in real time.

## Features

- Force-directed graph visualization of files/modules.
- Git/JJ history integration.
- Source + split diff inspection.
- Multi-language parser support (tree-sitter based).
- Real-time local file change monitoring.
