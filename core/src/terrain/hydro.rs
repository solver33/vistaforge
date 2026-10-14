//! Water on the macro grid: where rain runs (priority flood), how much land drains through
//! each node, which hollows hold lakes, and which low ground is open sea.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// No receiver: the node drains off the map or into the sea.
pub const NONE: u32 = u32::MAX;

/// 8 neighbours as (di, dj, distance in nodes).
pub const NEIGHBOURS: [(i32, i32, f32); 8] = [
    (1, 0, 1.0),
    (-1, 0, 1.0),
    (0, 1, 1.0),
    (0, -1, 1.0),
    (1, 1, std::f32::consts::SQRT_2),
    (-1, 1, std::f32::consts::SQRT_2),
    (1, -1, std::f32::consts::SQRT_2),
    (-1, -1, std::f32::consts::SQRT_2),
];

pub struct Flood {
    /// Height with every hollow filled to its spill level.
    pub filled: Vec<f32>,
    /// The node each node drains into.
    pub recv: Vec<u32>,
    /// Nodes in the order they were reached; every node comes after its receiver.
    pub order: Vec<u32>,
}

/// Orders f32 values as u32 (for heap keys).
#[inline]
fn key(v: f32) -> u32 {
    let b = v.to_bits();
    if b & 0x8000_0000 != 0 {
        !b
    } else {
        b | 0x8000_0000
    }
}

/// Priority flood (Barnes et al.) from `seeds` at their starting levels. Flat filled areas
/// drain outwards from their outlet in the order they were reached.
pub fn flood(h: &[f32], n: usize, seeds: &[(u32, f32)]) -> Flood {
    let total = n * n;
    let mut filled = h.to_vec();
    let mut recv = vec![NONE; total];
    let mut seen = vec![false; total];
    let mut order = Vec::with_capacity(total);
    // Heap entries pack the level key and the push count (the tie-break) into one u64;
    // `pushed` maps the count back to the node.
    let mut heap: BinaryHeap<Reverse<u64>> = BinaryHeap::with_capacity(n * 8);
    let mut pushed: Vec<u32> = Vec::with_capacity(total);
    // Nodes inside a hollow are filled to the level that reached them and go through a
    // plain queue, which is much cheaper than the heap (Barnes et al. 2014).
    let mut pit = std::collections::VecDeque::new();
    let push = |heap: &mut BinaryHeap<Reverse<u64>>, pushed: &mut Vec<u32>, level: f32, k: u32| {
        heap.push(Reverse(((key(level) as u64) << 32) | pushed.len() as u64));
        pushed.push(k);
    };
    for &(k, level) in seeds {
        if !seen[k as usize] {
            seen[k as usize] = true;
            filled[k as usize] = level;
            push(&mut heap, &mut pushed, level, k);
        }
    }
    let ni = n as i32;
    loop {
        let k = match pit.pop_front() {
            Some(k) => k,
            None => match heap.pop() {
                Some(Reverse(e)) => pushed[(e & 0xFFFF_FFFF) as usize],
                None => break,
            },
        };
        order.push(k);
        let (i, j) = ((k as usize % n) as i32, (k as usize / n) as i32);
        let level = filled[k as usize];
        let interior = i > 0 && j > 0 && i < ni - 1 && j < ni - 1;
        for &(di, dj, _) in &NEIGHBOURS {
            let (a, b) = (i + di, j + dj);
            if !interior && (a < 0 || b < 0 || a >= ni || b >= ni) {
                continue;
            }
            let m = b as usize * n + a as usize;
            if seen[m] {
                continue;
            }
            seen[m] = true;
            recv[m] = k;
            if h[m] <= level {
                filled[m] = level;
                pit.push_back(m as u32);
            } else {
                push(&mut heap, &mut pushed, h[m], m as u32);
            }
        }
    }
    Flood {
        filled,
        recv,
        order,
    }
}

