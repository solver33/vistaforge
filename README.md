# Vistaforge

A personal Windows tool for generating square Minecraft 1.20.1 (Fabric) maps built from
[Conquest Reforged](https://www.curseforge.com/minecraft/mc-mods/conquest-reforged) blocks:
terrain presets, biome blending, procedural trees made from the mod's tree pieces, and a 3D preview.

The app never ships or reads Conquest Reforged files. It only needs block ids and block states,
which the exporter mod below dumps from your own modpack.

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
