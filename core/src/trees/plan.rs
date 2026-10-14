//! A tree being assembled in a small grid before it is written, and the tools species use to
//! fill it: round trunk cross-sections, limbs walked cell by cell, leaf clumps and hanging
//! leaves.
//!
//! Everything here depends only on the tree's own request, never on what is already in the
//! world, so a tree comes out the same in every piece of the map that rebuilds it. The world is
//! only consulted when the plan is written (see `super::Trees::build`).

use std::sync::OnceLock;

use crate::noise::hash64;

/// A point or direction in block units. Cell `(x, y, z)` spans `x..x+1` etc., so its centre is
/// at `x + 0.5`.
pub(super) type V3 = [f32; 3];

pub(super) fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(super) fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(super) fn scale(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

pub(super) fn length(a: V3) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// Unit vector for a compass angle (radians, 0 = +x, π/2 = +z) and an elevation above the
/// horizon (radians).
pub(super) fn dir(azimuth: f32, elevation: f32) -> V3 {
    let h = elevation.cos();
    [h * azimuth.cos(), elevation.sin(), h * azimuth.sin()]
}

/// Compass angle and elevation of a direction.
pub(super) fn angles(d: V3) -> (f32, f32) {
    let h = (d[0] * d[0] + d[2] * d[2]).sqrt();
    (d[2].atan2(d[0]), d[1].atan2(h))
}

/// Branch thickness, as Conquest names the pieces: `large_branch` (8 px), `branch` (6 px) and
/// `small_branch` (4 px).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Thick {
    Large,
    Medium,
    Small,
}

impl Thick {
    pub(super) fn index(self) -> usize {
        self as usize
    }

    fn width(self) -> u8 {
        match self {
            Thick::Large => 8,
            Thick::Medium => 6,
            Thick::Small => 4,
        }
    }
}

/// What a cell of the plan holds. Vertical pieces carry Conquest's own `layer` value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum Piece {
    #[default]
    Empty,
    /// Leaves; the index picks the block from `super::Leaf`.
    Leaves(u8),
    /// Snow lying on the leaves below, 1 or 2 layers (2 or 4 px) deep.
    Snow(u8),
    /// A wall-like branch piece; its arms and post are worked out when it is written.
    Branch(Thick),
    /// A round post in the middle of the cell (`*_log_pillar`, `layer` 1..=5).
    Pillar(u8),
    /// A square post in one corner of the cell (`*_log_vertical_quarter`, `layer` 1..=5), see
    /// [`CORNERS`].
    Quarter(u8, u8),
    /// A slab against one side of the cell (`*_log_vertical_slab`, `layer` 1..=4), see
    /// [`SIDES`].
    Slab(u8, u8),
    /// A full log standing upright.
    Log,
    /// Bark on every face: thick limbs that run sideways, and branch collars.
    Wood,
}

/// Width in pixels (of 16) of a pillar for each `layer` value.
pub(super) const PILLAR_PX: [u8; 5] = [4, 8, 12, 6, 2];
/// Side in pixels of a vertical quarter for each `layer` value.
pub(super) const QUARTER_PX: [u8; 5] = [2, 4, 8, 12, 6];
/// Thickness in pixels of a vertical slab for each `layer` value.
pub(super) const SLAB_PX: [u8; 4] = [2, 4, 8, 12];
/// Quarter corners: north-west, north-east, south-east, south-west (north is −z).
pub(super) const CORNERS: [(i32, i32); 4] = [(-1, -1), (1, -1), (1, 1), (-1, 1)];
/// Slab sides and branch arms: north, east, south, west.
pub(super) const SIDES: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

impl Piece {
    pub(super) fn is_wood(self) -> bool {
        !matches!(self, Piece::Empty | Piece::Leaves(_) | Piece::Snow(_))
    }

    /// Pieces that Conquest can stretch down into a layer block below (`down=true`, or
    /// `extension_toggle=true` on a vertical slab).
    pub(super) fn has_down(self) -> bool {
        matches!(
            self,
            Piece::Pillar(_) | Piece::Quarter(..) | Piece::Slab(..)
        )
    }

