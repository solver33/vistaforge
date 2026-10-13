# Vistaforge

A personal Windows tool for generating square Minecraft 1.20.1 (Fabric) maps built from
[Conquest Reforged](https://www.curseforge.com/minecraft/mc-mods/conquest-reforged) blocks:
terrain presets, biome blending, procedural trees made from the mod's tree pieces, and a 3D preview.

The app never ships or reads Conquest Reforged files. It only needs block ids and block states,
which the exporter mod below dumps from your own modpack.

## Core

`core/` is the Rust library and command-line tool that turns the catalog into a Minecraft 1.20.1
world: a block-state catalog loader, an NBT and Anvil region writer, a `level.dat` writer with an
empty (void) superflat generator around the map, and the generators.

It generates maps up to 4096×4096 by streaming 64×64-block pieces to region files on all CPU
cores. How the modules fit together, and the rules each one follows, are in
[docs/architecture.md](docs/architecture.md).

```
cargo run --release -- presets
cargo run --release -- preview  --catalog path/to/catalog.json --png map.png --size 1024
cargo run --release -- generate --catalog path/to/catalog.json --out out --size 1024 --name "My Map"
cargo run --release -- testworld --catalog path/to/catalog.json --out out
```

`generate` writes `out/<name>` (`testworld` writes a 256×256 `out/Vistaforge Test`). Copy that
folder into the game's `saves` folder. The world is creative, with a fixed midday and no mobs.
Chunks are saved unlit, so the game computes lighting on first load.

`tools/check_world.py` reads a generated world back with an independent NBT parser (nbtlib),
checks every block state against the catalog and draws a top-down preview (needs nbtlib and Pillow).

## Exporter mod

`exporter/` is a small client-side Fabric mod for Minecraft 1.20.1. Once per launch, after the game
has finished loading, it writes `vistaforge/catalog.json` into the game folder with:

- every registered block id (vanilla, Conquest Reforged and any other mod);
- each block's state properties, their allowed values and the default state;
- the block's runtime class and class hierarchy, map colour and a few flags (air, opaque, liquid);
- average texture colours of the default model's top, side and particle textures, with tint info,
  for the map preview;
- versions of the mods that own the block namespaces.

It changes nothing in the game and has no commands or settings. Remove it from `mods/` when you
don't need a fresh catalog.

### Use

1. Put `vistaforge-exporter-<version>.jar` into the modpack's `mods` folder
   (CurseForge: right-click the profile, then **Open Folder**).
2. Start the game and wait for the title screen.
3. Look for `vistaforge/catalog.json` in the profile folder. The log line starting with
   `[Vistaforge]` in `logs/latest.log` reports how many blocks were exported.

### Build

```
cd exporter
./gradlew build
```

The jar lands in `exporter/build/libs/`. CI builds it on every change to `exporter/` and attaches
it to the workflow run.

The mod does not compile against Minecraft: it reaches the game through reflection using
intermediary names (checked against Yarn 1.20.1), and calls Fabric API's tick event reflectively too.
The few Fabric Loader and SLF4J interfaces it uses are compile-only stubs in `src/stubs`, so the
build needs nothing but a JDK.
