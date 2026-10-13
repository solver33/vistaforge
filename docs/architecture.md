# Vistaforge architecture

Vistaforge is a Windows app that generates square Minecraft 1.20.1 (Fabric) maps, up to 4096×4096 blocks, built from Conquest Reforged blocks. It reads the block catalog that the exporter mod (`exporter/`) writes from the user's own game. It ships no mod files.

**The bar is high world detail.** Ground should be made of CR layers that follow the terrain smoothly. Ground blocks should mix by biome, slope, moisture and noise. Plants should grow in natural clusters, and there should be rocks, fallen logs and debris. Trees should be visibly built from CR trunk and branch pieces. A flat field of one block with vanilla-looking trees is a failure.

## Layout

```
core/                 Rust library + `vistaforge` CLI (all generation logic)
  src/settings.rs     WorldSettings, Sliders, ShapeEdit, Preset      (contract)
  src/biome.rs        Biome enum                                     (contract)
  src/column.rs       Rect, ColumnInfo, ColumnGrid                   (contract)
  src/sink.rs         BlockId, Palette, Blocks, BlockSink            (contract)
  src/terrain/        land shape, water, biome layout, presets       (terrain)
  src/surface/        column blocks, Minecraft biome, preview colour (world detail)
  src/decor/          tree placement, plants, rocks, logs, debris    (world detail)
  src/trees/          species built from CR pieces                   (trees)
  src/engine/         pipeline, streaming export, preview            (engine)
  src/anvil.rs level.rs nbt.rs world.rs catalog.rs png.rs noise.rs   (engine)
app/                  Tauri 2 desktop app: UI, 3D preview, export    (app)
exporter/             Fabric catalog exporter mod
tools/                Python checkers for generated worlds
```

## Pipeline

The engine cuts the map into 64×64 units. For each unit it does the following, in parallel on all cores:

1. `Terrain::grid(unit + 16-column margin)` returns a `ColumnInfo` for every column: height (fractional, which becomes CR layers), water level and kind, biome and blend, slope, moisture, rock exposure and erosion flow.
2. `Surface::fill(x, z, grid, sink)` writes each column: bedrock, stone, soil, the top block or layer, and water.
3. `Decor::plan_trees(rect, column)` returns `TreeRequest`s, and `Trees::build(req, sink)` writes each tree.
4. `Decor::decorate(rect, grid, sink)` adds plants, rocks, logs and ground detail into what is still air.
5. Only the unit's own chunks are written to their region file. `Surface::minecraft_biome` sets each 4×4 biome cell.

Because every unit is generated with a margin and then clipped, **every module must be deterministic per position**. A decision may depend only on the seed, the coordinates and the column data. Hash the coordinates (`noise::hash2`, `unit2`), and never draw from a random sequence shared across the map. Trees may reach at most `trees::MAX_REACH` blocks from their trunk, and other decor at most `decor::MAX_REACH` blocks from their column. `engine::MARGIN` must cover both. The test `engine::tests::pieces_agree_across_borders` checks this.

A tree's shape must come from its `TreeRequest` and the ground alone, never from other trees or decor already in the sink. A piece near a border holds a different set of neighbouring trees than the piece next to it. The engine builds trees sorted by `(x, z, seed)`, so where crowns overlap, the later tree wins the shared cell in the same way in every piece. When writing, a tree may replace only air, water, plants and leaves.

`Terrain::new(settings, Quality)` may do whole-map work up front: erosion, river networks, biome layout. `Terrain::column` must then be cheap, `Sync` and pure. `Quality::Preview` has to answer within about a second for the app.

Blocks are resolved once, in each module's `new`, through `Blocks::id(name, props)`. That validates the state against the user's catalog and interns it in the shared `Palette`. A block missing from the catalog is an error at start-up, never a hole in the world. Write blocks only as `BlockId`s.

The app uses `Generator::preview(res)`, which returns heights, water, colours, biomes and tree positions at low resolution. It uses `Generator::export(saves_dir, …)` with progress and cancel.

## Contract rules

- Contract files are `settings.rs`, `biome.rs`, `column.rs` and `sink.rs`, plus the public signatures of `Terrain`, `Surface`, `Decor`, `Trees` and `Generator`.
  - Adding to them is fine: new fields with defaults, new enum variants at the end, new methods.
  - Renaming, removing, or changing what an existing field means needs the engine owner first.