    /// The pillar as wide as a branch's post, which can reach down into a layer block.
    pub(super) fn post_pillar(t: Thick) -> Piece {
        match t {
            Thick::Large => Piece::Pillar(2),
            Thick::Medium => Piece::Pillar(4),
            Thick::Small => Piece::Pillar(1),
        }
    }

    /// Which piece wins when two parts of a tree want the same cell: roughly, the thicker one.
    fn rank(self) -> u32 {
        match self {
            Piece::Empty => 0,
            Piece::Snow(_) => 1,
            Piece::Leaves(_) => 2,
            // A branch beats a pillar of the same width: its arms connect.
            Piece::Branch(t) => 9 + t.width() as u32,
            Piece::Pillar(l) => 8 + PILLAR_PX[l as usize - 1] as u32,
            Piece::Quarter(l, _) => 8 + QUARTER_PX[l as usize - 1] as u32,
            Piece::Slab(l, _) => 10 + SLAB_PX[l as usize - 1] as u32,
            Piece::Log | Piece::Wood => 40,
        }
    }
}

/// The cell sits at the bottom of the trunk: the writer stretches it down to the ground.
pub(super) const FOOT: u8 = 1;
/// Use the stump variant (roots and flared bark) of a pillar or vertical quarter.
pub(super) const STUMP: u8 = 2;
/// Use the mossy variant of the wood.
pub(super) const MOSSY: u8 = 4;
/// Set by the writer: the piece reaches down into the layer block below it.
pub(super) const DOWN: u8 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Cell {
    pub piece: Piece,
    /// Index of the wood type (see `super::Wood`).
    pub wood: u8,
    pub flags: u8,
    /// Sides a limb runs through, one bit per [`SIDES`] entry: where a branch piece gets its
    /// arms. Only the limb's own path joins up, so two limbs that pass side by side stay two
    /// limbs instead of fusing into a ladder.
    pub links: u8,
}

/// The cells of one tree, in a box around its trunk. Coordinates are relative to the tree:
/// the trunk column is `x = z = 0` and the trunk's first cell `y = 0`, so a tree has the
/// same shape wherever it stands.
pub(super) struct Plan {
    /// Lowest corner of the box.
    pub x0: i32,
    pub y0: i32,
    pub z0: i32,
    pub sx: i32,
    pub sy: i32,
    pub sz: i32,
    cells: Vec<Cell>,
    /// Wood type given to pieces put from now on.
    pub wood: u8,
    /// Flags added to pieces put from now on.
    pub flags: u8,
}