/// Seeds for a flood: every edge node, and every sea node, at `sea` or its own height.
pub fn seeds(h: &[f32], n: usize, sea_mask: Option<&[bool]>, sea: f32) -> Vec<(u32, f32)> {
    let mut out = Vec::new();
    let level = |k: usize| match sea_mask {
        Some(_) => h[k].max(sea),
        None => h[k],
    };
    for k in 0..n * n {
        let (i, j) = (k % n, k / n);
        let edge = i == 0 || j == 0 || i == n - 1 || j == n - 1;
        let sea_node = sea_mask.is_some_and(|m| m[k]);
        if edge || sea_node {
            out.push((k as u32, level(k)));
        }
    }
    out
}

/// Area (in nodes, weighted by `rain`) draining through each node.
pub fn drainage(flood: &Flood, rain: &[f32]) -> Vec<f32> {
    let mut area = rain.to_vec();
    for &k in flood.order.iter().rev() {
        let r = flood.recv[k as usize];
        if r != NONE {
            area[r as usize] += area[k as usize];
        }
    }
    area
}

/// Lakes and sea on the final macro heights.
pub struct Water {
    /// Lake id per node, or `NONE`.
    pub lake: Vec<u32>,
    /// Water surface per lake: water blocks fill every Y below it.
    pub surface: Vec<i32>,
    pub sea: Vec<bool>,
}

/// Finds lakes in the filled hollows. Hollows smaller than `min_nodes` or shallower than
/// `min_depth` are filled with ground instead (as sediment would), so the map is not
/// covered in puddles.
pub fn water(
    h: &mut [f32],
    filled: &[f32],
    n: usize,
    sea: Option<f32>,
    min_nodes: usize,
    min_depth: f32,
) -> Water {
    let total = n * n;
    let is_sea: Vec<bool> = (0..total)
        .map(|k| sea.is_some_and(|s| filled[k] <= s + 1e-3 && h[k] < s))
        .collect();
    let mut lake = vec![NONE; total];
    let mut surface = Vec::new();
    let mut seen = vec![false; total];
    let mut stack = Vec::new();
    for start in 0..total {
        if seen[start] || is_sea[start] || filled[start] - h[start] <= 0.05 {
            continue;
        }
        let mut members = Vec::new();
        seen[start] = true;
        stack.push(start);
        while let Some(k) = stack.pop() {
            members.push(k);
            let (i, j) = ((k % n) as i32, (k / n) as i32);
            for &(di, dj, _) in &NEIGHBOURS {
                let (a, b) = (i + di, j + dj);
                if a < 0 || b < 0 || a >= n as i32 || b >= n as i32 {
                    continue;
                }
                let m = b as usize * n + a as usize;
                if !seen[m] && !is_sea[m] && filled[m] - h[m] > 0.05 {
                    seen[m] = true;
                    stack.push(m);
                }
            }
        }
        // Rounded down, so the water stays below the ground it spills over.
        let level = (members.iter().map(|&k| filled[k]).fold(f32::MAX, f32::min) + 0.02).floor();
        let bottom = members.iter().map(|&k| h[k]).fold(f32::MAX, f32::min);
        if members.len() >= min_nodes && level - bottom >= min_depth {
            let id = surface.len() as u32;
            surface.push(level as i32);
            for &k in &members {
                lake[k] = id;
            }
        } else {
            for &k in &members {
                h[k] = filled[k];
            }
        }
    }
    Water {
        lake,
        surface,
        sea: is_sea,
    }
}

