// A stand-in for `Generator::preview` so the UI can be built and tested without the core.
// It only has to look plausible and react to every setting; the real terrain lives in
// core/src/terrain.

import { biomeColor } from '../palette';
import { distToLine, isLine } from '../edits';
import type { BiomeShare, PreviewData, WorldSettings } from '../types';

interface Profile {
  base: number;
  amp: number;
  ridged: number;
  trees: number;
  species: string[];
}

const PROFILES: Record<string, Profile> = {
  plains: { base: 5, amp: 5, ridged: 0, trees: 0.03, species: ['oak', 'oak', 'bush'] },
  meadow: { base: 7, amp: 7, ridged: 0, trees: 0.015, species: ['oak', 'birch', 'bush'] },
  forest: { base: 9, amp: 11, ridged: 0, trees: 0.5, species: ['oak', 'oak', 'birch', 'bush'] },
  birch_forest: { base: 9, amp: 9, ridged: 0, trees: 0.45, species: ['birch', 'birch', 'oak'] },
  taiga: { base: 14, amp: 16, ridged: 0, trees: 0.5, species: ['spruce', 'spruce', 'pine'] },
  pine_forest: { base: 16, amp: 14, ridged: 0, trees: 0.32, species: ['pine', 'pine', 'spruce', 'dead'] },
  swamp: { base: 0.7, amp: 1.4, ridged: 0, trees: 0.28, species: ['willow', 'willow', 'oak', 'dead'] },
  marsh: { base: 0.5, amp: 1.0, ridged: 0, trees: 0.02, species: ['bush', 'willow'] },
  highlands: { base: 26, amp: 24, ridged: 0.3, trees: 0.03, species: ['pine', 'bush', 'dead'] },
  mountains: { base: 45, amp: 60, ridged: 1, trees: 0.05, species: ['spruce', 'pine'] },
  alpine_meadow: { base: 58, amp: 34, ridged: 0.4, trees: 0.01, species: ['spruce', 'bush'] },
  snowy_peaks: { base: 90, amp: 80, ridged: 1, trees: 0, species: [] },
};
const FALLBACK: Profile = { base: 8, amp: 8, ridged: 0, trees: 0.1, species: ['oak'] };

const TREE_SIZE: Record<string, [number, number, number]> = {
  // min height, max height, crown radius as a share of height
  oak: [10, 20, 0.36],
  birch: [12, 20, 0.24],
  spruce: [14, 28, 0.22],
  pine: [14, 24, 0.2],
  willow: [9, 14, 0.5],
  dead: [6, 12, 0.15],
  bush: [2, 3, 0.9],
};

function mix32(x: number): number {
  x = Math.imul(x ^ (x >>> 16), 0x7feb352d);
  x = Math.imul(x ^ (x >>> 15), 0x846ca68b);
  return (x ^ (x >>> 16)) >>> 0;
}

/** A hash of (seed, x, z) in [0, 1). */
function hash(seed: number, x: number, z: number): number {
  return mix32(seed ^ mix32(x * 0x27d4eb2d + mix32(z * 0x165667b1))) / 4294967296;
}

const smooth = (t: number) => t * t * (3 - 2 * t);

function valueNoise(seed: number, x: number, z: number): number {
  const xi = Math.floor(x);
  const zi = Math.floor(z);
  const tx = smooth(x - xi);
  const tz = smooth(z - zi);
  const a = hash(seed, xi, zi);
  const b = hash(seed, xi + 1, zi);
  const c = hash(seed, xi, zi + 1);
  const d = hash(seed, xi + 1, zi + 1);
  return a + (b - a) * tx + (c - a) * tz + (a - b - c + d) * tx * tz;
}

function fbm(seed: number, x: number, z: number, octaves: number): number {
  let sum = 0;
  let amp = 0.5;
  let norm = 0;
  for (let o = 0; o < octaves; o++) {
    sum += amp * valueNoise(seed + o * 1013, x, z);
    norm += amp;
    x *= 2.03;
    z *= 2.03;
    amp *= 0.5;
  }
  return sum / norm;
}

function ridged(seed: number, x: number, z: number, octaves: number): number {
  let sum = 0;
  let amp = 0.5;
  let norm = 0;
  for (let o = 0; o < octaves; o++) {
    const n = 1 - Math.abs(valueNoise(seed + o * 7919, x, z) * 2 - 1);
    sum += amp * n * n;
    norm += amp;
    x *= 2.1;
    z *= 2.1;
    amp *= 0.5;
  }
  return sum / norm;
}

function pickBiome(mix: BiomeShare[], r: number): string {
  let acc = 0;
  for (const b of mix) {
    acc += b.share;
    if (r < acc) return b.biome;
  }
  return mix[mix.length - 1].biome;
}

