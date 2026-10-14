// Landforms (ShapeEdits) placed on the map: defaults, labels, hit tests and moves.

import type { BiomeId, CircleEdit, EditKind, LineEdit, Point, ShapeEdit, Waterfall } from './types';

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
  {
    kind: 'road',
    label: 'Road',
    key: 'D',
    line: true,
    color: '#b08a5a',
    help: 'Drag to draw a road, or click points and double-click or press Enter to finish. The ground beside it is levelled.',
  },
  {
    kind: 'waterfall',
    label: 'Waterfall',
    key: 'W',
    line: false,
    color: '#7cc8ec',
    help: 'Click where the water should fall, or drag the way it flows. A stream falls the given drop into a pool.',
  },
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

export function isWaterfall(edit: ShapeEdit): edit is Waterfall {
  return edit.kind === 'waterfall';
}

export type LineKind = LineEdit['kind'];
/** Landforms dragged out as a circle. */
export type CircleKind = Exclude<CircleEdit['kind'], 'waterfall'>;

export function isLineKind(kind: EditKind): kind is LineKind {
  return kind === 'river' || kind === 'ridge' || kind === 'road';
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
  kind: CircleKind,
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

export function newLine(kind: LineKind, points: Point[], size: number): LineEdit {
  const pts = points.map(([x, z]) => [Math.round(x), Math.round(z)] as Point);
  switch (kind) {
    case 'river':
      return { kind, points: pts, width: Math.round(Math.max(6, Math.min(24, size / 120))), depth: 4 };
    case 'ridge':
      return { kind, points: pts, width: Math.round(Math.max(48, size / 14)), height: 90 };
    case 'road':
      return { kind, points: pts, width: 5 };
  }
}

/** A waterfall at its lip, flowing towards `angle` degrees. */
export function newWaterfall(x: number, z: number, angle: number, drop: number): Waterfall {
  return { kind: 'waterfall', x, z, radius: 3, drop: Math.round(drop), angle: Math.round(((angle % 360) + 360) % 360) };
}

/** Flow direction of a waterfall, as a unit vector. */
export function flow(e: Waterfall): Point {
  const a = (e.angle * Math.PI) / 180;
  return [Math.cos(a), Math.sin(a)];
}

/**
 * Where a waterfall's stream starts (its spring) and its pool lies, and the pool's radius, as
 * the core lays them out (core/src/terrain/brushes/waterfall.rs).
 */
export function waterfallShape(e: Waterfall): { spring: Point; pool: Point; poolRadius: number } {
  const half = Math.max(1, Math.min(20, e.radius));
  const drop = Math.max(3, e.drop);
  const pool = Math.min(half + 2.5 + 0.12 * drop, half + 14);
  const run = Math.max(14, Math.min(64, 3 * half + 12 + 0.2 * drop));
  const [ux, uz] = flow(e);
  return { spring: [e.x - ux * run, e.z - uz * run], pool: [e.x + ux * pool, e.z + uz * pool], poolRadius: pool };
}

/** The handle that turns a waterfall: downstream past its pool, at least `4 * tol` from the lip. */
export function waterfallHandle(e: Waterfall, tol: number): Point {
  const { poolRadius } = waterfallShape(e);
  const [ux, uz] = flow(e);
  const d = Math.max(2 * poolRadius + 4, 4 * tol);
  return [e.x + ux * d, e.z + uz * d];
}

/** The angle, in whole degrees 0 to 359, of the direction from (x0, z0) to (x1, z1). */
export function angleTo(x0: number, z0: number, x1: number, z1: number): number {
  const a = Math.round((Math.atan2(z1 - z0, x1 - x0) * 180) / Math.PI);
  return (a + 360) % 360;
}

/** Fewer points along a freehand stroke, none of them moving the line more than `tol` blocks. */
export function simplify(points: Point[], tol: number): Point[] {
  if (points.length <= 2) return points.slice();
  const keep = new Array(points.length).fill(false);
  keep[0] = keep[points.length - 1] = true;
  const stack: [number, number][] = [[0, points.length - 1]];
  while (stack.length) {
    const [a, b] = stack.pop()!;
    let worst = -1;
    let at = -1;
    for (let i = a + 1; i < b; i++) {
      const d = distToSegment(points[i][0], points[i][1], points[a], points[b]);
      if (d > worst) {
        worst = d;
        at = i;
      }
    }
    if (worst > tol) {
      keep[at] = true;
      stack.push([a, at], [at, b]);
    }
  }
  return points.filter((_, i) => keep[i]);
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
    case 'road':
      return `${edit.points.length} points, ${edit.width} wide`;
    case 'waterfall':
      return `${edit.drop} drop, r ${edit.radius}`;
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
export type Part = { kind: 'body' } | { kind: 'radius' } | { kind: 'angle' } | { kind: 'vertex'; index: number };

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
    } else if (isWaterfall(e)) {
      const [hx, hz] = waterfallHandle(e, tol);
      if (Math.hypot(hx - x, hz - z) <= tol) return { index: selected, part: { kind: 'angle' } };
    } else {
      if (Math.hypot(e.x + e.radius - x, e.z - z) <= tol) return { index: selected, part: { kind: 'radius' } };
    }
  }
  for (let i = edits.length - 1; i >= 0; i--) {
    const e = edits[i];
    if (isLine(e)) {
      if (distToLine(x, z, e.points) <= Math.max(tol, e.width / 2)) return { index: i, part: { kind: 'body' } };
    } else if (isWaterfall(e)) {
      const { spring, pool, poolRadius } = waterfallShape(e);
      const stream = distToSegment(x, z, spring, [e.x, e.z]) <= Math.max(tol, e.radius + 2);
      if (stream || Math.hypot(pool[0] - x, pool[1] - z) <= Math.max(tol, poolRadius)) {
        return { index: i, part: { kind: 'body' } };
      }
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
  const out = { ...edit, x: Math.round(edit.x), z: Math.round(edit.z), radius: Math.max(1, Math.round(edit.radius)) };
  if (out.kind === 'waterfall') out.angle = (Math.round(out.angle) + 360) % 360;
  return out;
}