/// Lowers the outlets of lakes that are deeper than `max_depth` or cover more than
/// `max_nodes`, as their outflow would have cut a gorge. Returns whether anything changed.
/// Lakes with a node in `keep` (ones the user placed) are left alone.
pub fn breach(
    h: &mut [f32],
    flood: &Flood,
    water: &Water,
    max_depth: f32,
    max_nodes: usize,
    cell: f32,
    keep: &[bool],
) -> bool {
    let lakes = water.surface.len();
    let mut members: Vec<Vec<u32>> = vec![Vec::new(); lakes];
    for (k, &l) in water.lake.iter().enumerate() {
        if l != NONE {
            members[l as usize].push(k as u32);
        }
    }
    let mut changed = false;
    let mut search = Search::new(h.len());
    for (l, nodes) in members.iter().enumerate() {
        if nodes.iter().any(|&k| keep[k as usize]) {
            continue;
        }
        let level = water.surface[l] as f32;
        let mut heights: Vec<f32> = nodes.iter().map(|&k| h[k as usize]).collect();
        heights.sort_by(|a, b| a.total_cmp(b));
        let bottom = heights[0];
        // Low enough that only `max_nodes` of the basin stay under water.
        let by_area = if heights.len() > max_nodes {
            heights[max_nodes * 7 / 10] - 0.5
        } else {
            f32::MAX
        };
        let target = (bottom + max_depth).min(by_area);
        if target >= level - 0.5 {
            continue;
        }
        // Follow the water out of the lake to the node it spills over: the last one at the
        // lake's filled level before the ground falls away.
        let mut k = nodes[0] as usize;
        let spill = flood.filled[k] - 0.01;
        while let Some(r) =
            Some(flood.recv[k]).filter(|&r| r != NONE && flood.filled[r as usize] >= spill)
        {
            k = r as usize;
        }
        // Cut a channel across the lake bed from its deepest point to the spill, so ridges
        // under the water cannot hold part of the lake up.
        let n = (h.len() as f64).sqrt() as usize;
        let deepest = *nodes
            .iter()
            .min_by(|&&a, &&b| h[a as usize].total_cmp(&h[b as usize]))
            .unwrap() as usize;
        // Cut a channel across the lake bed from its deepest point to the spill, and on down
        // the outflow until the water downstream is lower anyway (through any hollows on the
        // way, so they cannot hold the lake up). Both follow the path that needs the least
        // cut, so the channel keeps to the low ground the way the river will once the lake
        // has drained, rather than running straight across filled flats.
        let bed = search.least_cut(
            h,
            n,
            deepest,
            target,
            |m| water.lake[m] == l as u32 || m == k,
            |m| m == k,
        );
        for &c in &bed {
            if h[c] > target {
                h[c] = target;
                changed = true;
            }
        }
        // The lake's water can only leave over ground whose filled level is not above its own.
        let top = flood.filled[k] + 0.01;
        let out = search.least_cut(
            h,
            n,
            k,
            target,
            |m| water.lake[m] != l as u32 && flood.filled[m] <= top,
            |m| flood.filled[m] < target,
        );
        let mut allowed = target;
        for c in out {
            if flood.filled[c] <= allowed {
                break;
            }
            if h[c] > allowed {
                h[c] = allowed;
                changed = true;
            }
            allowed -= 0.04 * cell;
        }
    }
    changed
}

/// Scratch space for [`least_cut`], reused between calls.
struct Search {
    cost: Vec<f32>,
    prev: Vec<u32>,
    touched: Vec<u32>,
}

impl Search {
    fn new(len: usize) -> Self {
        Self {
            cost: vec![f32::INFINITY; len],
            prev: vec![NONE; len],
            touched: Vec::new(),
        }
    }

    /// The path from `from` to the first node where `end` holds, through nodes where `open`
    /// holds, that needs the least cutting down to `level` (with a little weight on
    /// length). Nodes in order from `from`; empty when no such node is reachable.
    fn least_cut(
        &mut self,
        h: &[f32],
        n: usize,
        from: usize,
        level: f32,
        open: impl Fn(usize) -> bool,
        end: impl Fn(usize) -> bool,
    ) -> Vec<usize> {
        for &t in &self.touched {
            self.cost[t as usize] = f32::INFINITY;
            self.prev[t as usize] = NONE;
        }
        self.touched.clear();
        self.cost[from] = 0.0;
        self.touched.push(from as u32);
        let mut heap = BinaryHeap::from([Reverse((key(0.0), from))]);
        let mut found = None;
        while let Some(Reverse((c_key, c))) = heap.pop() {
            let here = self.cost[c];
            if key(here) != c_key {
                continue;
            }
            if c != from && end(c) {
                found = Some(c);
                break;
            }
            let (i, j) = ((c % n) as i32, (c / n) as i32);
            for &(di, dj, dist) in &NEIGHBOURS {
                let (a, b) = (i + di, j + dj);
                if a < 0 || b < 0 || a >= n as i32 || b >= n as i32 {
                    continue;
                }
                let m = b as usize * n + a as usize;
                if !open(m) {
                    continue;
                }
                let next = here + (h[m] - level).max(0.0) + 0.05 * dist;
                if next < self.cost[m] {
                    if self.cost[m] == f32::INFINITY {
                        self.touched.push(m as u32);
                    }
                    self.cost[m] = next;
                    self.prev[m] = c as u32;
                    heap.push(Reverse((key(next), m)));
                }
            }
        }
        let mut path = Vec::new();
        let Some(mut c) = found else {
            return path;
        };
        while c != from {
            path.push(c);
            c = self.prev[c] as usize;
        }
        path.push(from);
        path.reverse();
        path
    }
}

