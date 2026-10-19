# Vistaforge app

The Windows desktop app: pick a preset, shape the map, see it in 3D, and write the world into
the game's saves folder. It also opens worlds already played (read only): a map of every saved
chunk, and a square of it in the 3D view and render mode. Tauri 2 shell, Svelte 5 + TypeScript UI, three.js for the 3D view.

```
app/
  src/                 the UI
    lib/app.svelte.ts  app state and actions
    lib/backend/       how the UI reaches the core: tauri.ts (the app), http.ts (dev server), mock.ts
    lib/scene3d.ts     3D preview
    lib/decode.ts      reads the packed preview (layout in bridge/src/encode.rs)
    lib/world.svelte.ts  an opened world: its map, the chosen square, render mode's scenes of it
    components/        sidebar panels, 3D view, map editor, export dialog
  bridge/              Rust: everything the app asks of the core, with no Tauri dependency
    src/world/         reads Minecraft 1.18+ worlds: regions, chunks, the map, render scenes
    src/bin/devserver.rs   the same commands over HTTP, for working on the UI in a browser
  src-tauri/           Rust: the Tauri window, a thin layer over the bridge
  tests/               browser tests (Playwright) against the mock backend
```

`app/` is its own cargo workspace. The Tauri crate needs webkit on Linux, which the cloud
container lacks, so it is built on Windows CI only (`.github/workflows/app.yml`); the bridge
builds and tests anywhere.

## Working on it

```
npm ci
npm run dev        # UI in a browser with made-up data: http://localhost:1420
npm run dev:real -- --catalog /path/to/catalog.json
                   # UI in a browser with the real core: http://localhost:1420/?backend=http
npm run check      # types
npm test           # unit tests
npm run build && npm run e2e   # browser tests
cargo test -p vistaforge-bridge --all-features   # Rust tests (use the real catalog if present)
npx tauri build    # the app, on Windows
```

Rules worth knowing:

- No inline `style="…"` attributes: the window's content security policy blocks them. Use
  classes, or `style:` directives (they go through CSSOM). `vite preview` serves the same policy,
  so the browser tests catch slips.
- Commands keep the same names and snake_case arguments in `src-tauri/src/main.rs`,
  `bridge/src/bin/devserver.rs` and `src/lib/backend/*.ts`.
- The UI reads presets, biomes and slider names from the core at start-up. A new biome, preset or
  slider shows up without UI changes (labels for sliders live in `src/lib/sliders.ts`).
- Tests steer the mock through `window.__vistaforgeMock` (see `MockHooks` in `mock.ts`).
