# Sloposcope

- Read [README.md](README.md). Stack: Tauri, Svelte, Rust; graph and history support Git/Jujutsu repositories.
- Frontend unit checks: `npx vitest run`; tests are co-located with source.
- Browser checks: `npm run test:e2e` or `npx playwright test e2e/graph.test.js`.
- `playwright.config.js` builds the frontend and starts an isolated Vite preview. Port defaults to 4173; override with `SLOPOSCOPE_PLAYWRIGHT_PORT` if occupied. `reuseExistingServer` is false.
- Browser flows use mock data/Tauri boundaries where configured; they do not prove native desktop behavior. Check changed Rust/native paths separately.
- Backend checks: run `cargo test` from `src-tauri/`; integration tests live in `src-tauri/tests/` when present.
- `npm run dev` starts Vite; `npm run tauri dev` starts the desktop app.
