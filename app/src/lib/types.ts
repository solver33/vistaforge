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
  | { kind: 'biome_zone'; x: number; z: number; radius: number; biome: BiomeId };

export type EditKind = ShapeEdit['kind'];
export type CircleEdit = Extract<ShapeEdit, { radius: number }>;
export type LineEdit = Extract<ShapeEdit, { points: Point[] }>;

export interface WorldSettings {
  name: string;
  size: number;
  seed: number;
  preset: string;
  biomes: BiomeShare[];
  sliders: Sliders;
  sea_level: number;
  edits: ShapeEdit[];
  [later: string]: unknown;
}

export interface Preset {
  id: string;
  name: string;
  description: string;
  biomes: BiomeShare[];
  sliders: Sliders;
  sea_level: number;
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
  species: SpeciesId[];
}