impl Plan {
    /// A box reaching `reach` columns from the trunk on each side, and from `below` blocks
    /// under the trunk base to `above` blocks over it.
    pub fn new(reach: i32, below: i32, above: i32, wood: u8) -> Self {
        let (sx, sy, sz) = (2 * reach + 1, below + above + 1, 2 * reach + 1);
        Self {
            x0: -reach,
            y0: -below,
            z0: -reach,
            sx,
            sy,
            sz,
            cells: vec![Cell::default(); (sx * sy * sz) as usize],
            wood,
            flags: 0,
        }
    }

    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let (i, j, k) = (x - self.x0, y - self.y0, z - self.z0);
        (i >= 0 && j >= 0 && k >= 0 && i < self.sx && j < self.sy && k < self.sz)
            .then(|| ((j * self.sz + k) * self.sx + i) as usize)
    }

    pub fn get(&self, x: i32, y: i32, z: i32) -> Cell {
        self.index(x, y, z)
            .map(|i| self.cells[i])
            .unwrap_or_default()
    }

    pub fn piece(&self, x: i32, y: i32, z: i32) -> Piece {
        self.get(x, y, z).piece
    }

    pub fn is_empty(&self, x: i32, y: i32, z: i32) -> bool {
        self.piece(x, y, z) == Piece::Empty
    }

    /// Overwrites a cell (inside the box) as it is.
    pub fn set(&mut self, x: i32, y: i32, z: i32, cell: Cell) {
        if let Some(i) = self.index(x, y, z) {
            self.cells[i] = cell;
        }
    }

    /// Puts a piece unless the cell already holds a thicker one.
    ///
    /// A branch that passes through a vertical piece of a round trunk always takes the cell,
    /// so its arms meet the next cell of the limb instead of stopping short of a piece set
    /// back from the face. Through a piece 8 px or wider it becomes bark: the swelling where
    /// a limb leaves the trunk. Through a thinner one it becomes a branch at least as thick,
    /// so twigs don't fill out the top of a tapering trunk.
    pub fn put(&mut self, x: i32, y: i32, z: i32, piece: Piece) {
        let (wood, flags) = (self.wood, self.flags);
        let Some(i) = self.index(x, y, z) else {
            return;
        };
        let old = self.cells[i];
        let width = match old.piece {
            Piece::Slab(l, _) => Some(SLAB_PX[l as usize - 1]),
            Piece::Quarter(l, _) => Some(QUARTER_PX[l as usize - 1]),
            Piece::Pillar(l) => Some(PILLAR_PX[l as usize - 1]),
            _ => None,
        };
        let forced = match (width, piece) {
            (Some(w), Piece::Branch(_))
                if w >= 12 || (w >= 8 && !matches!(old.piece, Piece::Pillar(_))) =>
            {
                Some(Piece::Wood)
            }
            (Some(w), Piece::Branch(t)) => {
                let fit = match w {
                    8.. => Thick::Large,
                    6.. => Thick::Medium,
                    _ => Thick::Small,
                };
                Some(Piece::Branch(if fit.width() > t.width() { fit } else { t }))
            }
            _ => None,
        };
        let piece = forced.unwrap_or(piece);
        if forced.is_some() || piece.rank() > old.piece.rank() {
            self.cells[i] = Cell {
                piece: if old.links != 0 {
                    jointed(piece)
                } else {
                    piece
                },
                wood,
                flags: flags | (old.flags & (FOOT | STUMP)),
                links: old.links,
            };
        }
    }

    /// Joins two side-by-side cells of a limb (see [`jointed`]).
    fn link(&mut self, a: [i32; 3], b: [i32; 3]) {
        for (from, to) in [(a, b), (b, a)] {
            let side = SIDES
                .iter()
                .position(|&(dx, dz)| from[0] + dx == to[0] && from[2] + dz == to[2]);
            if let (Some(side), true, Some(i)) = (
                side,
                from[1] == to[1],
                self.index(from[0], from[1], from[2]),
            ) {
                let cell = &mut self.cells[i];
                cell.links |= 1 << side;
                cell.piece = jointed(cell.piece);
            }
        }
    }

    pub fn put_leaves(&mut self, x: i32, y: i32, z: i32, leaf: u8) {
        if self.is_empty(x, y, z) {
            self.put(x, y, z, Piece::Leaves(leaf));
        }
    }

    /// Every non-empty cell with its position, bottom up.
    pub fn cells(&self) -> impl Iterator<Item = (i32, i32, i32, Cell)> + '_ {
        self.cells.iter().enumerate().filter_map(move |(n, c)| {
            if c.piece == Piece::Empty {
                return None;
            }
            let n = n as i32;
            let i = n % self.sx;
            let k = (n / self.sx) % self.sz;
            let j = n / (self.sx * self.sz);
            Some((self.x0 + i, self.y0 + j, self.z0 + k, *c))
        })
    }

    /// Snow on the leaves: on each leaf cell with nothing on top, `share` of the time, a
    /// layer or two of snow.
    pub fn snow(&mut self, share: f32, seed: u64) {
        let tops: Vec<[i32; 3]> = self
            .cells()
            .filter(|&(x, y, z, c)| {
                matches!(c.piece, Piece::Leaves(_)) && self.piece(x, y + 1, z) == Piece::Empty
            })
            .map(|(x, y, z, _)| [x, y + 1, z])
            .collect();
        for [x, y, z] in tops {
            let u = unit3(seed ^ 0x5_0e, x, y, z);
            if u < share {
                let layers = 1 + (u < share * 0.4) as u8;
                self.put(x, y, z, Piece::Snow(layers));
            }
        }
    }

    /// The lowest leaf cell in a column.
    pub fn lowest_leaves(&self, x: i32, z: i32) -> Option<i32> {
        (self.y0..self.y0 + self.sy).find(|&y| matches!(self.piece(x, y, z), Piece::Leaves(_)))
    }

    /// One horizontal slice of a round trunk: the circle of radius `r` (blocks) around
    /// `(cx, cz)` at height `y`, drawn with the Conquest piece that best covers it in each
    /// cell — full logs inside, vertical slabs on the flat sides, vertical quarters on the
    /// diagonals, a pillar when the whole trunk is thinner than a block.
    pub fn disc(&mut self, y: i32, cx: f32, cz: f32, r: f32) {
        self.blob(y, &[(cx, cz, r)]);
    }

    /// Like [`Plan::disc`] for the union of several circles `(cx, cz, r)`: the foot of a
    /// trunk with its buttress roots, drawn as one shape so neighbouring pieces meet.
    pub fn blob(&mut self, y: i32, circles: &[(f32, f32, f32)]) {
        let (mut i0, mut i1, mut k0, mut k1) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
        for &(cx, cz, r) in circles {
            let r = r.max(0.03);
            i0 = i0.min((cx - r).floor() as i32);
            i1 = i1.max((cx + r).floor() as i32);
            k0 = k0.min((cz - r).floor() as i32);
            k1 = k1.max((cz + r).floor() as i32);
        }
        for i in i0..=i1 {
            for k in k0..=k1 {
                let mut mask = [0u64; 4];
                for &(cx, cz, r) in circles {
                    let m = disc_mask(cx - i as f32, cz - k as f32, r.max(0.03));
                    for w in 0..4 {
                        mask[w] |= m[w];
                    }
                }
                if let Some(piece) = best_piece(&mask) {
                    self.put(i, y, k, piece);
                }
            }
        }
        let placed: Vec<[i32; 3]> = (i0..=i1)
            .flat_map(|i| (k0..=k1).map(move |k| [i, y, k]))
            .filter(|c| self.piece(c[0], c[1], c[2]).is_wood())
            .collect();
        if placed.len() > 1 {
            self.prune(&placed);
        }
    }

    /// Leaves out each of `cells` that holds a thin sliver (a piece no wider than a quarter
    /// of the cell) touching neither a neighbour on its level nor the piece above: it would
    /// stand apart from the trunk, a stick beside it.
    pub fn prune(&mut self, cells: &[[i32; 3]]) {
        self.leave_out(cells, 4 * 16, false);
    }

    /// Leaves out the pieces among `cells` that only meet each other: held neither by the
    /// piece above one of them nor by a piece beside them that isn't in `cells`. They would
    /// stand apart from the rest of the tree.
    pub fn prune_apart(&mut self, cells: &[[i32; 3]]) {
        self.leave_out(cells, 16 * 16, true);
    }

    fn leave_out(&mut self, cells: &[[i32; 3]], max: u32, apart: bool) {
        let mut doubt: Vec<[i32; 3]> = cells
            .iter()
            .copied()
            .filter(|&[i, y, k]| footprint(self.piece(i, y, k)).is_some_and(|m| count(&m) <= max))
            .collect();
        loop {
            let before = doubt.len();
            let held: Vec<bool> = doubt
                .iter()
                .map(|&c| self.held(c, if apart { &doubt } else { &[] }))
                .collect();
            let mut n = 0;
            doubt.retain(|_| {
                n += 1;
                !held[n - 1]
            });
            // Without `apart`, each piece stands or falls on its own neighbours at once.
            if doubt.len() == before || !apart {
                break;
            }
        }
        for [i, y, k] in doubt {
            self.set(i, y, k, Cell::default());
        }
    }

    /// Whether the piece in `c` meets the piece above it or one beside it, leaving out those
    /// in `doubt`.
    fn held(&self, [i, y, k]: [i32; 3], doubt: &[[i32; 3]]) -> bool {
        let reach = |c: Cell, y: i32| y - (c.flags & DOWN != 0) as i32..=y;
        let cell = self.get(i, y, k);
        let Some(m) = footprint(cell.piece) else {
            return true;
        };
        let span = reach(cell, y);
        let beside = (y - 1..=y + 1).any(|v| {
            SIDES.iter().enumerate().any(|(side, &(dx, dz))| {
                let n = self.get(i + dx, v, k + dz);
                let r = reach(n, v);
                !doubt.contains(&[i + dx, v, k + dz])
                    && r.start() <= span.end()
                    && span.start() <= r.end()
                    && footprint(n.piece).is_some_and(|f| meets(&m, &f, side))
            })
        });
        let above = !doubt.contains(&[i, y + 1, k])
            && footprint(self.piece(i, y + 1, k)).is_some_and(|n| (0..4).any(|w| m[w] & n[w] != 0));
        beside || above
    }

    /// A limb through `points` (centre and radius in pixels), walked cell by cell so every
    /// cell touches the previous one by a face, as Conquest's branch pieces need.
    ///
    /// The walk carries on along the axis it last moved on whenever the line runs out of the
    /// cell that way, and only turns once the line has drifted [`TURN`] blocks off on another
    /// axis.
    /// So a slanting limb comes out as straight runs with a step between them, the way
    /// builders lay branches by hand, instead of a one-by-one staircase.
    pub fn limb(&mut self, points: &[(V3, f32)]) {
        let Some(&(first, r)) = points.first() else {
            return;
        };
        // A limb grows out of the wood it starts beside: begin in the nearest cell of it, so
        // the two meet by a face even where the parent's walk strayed from its line. A slab
        // or quarter at the edge of a trunk swells into bark there, the collar the limb grows
        // from, which meets the trunk's pieces round it where a branch post would stand
        // clear of them.
        let mut cur = cell_of(first);
        if !self.piece(cur[0], cur[1], cur[2]).is_wood() {
            cur = self.nearest(first, 2, Piece::is_wood).unwrap_or(cur);
        }
        if let Some(i) = self.index(cur[0], cur[1], cur[2]) {
            if matches!(self.cells[i].piece, Piece::Quarter(..) | Piece::Slab(..)) {
                self.cells[i].piece = Piece::Wood;
            }
        }
        let up = points.get(1).map(|p| p.0[1] - first[1]).unwrap_or(1.0) > 0.0;
        self.put_limb_cell(cur, r, up);
        let mut along = 1;
        for pair in points.windows(2) {
            let ((a, ra), (b, rb)) = (pair[0], pair[1]);
            let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let len = length(d);
            let steps = (len * 8.0).ceil().max(1.0) as i32;
            let vertical = d[1].abs() > (d[0] * d[0] + d[2] * d[2]).sqrt();
            for s in 1..=steps {
                let t = s as f32 / steps as f32;
                let p = add(a, scale(d, t));
                let r = ra + (rb - ra) * t;
                while let Some(axis) = next_step(p, cur, along) {
                    let prev = cur;
                    cur[axis] += if p[axis] > cur[axis] as f32 + 0.5 {
                        1
                    } else {
                        -1
                    };
                    along = axis;
                    self.put_limb_cell(cur, r, vertical || axis == 1);
                    self.link(prev, cur);
                }
            }
        }
    }

    /// Joins cell `from` to cell `to` on the same level with `piece`, one face at a time.
    pub fn join(&mut self, from: [i32; 3], to: [i32; 3], piece: Piece) {
        let mut cur = from;
        for axis in [0, 2] {
            while cur[axis] != to[axis] {
                let prev = cur;
                cur[axis] += (to[axis] - cur[axis]).signum();
                self.put(cur[0], cur[1], cur[2], piece);
                self.link(prev, cur);
            }
        }
    }

    /// The cell nearest to `p`, within `reach` cells of it, whose piece passes `test`.
    fn nearest(&self, p: V3, reach: i32, test: impl Fn(Piece) -> bool) -> Option<[i32; 3]> {
        let c = cell_of(p);
        let mut best: Option<([i32; 3], f32)> = None;
        for dy in -reach..=reach {
            for dz in -reach..=reach {
                for dx in -reach..=reach {
                    let q = [c[0] + dx, c[1] + dy, c[2] + dz];
                    if !test(self.piece(q[0], q[1], q[2])) {
                        continue;
                    }
                    let centre = [q[0] as f32 + 0.5, q[1] as f32 + 0.5, q[2] as f32 + 0.5];
                    let d = length(sub(centre, p));
                    if best.is_none_or(|(_, b)| d < b) {
                        best = Some((q, d));
                    }
                }
            }
        }
        best.map(|(q, _)| q)
    }

    fn put_limb_cell(&mut self, c: [i32; 3], r: f32, vertical: bool) {
        self.put(c[0], c[1], c[2], limb_piece(r, vertical));
    }

    /// A loose clump of leaves: an ellipsoid (radii in blocks) with a ragged edge and holes,
    /// so the branches inside show through.
    pub fn clump(&mut self, c: V3, rx: f32, ry: f32, holes: f32, leaf: u8, seed: u64) {
        // Noise relative to the clump, so a tree looks the same wherever it stands.
        let o = cell_of(c);
        let (i0, i1) = ((c[0] - rx).floor() as i32, (c[0] + rx).floor() as i32);
        let (j0, j1) = ((c[1] - ry).floor() as i32, (c[1] + ry).floor() as i32);
        let (k0, k1) = ((c[2] - rx).floor() as i32, (c[2] + rx).floor() as i32);
        for i in i0..=i1 {
            for j in j0..=j1 {
                for k in k0..=k1 {
                    let dx = (i as f32 + 0.5 - c[0]) / rx;
                    let dy = (j as f32 + 0.5 - c[1]) / ry;
                    let dz = (k as f32 + 0.5 - c[2]) / rx;
                    let d = dx * dx + dy * dy + dz * dz;
                    let h = unit3(seed, i - o[0], j - o[1], k - o[2]);
                    if d + (h - 0.5) * 0.6 > 1.0 {
                        continue;
                    }
                    if d > 0.35 && unit3(seed ^ 0x5EED, i - o[0], j - o[1], k - o[2]) < holes {
                        continue;
                    }
                    self.put_leaves(i, j, k, leaf);
                }
            }
        }
    }

    /// Leaves hanging down from `(x, y, z)` for up to `len` cells, stopping at anything solid.
    pub fn hang(&mut self, x: i32, y: i32, z: i32, len: i32, leaf: u8) {
        for k in 0..len {
            if !self.is_empty(x, y - k, z) {
                break;
            }
            self.put(x, y - k, z, Piece::Leaves(leaf));
        }
    }
}