function hexToRgb(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

export function mockPreview(settings: WorldSettings, res: number, biomeIds: string[]): PreviewData {
  const size = settings.size;
  res = Math.max(16, Math.min(res, size));
  const step = size / res;
  const n = res * res;
  const seed = (settings.seed % 4294967296 ^ Math.floor(settings.seed / 4294967296)) >>> 0;
  const s = (k: string) => settings.sliders[k] ?? 0.5;
  const sea = settings.sea_level;

  const total = settings.biomes.reduce((t, b) => t + Math.max(0, b.share), 0) || 1;
  const mix = settings.biomes.filter((b) => b.share > 0).map((b) => ({ biome: b.biome, share: b.share / total }));
  if (mix.length === 0) mix.push({ biome: 'plains', share: 1 });

  // Biome regions: jittered cells, blended by distance for smooth heights.
  const cell = 120 + 900 * s('biome_scale');
  const blendWidth = 0.12 + 0.5 * s('biome_blend');
  const cellBiome = (cx: number, cz: number) => pickBiome(mix, hash(seed ^ 0xb10e, cx, cz));
  const cellCentre = (cx: number, cz: number): [number, number] => [
    (cx + 0.15 + 0.7 * hash(seed ^ 0x11, cx, cz)) * cell,
    (cz + 0.15 + 0.7 * hash(seed ^ 0x22, cx, cz)) * cell,
  ];

  const heights = new Float32Array(n);
  const water = new Float32Array(n).fill(NaN);
  const biomeOf: string[] = new Array(n);
  const treeChance = new Float32Array(n);
  const species: string[][] = new Array(n);

  for (let j = 0; j < res; j++) {
    for (let i = 0; i < res; i++) {
      const k = j * res + i;
      const x = (i + 0.5) * step;
      const z = (j + 0.5) * step;
      const wx = x + 60 * (fbm(seed ^ 0x77, x / 300, z / 300, 2) - 0.5);
      const wz = z + 60 * (fbm(seed ^ 0x78, x / 300, z / 300, 2) - 0.5);
      const cx = Math.floor(wx / cell);
      const cz = Math.floor(wz / cell);
      let best = Infinity;
      let biome = 'plains';
      let wsum = 0;
      let base = 0;
      let amp = 0;
      let ridge = 0;
      let trees = 0;
      for (let dz = -1; dz <= 1; dz++) {
        for (let dx = -1; dx <= 1; dx++) {
          const [px, pz] = cellCentre(cx + dx, cz + dz);
          const d = Math.hypot(px - wx, pz - wz) / cell;
          const b = cellBiome(cx + dx, cz + dz);
          if (d < best) {
            best = d;
            biome = b;
          }
          const w = Math.exp(-(d * d) / (blendWidth * blendWidth));
          const p = PROFILES[b] ?? FALLBACK;
          wsum += w;
          base += w * p.base;
          amp += w * p.amp;
          ridge += w * p.ridged;
          trees += w * p.trees;
        }
      }
      base /= wsum;
      amp /= wsum;
      ridge /= wsum;
      trees /= wsum;
      const relief = 0.3 + 1.4 * s('relief');
      let h =
        sea +
        2 +
        base * relief +
        amp * relief * (fbm(seed, x / 260, z / 260, 5) - 0.35) * 1.6 +
        ridge * (0.4 + 1.2 * s('mountains')) * 110 * ridged(seed ^ 0x5a, x / 420, z / 420, 5) +
        (0.5 + 3 * s('roughness')) * (fbm(seed ^ 0x99, x / 14, z / 14, 2) - 0.5);
      // Lakes: low spots of a broad noise, more with the water slider.
      const lake = fbm(seed ^ 0x1a4e, x / 220, z / 220, 3);
      const lakeCut = 0.22 + 0.25 * s('water');
      if (lake < lakeCut && ridge < 0.5) h = Math.min(h, sea - 1 - 20 * (lakeCut - lake));
      biomeOf[k] = biome;
      heights[k] = h;
      treeChance[k] = trees * 2 * s('trees');
      species[k] = (PROFILES[biome] ?? FALLBACK).species;
    }
  }

  // Hand-placed landforms.
  for (const e of settings.edits) {
    for (let j = 0; j < res; j++) {
      for (let i = 0; i < res; i++) {
        const k = j * res + i;
        const x = (i + 0.5) * step;
        const z = (j + 0.5) * step;
        if (isLine(e)) {
          const d = distToLine(x, z, e.points);
          const half = e.width / 2;
          if (e.kind === 'ridge' && d < half) {
            const t = 1 - d / half;
            heights[k] += e.height * t * t * (0.75 + 0.5 * ridged(seed ^ 0x3c, x / 60, z / 60, 3));
          } else if (e.kind === 'river' && d < half + 6) {
            const level = Math.min(heights[k], sea + 6) - 1;
            if (d < half) {
              heights[k] = level - e.depth * (1 - (d / half) ** 2);
              water[k] = level + 1;
              biomeOf[k] = 'river';
              treeChance[k] = 0;
            } else {
              heights[k] = Math.min(heights[k], level + (d - half) * 0.8);
            }
          }
        } else {
          const d = Math.hypot(x - e.x, z - e.z) / e.radius;
          if (d >= 1) continue;
          const t = smooth(1 - d);
          switch (e.kind) {
            case 'hill':
              heights[k] += e.height * t;
              break;
            case 'mountain':
              heights[k] += e.height * Math.pow(1 - d, 1.6) * (0.8 + 0.4 * ridged(seed ^ 0x4d, x / 50, z / 50, 3));
              break;
            case 'plateau':
              heights[k] += (e.level - heights[k]) * Math.min(1, t * 1.8);
              break;
            case 'lake': {
              const level = sea;
              heights[k] = Math.min(heights[k], level - e.depth * (1 - d * d)) * t + heights[k] * (1 - t);
              if (heights[k] < level) water[k] = level + 1;
              break;
            }
            case 'biome_zone':
              biomeOf[k] = e.biome;
              species[k] = (PROFILES[e.biome] ?? FALLBACK).species;
              treeChance[k] = (PROFILES[e.biome] ?? FALLBACK).trees * 2 * s('trees');
              break;
          }
        }
      }
    }
  }

  const colors = new Uint8Array(4 * n);
  const biomeIndex = new Uint8Array(n);
  const indexOf = new Map(biomeIds.map((id, i) => [id, i]));
  const usedIds: string[] = [];
  const used = new Map<string, number>();
  const idx = (id: string) => {
    let v = used.get(id);
    if (v === undefined) {
      v = usedIds.length;
      usedIds.push(id);
      used.set(id, v);
    }
    return v;
  };
  for (let j = 0; j < res; j++) {
    for (let i = 0; i < res; i++) {
      const k = j * res + i;
      const h = heights[k];
      if (Number.isNaN(water[k]) && h < sea + 1) {
        water[k] = sea + 1;
        if (biomeOf[k] !== 'river') biomeOf[k] = indexOf.has('lake') ? 'lake' : biomeOf[k];
      }
      const right = heights[j * res + Math.min(i + 1, res - 1)];
      const down = heights[Math.min(j + 1, res - 1) * res + i];
      const slope = Math.hypot(right - h, down - h) / step;
      let [r, g, b] = hexToRgb(biomeColor(biomeOf[k]).startsWith('#') ? biomeColor(biomeOf[k]) : '#7f9f5f');
      const wet = !Number.isNaN(water[k]);
      if (!wet && h < sea + 2.5 && biomeOf[k] !== 'swamp' && biomeOf[k] !== 'marsh') {
        [r, g, b] = [214, 199, 150];
        biomeOf[k] = indexOf.has('beach') ? 'beach' : biomeOf[k];
      }
      if (slope > 0.9) {
        const t = Math.min(1, (slope - 0.9) * 1.5);
        r += (124 - r) * t;
        g += (120 - g) * t;
        b += (112 - b) * t;
      }
      if (h > 175) {
        const t = Math.min(1, (h - 175) / 20);
        r += (238 - r) * t;
        g += (242 - g) * t;
        b += (245 - b) * t;
      }
      if (wet) {
        const depth = Math.min(1, (water[k] - h) / 8);
        r = 60 + 30 * (1 - depth);
        g = 100 + 40 * (1 - depth);
        b = 150 + 30 * (1 - depth);
      }
      const v = 0.9 + 0.2 * hash(seed ^ 0xc0, i, j);
      colors[4 * k] = Math.min(255, r * v);
      colors[4 * k + 1] = Math.min(255, g * v);
      colors[4 * k + 2] = Math.min(255, b * v);
      colors[4 * k + 3] = 255;
      biomeIndex[k] = idx(biomeOf[k]);
      if (wet || slope > 1.2) treeChance[k] = 0;
    }
  }

  // Trees on a jittered grid.
  const treeCell = Math.max(5, Math.round(size / 400));
  const out: number[] = [];
  const speciesIds: string[] = [];
  for (let gz = 0; gz < size / treeCell; gz++) {
    for (let gx = 0; gx < size / treeCell; gx++) {
      const x = (gx + hash(seed ^ 0x7e, gx, gz)) * treeCell;
      const z = (gz + hash(seed ^ 0x7f, gx, gz)) * treeCell;
      const i = Math.min(res - 1, Math.floor(x / step));
      const j = Math.min(res - 1, Math.floor(z / step));
      const k = j * res + i;
      if (hash(seed ^ 0x80, gx, gz) >= treeChance[k] * (0.6 + 0.8 * fbm(seed ^ 0x81, x / 90, z / 90, 2))) continue;
      const list = species[k];
      if (!list || list.length === 0) continue;
      const sp = list[Math.floor(hash(seed ^ 0x82, gx, gz) * list.length)];
      const [lo, hi, crown] = TREE_SIZE[sp] ?? [8, 16, 0.3];
      const height = lo + (hi - lo) * hash(seed ^ 0x83, gx, gz);
      let si = speciesIds.indexOf(sp);
      if (si < 0) si = speciesIds.push(sp) - 1;
      out.push(x, Math.floor(heights[k]) + 1, z, height, Math.max(1, height * crown), si);
    }
  }

  return {
    size,
    res,
    step,
    heights,
    water,
    colors,
    biomes: biomeIndex,
    biomeIds: usedIds,
    trees: new Float32Array(out),
    treeCount: out.length / 6,
    species: speciesIds,
  };
}
