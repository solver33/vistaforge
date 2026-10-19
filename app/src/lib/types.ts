// Mirrors of the core's JSON types (core/src/settings.rs, core/src/engine, app/bridge). Field
// names are the core's snake_case names. Objects from the core may carry fields added later;
// the UI keeps them untouched and sends them back.

export type BiomeId = string;
export type SpeciesId = string;

export interface BiomeShare {
  biome: BiomeId;
  /** Relative weight; shown as a percentage of the total. */
  share: number;
}

/** Each 0..1, with 0.5 as the preset's "normal". Keys are the core's slider names. */
export type Sliders = Record<string, number>;

export type Point = [number, number];

export type ShapeEdit =
  | { kind: 'hill'; x: number; z: number; radius: number; height: number }
  | { kind: 'mountain'; x: number; z: number; radius: number; height: number }
  | { kind: 'plateau'; x: number; z: number; radius: number; level: number }
  | { kind: 'lake'; x: number; z: number; radius: number; depth: number }
  | { kind: 'river'; points: Point[]; width: number; depth: number }
  | { kind: 'ridge'; points: Point[]; width: number; height: number }
  | { kind: 'biome_zone'; x: number; z: number; radius: number; biome: BiomeId }
  | { kind: 'road'; points: Point[]; width: number }
  /** At the lip; `radius` is half the width of the falling water, `angle` the flow in degrees (0 east, 90 south). */
  | { kind: 'waterfall'; x: number; z: number; radius: number; drop: number; angle: number };

export type EditKind = ShapeEdit['kind'];
export type CircleEdit = Extract<ShapeEdit, { radius: number }>;
export type LineEdit = Extract<ShapeEdit, { points: Point[] }>;
export type Waterfall = Extract<ShapeEdit, { kind: 'waterfall' }>;

/** How far up the world goes; the bottom is always Y -64. */
export type WorldHeight = 'standard' | 'tall';

export interface WorldSettings {
  name: string;
  size: number;
  seed: number;
  preset: string;
  biomes: BiomeShare[];
  sliders: Sliders;
  sea_level: number;
  edits: ShapeEdit[];
  world_height: WorldHeight;
  [later: string]: unknown;
}

export interface Preset {
  id: string;
  name: string;
  description: string;
  biomes: BiomeShare[];
  sliders: Sliders;
  sea_level: number;
  /** Set on presets made for one world height (the tall ones); others keep the user's choice. */
  world_height?: WorldHeight;
}

export interface BiomeInfo {
  id: BiomeId;
  name: string;
  /** Made by the terrain (beaches, rivers, lakes) rather than chosen in the mix. */
  feature: boolean;
}

export interface AppInfo {
  version: string;
  presets: Preset[];
  biomes: BiomeInfo[];
  min_size: number;
  max_size: number;
  defaults: WorldSettings;
}

export interface CatalogInfo {
  path: string;
  blocks: number;
  mods: Record<string, string | null>;
  seconds: number;
}

/** A Minecraft game folder found on this computer. */
export interface Instance {
  launcher: string;
  name: string;
  game_dir: string;
  saves: string | null;
  catalog: string | null;
}

export interface Progress {
  chunks_done: number;
  chunks_total: number;
}

export interface ExportReport {
  world_dir: string;
  chunks: number;
  regions: number;
  block_states: number;
  trees: number;
  seconds: number;
  spawn: [number, number, number];
}

export type ExportOutcome = { status: 'done'; report: ExportReport } | { status: 'cancelled' };

export interface AppConfig {
  catalog: string | null;
  saves_dir: string | null;
  settings: WorldSettings | null;
  ui: Record<string, unknown> | null;
}

/** A decoded `Generator::preview` (see decode.ts). Sample (i, j) is row-major, z then x. */
export interface PreviewData {
  size: number;
  res: number;
  /** Blocks between samples. */
  step: number;
  heights: Float32Array;
  /** Water surface Y, NaN where dry. */
  water: Float32Array;
  /** RGBA, 4 bytes per sample. */
  colors: Uint8Array;
  /** Index into `biomeIds` per sample. */
  biomes: Uint8Array;
  biomeIds: BiomeId[];
  /** 6 floats per tree: x, y, z, height, crown, species index into `species`. */
  trees: Float32Array;
  treeCount: number;
  /** Trees on the whole map; more than `treeCount` when the bridge thinned a dense forest. */
  totalTrees: number;
  species: SpeciesId[];
}

/** A Minecraft world opened to look at (app/bridge/src/world). Coordinates are the game's. */
export interface WorldInfo {
  path: string;
  name: string;
  /** The Minecraft version that last saved it. */
  version: string | null;
  data_version: number | null;
  /** `overworld`, `the_nether` or `the_end`. */
  dimension: string;
  /** Dimensions with saved chunks. */
  dimensions: string[];
  spawn: [number, number, number];
  /** Where the player was when the world was last saved, when in this dimension. */
  player: [number, number, number] | null;
  regions: number;
  chunks: number;
  /** Blocks covered by saved chunks: x0, z0, x1, z1 (the far edges exclusive). */
  bounds: [number, number, number, number] | null;
  seconds: number;
}

/** The picture of every saved chunk of an opened world (decoded by worldmap.ts). */
export interface WorldMapData {
  /** Block at the north-west corner. */
  x0: number;
  z0: number;
  /** Blocks per pixel. */
  scale: number;
  width: number;
  height: number;
  chunks: number;
  /** Chunks saved by Minecraft before 1.18; the game updates them when they are visited. */
  old_chunks: number;
  /** Chunks at the edge of the explored world the game had not finished. */
  unfinished: number;
  failed: number;
  seconds: number;
  /** RGBA, alpha 0 where nothing was saved. */
  rgba: Uint8ClampedArray;
}