/// The piece for a cell a limb runs through sideways, which must reach its neighbour across
/// the face: a pillar, quarter or slab can't, so it becomes a branch as thick (arms reach the
/// face), or bark where a branch would be thinner or set back from the rest of the trunk.
fn jointed(piece: Piece) -> Piece {
    let (w, centred) = match piece {
        Piece::Pillar(l) => (PILLAR_PX[l as usize - 1], true),
        Piece::Quarter(l, _) => (QUARTER_PX[l as usize - 1], false),
        Piece::Slab(l, _) => (SLAB_PX[l as usize - 1], false),
        _ => return piece,
    };
    match w {
        12.. => Piece::Wood,
        8.. if !centred => Piece::Wood,
        8.. => Piece::Branch(Thick::Large),
        6.. => Piece::Branch(Thick::Medium),
        _ => Piece::Branch(Thick::Small),
    }
}

/// How far (blocks from the cell centre) a limb's line may drift sideways before the walk
/// turns towards it.
const TURN: f32 = 0.95;

/// The axis a limb walk at cell `cur`, last moving along `along`, steps on to follow the
/// point `p`, if any: on along the same axis once `p` has left the cell that way, otherwise
/// on the axis where it has drifted furthest past [`TURN`].
fn next_step(p: V3, cur: [i32; 3], along: usize) -> Option<usize> {
    let off = |a: usize| (p[a] - (cur[a] as f32 + 0.5)).abs();
    if off(along) > 0.5 {
        return Some(along);
    }
    (0..3)
        .filter(|&a| off(a) > TURN)
        .max_by(|&a, &b| off(a).total_cmp(&off(b)))
}

