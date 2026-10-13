// The flat map picture: preview colours lit from the north-west, water by depth, and tree
// crowns as dark dots. Same look as `vistaforge preview` on the command line.

import { speciesStyle } from './palette';
import type { PreviewData } from './types';

export function shadeMap(p: PreviewData, trees: boolean): ImageData {
  const n = p.res;
  const out = new ImageData(n, n);
  const px = out.data;
  const h = (i: number, j: number) => p.heights[Math.min(n - 1, Math.max(0, j)) * n + Math.min(n - 1, Math.max(0, i))];
  for (let j = 0; j < n; j++) {
    for (let i = 0; i < n; i++) {
      const k = j * n + i;
      const w = p.water[k];
      let r = p.colors[4 * k];
      let g = p.colors[4 * k + 1];
      let b = p.colors[4 * k + 2];
      if (Number.isNaN(w)) {
        const dx = h(i + 1, j) - h(i - 1, j);
        const dz = h(i, j + 1) - h(i, j - 1);
        const light = Math.min(1.35, Math.max(0.55, 1 - ((dx + dz) / p.step) * 0.35));
        r *= light;
        g *= light;
        b *= light;
      } else {
        // Deeper water is darker; the shore stays light.
        const depth = Math.min(1, Math.max(0, (w - p.heights[k]) / 10));
        const mix = 0.55 + 0.35 * depth;
        r = r * (1 - mix) + 38 * mix;
        g = g * (1 - mix) + 92 * mix;
        b = b * (1 - mix) + 138 * mix;
      }
      px[4 * k] = r;
      px[4 * k + 1] = g;
      px[4 * k + 2] = b;
      px[4 * k + 3] = 255;
    }
  }
  if (trees) {
    for (let t = 0; t < p.treeCount; t++) {
      const o = 6 * t;
      const ci = p.trees[o] / p.step;
      const cj = p.trees[o + 2] / p.step;
      const rad = Math.max(0.5, p.trees[o + 4] / p.step);
      const leaves = speciesStyle(p.species[p.trees[o + 5]] ?? '').leaves;
      const lr = parseInt(leaves.slice(1, 3), 16) || 52;
      const lg = parseInt(leaves.slice(3, 5), 16) || 92;
      const lb = parseInt(leaves.slice(5, 7), 16) || 36;
      for (let j = Math.floor(cj - rad); j <= Math.ceil(cj + rad); j++) {
        for (let i = Math.floor(ci - rad); i <= Math.ceil(ci + rad); i++) {
          if (i < 0 || j < 0 || i >= n || j >= n) continue;
          const d = Math.hypot(i + 0.5 - ci, j + 0.5 - cj);
          if (d > rad) continue;
          const k = 4 * (j * n + i);
          const shade = 0.7 + 0.3 * (1 - d / rad);
          px[k] = lr * shade;
          px[k + 1] = lg * shade;
          px[k + 2] = lb * shade;
        }
      }
    }
  }
  return out;
}