- `WorldSettings` is saved as JSON by the app. New fields need `#[serde(default)]`.
- Each workstream changes only its own directories. If you need something from another module, ask its owner, or add it additively and say so in the PR.
- Tests run without the user's catalog in CI. Tests that need it read `VISTAFORGE_CATALOG`, or the project copy at `/mnt/project-files/vistaforge/catalog-cr-1.7.0.json`, and skip when it is absent.
- Before a PR, run `cargo fmt`, `cargo clippy --all-targets`, `cargo test` and a real `vistaforge generate`. Then check the result with `tools/check_world.py` and `vistaforge preview`.

## CLI

```
vistaforge presets
vistaforge preview  --catalog <catalog.json> --png out.png [--preset id | --settings file.json] [--size n] [--seed n] [--res px]
vistaforge generate --catalog <catalog.json> --out <saves folder> [--preset id | --settings file.json] [--name n] [--size n] [--seed n] [--overwrite]
vistaforge testworld --catalog <catalog.json> --out <saves folder>     # 256×256 plains
```

## Known facts about the target game

- The owner plays the CR 1.7.0 modpack (Fabric 1.20.1). The catalog from that install is in the project files as `catalog-cr-1.7.0.json` and contains 19,168 blocks.
- Chunks are written with status `minecraft:full`, `isLightOn = 0` and no heightmaps. The game lights them and primes the heightmaps on load, which has been checked in game.
- CR's water texture is darker than vanilla's. Tinted by plains water colour, lakes look very dark navy. Choose Minecraft biomes with lighter water colours for water areas.
- CR layer blocks have `layers` 1–8. Plants (`PlantsDense`) take the `layers` value of the layer they stand on (8 on a full block).
- CR branches connect only to the branch family. `up` is set when the block below is sturdy or a branch. A post on a log pillar is forced to `up=true`, which is a deliberate deviation.

## Workstreams

Each workstream has its own branch and owns its own directories. Every workstream rebases on `main` before opening a PR, and merges its own PR once CI is green.

| Workstream | Branch | Owns |
|---|---|---|
| Engine and integration | `claude/engine-*` | `core/src/engine`, contract files, `anvil/level/nbt/world/catalog/png/noise`, `core/src/main.rs`, root `Cargo.toml` |
| Terrain and presets | `claude/terrain` | `core/src/terrain/**` |
| World detail | `claude/world-detail` | `core/src/surface/**`, `core/src/decor/**` |
| Trees | `claude/trees` | `core/src/trees/**` |
| Desktop app | `claude/app` | `app/**`, `.github/workflows/app.yml` |

**Terrain and presets**
- Realistic land: warped and ridged noise, plus whole-map hydraulic and thermal erosion. Erosion may run coarse and be upsampled, with detail noise added on top.
- Rivers that run downhill into lakes or the sea, and lakes, ponds and swamp pools.
- A biome layout drawn from the biome mix, with blend zones, altitude bands such as the tree line and snow line, and moisture.
- Every `ShapeEdit` kind.
- Fill in every `ColumnInfo` field.
- As many presets as make sense: plains, meadow, forests of each kind, taiga, swamp, marsh, highlands, realistic mountains, alpine valley, river valley, lake district, and mixes of them.

**World detail**
- Curate block sets per biome from the CR 1.7.0 catalog. These cover:
  - grass, clover, moss, podzol, coarse dirt, mud, sand, gravel, stone and snow layers;
  - rock strata on slopes and cliffs;
  - shores and river beds.
- Plants in natural clusters, using the many CR grasses, flowers, ferns, reeds and sedges.
- Rocks and boulders made of CR stone shapes, fallen logs, ground leaves and moss.
- Tree placement per biome: species, density, groves and clearings.
- Water colours via the choice of Minecraft biome.
- Preview colours.

**Trees**
- Species built from CR pieces for each wood type: oak, birch, spruce, pine, willow, dead trees, bushes and young trees.
- Thick trunks from pillars and vertical quarters, branch tips, roots and stumps.
- A size range for every species.
- Side renders of each species as evidence. Stay within `MAX_REACH`.

**Desktop app**
- Tauri 2, a TypeScript UI and a three.js preview. Webkit is missing in the cloud container, so the Tauri crate stays out of the root cargo workspace and builds on Windows CI. The UI is developed against a mocked backend.
- Inputs: the catalog file, the saves folder, presets, size, seed, the biome mix and the sliders.
- A 3D preview, and a 2D map where `ShapeEdit`s are placed by dragging.
- Export with progress and cancel, and save and load of settings JSON.
- English UI.