pub(super) fn cell_of(p: V3) -> [i32; 3] {
    [
        p[0].floor() as i32,
        p[1].floor() as i32,
        p[2].floor() as i32,
    ]
}

/// The piece for one cell of a limb of radius `r` (pixels).
pub(super) fn limb_piece(r: f32, vertical: bool) -> Piece {
    if vertical {
        match r {
            r if r >= 7.0 => Piece::Log,
            r if r >= 5.0 => Piece::Pillar(3),
            r if r >= 3.5 => Piece::Branch(Thick::Large),
            r if r >= 2.5 => Piece::Branch(Thick::Medium),
            _ => Piece::Branch(Thick::Small),
        }
    } else {
        match r {
            r if r >= 6.0 => Piece::Wood,
            r if r >= 3.5 => Piece::Branch(Thick::Large),
            r if r >= 2.5 => Piece::Branch(Thick::Medium),
            _ => Piece::Branch(Thick::Small),
        }
    }
}

/// Uniform value in [0, 1) for a cell.
pub(super) fn unit3(seed: u64, x: i32, y: i32, z: i32) -> f32 {
    let h = hash64(
        seed ^ hash64(
            (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
                ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9),
        ),
    );
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// A 16×16 coverage mask of one cell, one bit per pixel column, row-major (z, then x).
type Mask = [u64; 4];

fn mask_from(f: impl Fn(i32, i32) -> bool) -> Mask {
    let mut m = [0u64; 4];
    for pz in 0..16 {
        for px in 0..16 {
            if f(px, pz) {
                let bit = pz * 16 + px;
                m[(bit / 64) as usize] |= 1 << (bit % 64);
            }
        }
    }
    m
}

/// Pixels of a cell covered by the circle of radius `r` around `(cx, cz)`, in cell units.
fn disc_mask(cx: f32, cz: f32, r: f32) -> Mask {
    let r2 = r * r;
    mask_from(|px, pz| {
        let dx = (px as f32 + 0.5) / 16.0 - cx;
        let dz = (pz as f32 + 0.5) / 16.0 - cz;
        dx * dx + dz * dz <= r2
    })
}

/// Every vertical piece with the pixels it covers.
fn candidates() -> &'static [(Piece, Mask)] {
    static CANDIDATES: OnceLock<Vec<(Piece, Mask)>> = OnceLock::new();
    CANDIDATES.get_or_init(|| {
        let mut out = vec![(Piece::Log, mask_from(|_, _| true))];
        for (side, &(sx, sz)) in SIDES.iter().enumerate() {
            for (l, &t) in SLAB_PX.iter().enumerate() {
                let t = t as i32;
                let m = mask_from(|px, pz| match (sx, sz) {
                    (0, -1) => pz < t,
                    (1, 0) => px >= 16 - t,
                    (0, 1) => pz >= 16 - t,
                    _ => px < t,
                });
                out.push((Piece::Slab(l as u8 + 1, side as u8), m));
            }
        }
        for (corner, &(cx, cz)) in CORNERS.iter().enumerate() {
            for (l, &s) in QUARTER_PX.iter().enumerate() {
                let s = s as i32;
                let m = mask_from(|px, pz| {
                    let inx = if cx < 0 { px < s } else { px >= 16 - s };
                    let inz = if cz < 0 { pz < s } else { pz >= 16 - s };
                    inx && inz
                });
                out.push((Piece::Quarter(l as u8 + 1, corner as u8), m));
            }
        }
        for (l, &w) in PILLAR_PX.iter().enumerate() {
            let (a, b) = (8 - w as i32 / 2, 8 + w as i32 / 2);
            let m = mask_from(|px, pz| (a..b).contains(&px) && (a..b).contains(&pz));
            out.push((Piece::Pillar(l as u8 + 1), m));
        }
        out
    })
}

