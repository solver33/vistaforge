// Limits that depend on how tall the world is (core/src/settings.rs `WorldHeight`). A tall world
// lets the ground reach Y 2031, so landforms may be far higher than in a vanilla-height world.

import type { ShapeEdit, WorldHeight, WorldSettings } from './types';

export interface HeightLimits {
  /** Highest Y a block can be at. */
  top: number;
  seaLevel: number;
  /** Highest value of each landform's height field. */
  hill: number;
  mountain: number;
  plateau: number;
  ridge: number;
}

export const WORLD_HEIGHTS: { id: WorldHeight; label: string; top: number; help: string }[] = [
  { id: 'standard', label: 'Standard', top: 319, help: 'Up to Y 319, like any Minecraft world.' },
  {
    id: 'tall',
    label: 'Tall',
    top: 2031,
    help: 'Up to Y 2031 for huge mountains. The world carries a small data pack for this, so Minecraft warns about experimental settings when you open it.',
  },
];

const STANDARD: HeightLimits = { top: 319, seaLevel: 160, hill: 200, mountain: 320, plateau: 300, ridge: 320 };
const TALL: HeightLimits = { top: 2031, seaLevel: 300, hill: 1000, mountain: 1900, plateau: 2000, ridge: 1500 };

export function heightLimits(h: WorldHeight | undefined): HeightLimits {
  return h === 'tall' ? TALL : STANDARD;
}

/** Pulls the sea level and landform heights back inside the limits of `s.world_height`. */
export function clampHeights(s: WorldSettings): void {
  const l = heightLimits(s.world_height);
  s.sea_level = Math.min(s.sea_level, l.seaLevel);
  s.edits = s.edits.map((e): ShapeEdit => {
    switch (e.kind) {
      case 'hill':
        return { ...e, height: Math.min(e.height, l.hill) };
      case 'mountain':
        return { ...e, height: Math.min(e.height, l.mountain) };
      case 'plateau':
        return { ...e, level: Math.min(e.level, l.plateau) };
      case 'ridge':
        return { ...e, height: Math.min(e.height, l.ridge) };
      default:
        return e;
    }
  });
}
