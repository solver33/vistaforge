// Landforms (ShapeEdits) placed on the map: defaults, labels, hit tests and moves.

import type { BiomeId, CircleEdit, EditKind, LineEdit, Point, ShapeEdit } from './types';

export interface KindInfo {
  kind: EditKind;
  label: string;
  /** Keyboard shortcut for the map tool. */
  key: string;
  line: boolean;
  color: string;
  help: string;
}

export const KINDS: KindInfo[] = [
  { kind: 'hill', label: 'Hill', key: 'H', line: false, color: '#8fc06a', help: 'Drag out a rounded rise. A negative height makes a basin.' },
  { kind: 'mountain', label: 'Mountain', key: 'M', line: false, color: '#c8a27a', help: 'Drag out a mountain; its peak rises the given height above the land.' },
  { kind: 'plateau', label: 'Plateau', key: 'P', line: false, color: '#d6c27a', help: 'Drag out an area flattened towards one level.' },
  { kind: 'lake', label: 'Lake', key: 'L', line: false, color: '#5aa0dc', help: 'Drag out a lake.' },
  { kind: 'river', label: 'River', key: 'R', line: true, color: '#4f9fe8', help: 'Click points from the source to the mouth; double-click or Enter to finish.' },
  { kind: 'ridge', label: 'Ridge', key: 'G', line: true, color: '#b49a86', help: 'Click points along a mountain ridge; double-click or Enter to finish.' },
  { kind: 'biome_zone', label: 'Biome zone', key: 'B', line: false, color: '#c58fd6', help: 'Drag out a circle that forces one biome.' },
];

export function kindInfo(kind: EditKind): KindInfo {
  return KINDS.find((k) => k.kind === kind)!;
}

export function isLine(edit: ShapeEdit): edit is LineEdit {
  return 'points' in edit;
}

export function isCircle(edit: ShapeEdit): edit is CircleEdit {
  return 'radius' in edit;
}

/** What a newly placed landform starts with, scaled to the map. */
export interface PlaceContext {
  size: number;
  /** Ground height at the point, if the preview knows it. */
  ground: number | null;
  seaLevel: number;
  biome: BiomeId;
}

export function newCircle(
  kind: Exclude<EditKind, 'river' | 'ridge'>,
  x: number,
  z: number,
  radius: number,
  ctx: PlaceContext,
): CircleEdit {
  const r = Math.round(radius);
  switch (kind) {
    case 'hill':
      return { kind, x, z, radius: r, height: Math.round(Math.max(12, Math.min(48, r * 0.35))) };
    case 'mountain':
      return { kind, x, z, radius: r, height: Math.round(Math.max(60, Math.min(220, r * 0.9))) };
    case 'plateau':
      return { kind, x, z, radius: r, level: Math.round((ctx.ground ?? ctx.seaLevel + 8) + 12) };
    case 'lake':
      return { kind, x, z, radius: r, depth: Math.round(Math.max(4, Math.min(16, r * 0.15))) };
    case 'biome_zone':
      return { kind, x, z, radius: r, biome: ctx.biome };
  }
}

export function newLine(kind: 'river' | 'ridge', points: Point[], size: number): LineEdit {
  const pts = points.map(([x, z]) => [Math.round(x), Math.round(z)] as Point);
  return kind === 'river'
    ? { kind, points: pts, width: Math.round(Math.max(6, Math.min(24, size / 120))), depth: 4 }
    : { kind, points: pts, width: Math.round(Math.max(48, size / 14)), height: 90 };
}

/** The smallest radius a dragged circle gets, so a click still makes something usable. */
export function defaultRadius(kind: EditKind, size: number): number {
  const share = kind === 'mountain' ? 0.08 : kind === 'biome_zone' ? 0.1 : kind === 'hill' ? 0.04 : 0.05;
  return Math.max(16, Math.round(size * share));
}

/** One line describing an edit's parameters, for lists. */
export function describe(edit: ShapeEdit, biomeName: (id: BiomeId) => string): string {
  switch (edit.kind) {
    case 'hill':
      return `${edit.height >= 0 ? '+' : ''}${edit.height} high, r ${edit.radius}`;
    case 'mountain':
      return `${edit.height} high, r ${edit.radius}`;
    case 'plateau':
      return `level Y ${edit.level}, r ${edit.radius}`;
    case 'lake':
      return `${edit.depth} deep, r ${edit.radius}`;
    case 'river':
      return `${edit.points.length} points, ${edit.width} wide`;
    case 'ridge':
      return `${edit.points.length} points, ${edit.height} high`;
    case 'biome_zone':
      return `${biomeName(edit.biome)}, r ${edit.radius}`;
  }
}

export function distToSegment(px: number, pz: number, a: Point, b: Point): number {
  const [ax, az] = a;
  const dx = b[0] - ax;
  const dz = b[1] - az;
  const len2 = dx * dx + dz * dz;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((px - ax) * dx + (pz - az) * dz) / len2));
  return Math.hypot(px - (ax + t * dx), pz - (az + t * dz));
}

export function distToLine(px: number, pz: number, points: Point[]): number {
  if (points.length === 1) return Math.hypot(px - points[0][0], pz - points[0][1]);
  let best = Infinity;
  for (let i = 1; i < points.length; i++) {
    best = Math.min(best, distToSegment(px, pz, points[i - 1], points[i]));
  }
  return best;
}

/** Which part of an edit is under a point. */
export type Part = { kind: 'body' } | { kind: 'radius' } | { kind: 'vertex'; index: number };

export interface Hit {
  index: number;
  part: Part;
}

/**
 * Finds what is under (x, z). `tol` is the pick tolerance in blocks. Handles of the selected
 * edit win, then the topmost (last) edit whose shape contains the point.
 */
export function hitTest(edits: ShapeEdit[], x: number, z: number, tol: number, selected: number | null): Hit | null {
  if (selected !== null && edits[selected]) {
    const e = edits[selected];
    if (isLine(e)) {
      const v = e.points.findIndex(([px, pz]) => Math.hypot(px - x, pz - z) <= tol);
      if (v >= 0) return { index: selected, part: { kind: 'vertex', index: v } };
    } else {
      if (Math.hypot(e.x + e.radius - x, e.z - z) <= tol) return { index: selected, part: { kind: 'radius' } };
    }
  }
  for (let i = edits.length - 1; i >= 0; i--) {
    const e = edits[i];
    if (isLine(e)) {
      if (distToLine(x, z, e.points) <= Math.max(tol, e.width / 2)) return { index: i, part: { kind: 'body' } };
    } else if (Math.hypot(e.x - x, e.z - z) <= Math.max(e.radius, tol)) {
      return { index: i, part: { kind: 'body' } };
    }
  }
  return null;
}

/** A copy of `edit` moved by (dx, dz). */
export function moved(edit: ShapeEdit, dx: number, dz: number): ShapeEdit {
  if (isLine(edit)) {
    return { ...edit, points: edit.points.map(([x, z]) => [x + dx, z + dz] as Point) };
  }
  return { ...edit, x: edit.x + dx, z: edit.z + dz };
}

/** Rounds positions to whole blocks, as they are saved. */
export function rounded(edit: ShapeEdit): ShapeEdit {
  if (isLine(edit)) {
    return { ...edit, points: edit.points.map(([x, z]) => [Math.round(x), Math.round(z)] as Point) };
  }
  return { ...edit, x: Math.round(edit.x), z: Math.round(edit.z), radius: Math.max(1, Math.round(edit.radius)) };
}