/// The pixels a piece covers seen from above (a branch by its post).
fn footprint(piece: Piece) -> Option<Mask> {
    match piece {
        Piece::Empty | Piece::Leaves(_) | Piece::Snow(_) => None,
        Piece::Log | Piece::Wood => Some([u64::MAX; 4]),
        Piece::Branch(t) => {
            let (a, b) = (8 - t.width() as i32 / 2, 8 + t.width() as i32 / 2);
            Some(mask_from(|px, pz| {
                (a..b).contains(&px) && (a..b).contains(&pz)
            }))
        }
        p => candidates().iter().find(|c| c.0 == p).map(|c| c.1),
    }
}

/// Whether footprint `a` meets footprint `b` of the next cell across `a`'s side `side` (see
/// [`SIDES`]) along an edge.
fn meets(a: &Mask, b: &Mask, side: usize) -> bool {
    let bit = |m: &Mask, px: i32, pz: i32| {
        let n = pz * 16 + px;
        m[(n / 64) as usize] >> (n % 64) & 1 == 1
    };
    (0..16).any(|t| match side {
        0 => bit(a, t, 0) && bit(b, t, 15),
        1 => bit(a, 15, t) && bit(b, 0, t),
        2 => bit(a, t, 15) && bit(b, t, 0),
        _ => bit(a, 0, t) && bit(b, 15, t),
    })
}