/// Widens lake (and sea) masks by up to `rings` nodes onto shore nodes that are less than
/// `margin(k)` above the water and do not drain away below it. Columns over those nodes
/// may dip under the surface once detail is added, so the water must be allowed there.
pub fn widen(
    ids: &[u32],
    surface: impl Fn(u32) -> f32,
    h: &[f32],
    filled: &[f32],
    n: usize,
    rings: usize,
    margin: impl Fn(usize) -> f32,
) -> Vec<u32> {
    let mut out = ids.to_vec();
    for _ in 0..rings {
        let prev = out.clone();
        for k in 0..n * n {
            if prev[k] != NONE {
                continue;
            }
            let (i, j) = ((k % n) as i32, (k / n) as i32);
            for &(di, dj, _) in &NEIGHBOURS {
                let (a, b) = (i + di, j + dj);
                if a < 0 || b < 0 || a >= n as i32 || b >= n as i32 {
                    continue;
                }
                let l = prev[b as usize * n + a as usize];
                if l == NONE {
                    continue;
                }
                let s = surface(l);
                if filled[k] >= s - 0.01 && h[k] < s + margin(k) {
                    out[k] = l;
                    break;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bowl in a slope: water drains to the low edge, the bowl becomes a lake.
    #[test]
    fn bowl_becomes_lake() {
        let n = 21;
        let mut h: Vec<f32> = (0..n * n)
            .map(|k| {
                let (i, j) = ((k % n) as f32, (k / n) as f32);
                let d = ((i - 10.0).powi(2) + (j - 10.0).powi(2)).sqrt();
                20.0 + i * 0.5 - if d < 4.0 { 6.0 * (1.0 - d / 4.0) } else { 0.0 }
            })
            .collect();
        let f = flood(&h, n, &seeds(&h, n, None, 0.0));
        assert_eq!(f.order.len(), n * n);
        for &k in &f.order {
            let (i, j) = (k as usize % n, k as usize / n);
            let edge = i == 0 || j == 0 || i == n - 1 || j == n - 1;
            assert_eq!(f.recv[k as usize] == NONE, edge);
        }
        let w = water(&mut h, &f.filled, n, None, 3, 1.0);
        assert_eq!(w.surface.len(), 1);
        assert_ne!(w.lake[10 * n + 10], NONE);
        assert_eq!(w.lake[0], NONE);
        let rain = vec![1.0; n * n];
        let a = drainage(&f, &rain);
        let total: f32 = (0..n * n)
            .filter(|&k| f.recv[k] == NONE)
            .map(|k| a[k])
            .sum();
        assert_eq!(total, (n * n) as f32);
    }

    #[test]
    fn sea_fills_low_ground_from_the_edges() {
        let n = 16;
        let mut h: Vec<f32> = (0..n * n).map(|k| (k % n) as f32 - 4.0).collect();
        let mask: Vec<bool> = (0..n * n).map(|k| k % n == 0).collect();
        let f = flood(&h, n, &seeds(&h, n, Some(&mask), 1.0));
        let w = water(&mut h, &f.filled, n, Some(1.0), 3, 1.0);
        assert!(w.sea[3] && w.sea[4] && !w.sea[5]);
    }
}