fn count(m: &Mask) -> u32 {
    m.iter().map(|w| w.count_ones()).sum()
}

/// The piece whose shape differs least from `mask`, or `None` when leaving the cell empty
/// is closer.
fn best_piece(mask: &Mask) -> Option<Piece> {
    let filled = count(mask);
    if filled == 0 {
        return None;
    }
    let mut best = (filled, None);
    for (piece, m) in candidates() {
        let diff = (0..4).map(|w| (mask[w] ^ m[w]).count_ones()).sum::<u32>();
        if diff < best.0 {
            best = (diff, Some(*piece));
        }
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disc_picks_conquest_shapes() {
        // A one-block trunk is a log; a thinner one a pillar.
        let mut p = Plan::new(4, 2, 2, 0);
        p.disc(0, 0.5, 0.5, 0.5);
        assert_eq!(p.piece(0, 0, 0), Piece::Log);
        p.disc(1, 0.5, 0.5, 0.25);
        assert_eq!(p.piece(0, 1, 0), Piece::Pillar(2));
        // A thin trunk centred on a block corner: a quarter in the inner corner of each cell.
        p.disc(-1, 1.0, 1.0, 0.4);
        assert!(matches!(p.piece(0, -1, 0), Piece::Quarter(_, 2)));
        assert!(matches!(p.piece(1, -1, 1), Piece::Quarter(_, 0)));
        assert!(matches!(p.piece(1, -1, 0), Piece::Quarter(_, 3)));
        // A wide trunk: logs inside, slabs on the sides facing the centre.
        p.disc(2, 0.5, 0.5, 1.3);
        assert_eq!(p.piece(0, 2, 0), Piece::Log);
        match p.piece(1, 2, 0) {
            Piece::Slab(_, side) => assert_eq!(SIDES[side as usize], (-1, 0)),
            other => panic!("expected a slab, got {other:?}"),
        }
        match p.piece(1, 2, 1) {
            Piece::Quarter(_, c) => assert_eq!(CORNERS[c as usize], (-1, -1)),
            Piece::Log => {}
            other => panic!("expected a quarter, got {other:?}"),
        }
    }

    #[test]
    fn limbs_are_face_connected() {
        let mut p = Plan::new(8, 2, 12, 0);
        p.limb(&[([0.5, 0.5, 0.5], 4.0), ([5.3, 4.2, 3.9], 2.0)]);
        let cells: Vec<_> = p.cells().map(|(x, y, z, _)| [x, y, z]).collect();
        assert!(cells.len() >= 12);
        for c in &cells {
            let touching = cells
                .iter()
                .any(|o| (0..3).map(|a| (o[a] - c[a]).abs()).sum::<i32>() == 1);
            assert!(touching, "{c:?} is not connected");
        }
    }

    #[test]
    fn collar_where_a_branch_leaves_a_slab() {
        let mut p = Plan::new(4, 2, 2, 0);
        p.put(1, 0, 0, Piece::Slab(3, 3));
        p.put(1, 0, 0, Piece::Branch(Thick::Large));
        assert_eq!(p.piece(1, 0, 0), Piece::Wood);
        p.put(1, 0, 0, Piece::Leaves(0));
        assert_eq!(p.piece(1, 0, 0), Piece::Wood);
        // A twig through the thin top of a trunk becomes a branch there, so its arms reach
        // the next cell instead of stopping short of a quarter set in a corner; through a
        // pillar it takes the pillar's width.
        p.put(2, 0, 0, Piece::Quarter(2, 1));
        p.put(2, 0, 0, Piece::Branch(Thick::Small));
        assert_eq!(p.piece(2, 0, 0), Piece::Branch(Thick::Small));
        p.put(3, 0, 0, Piece::Pillar(2));
        p.put(3, 0, 0, Piece::Branch(Thick::Small));
        assert_eq!(p.piece(3, 0, 0), Piece::Branch(Thick::Large));
    }
}
