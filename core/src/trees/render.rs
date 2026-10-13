//! Side views of trees for checking them by eye: a small ray tracer that draws each block
//! with the shape of its Conquest piece (pillar widths, quarters, slabs, branch arms and
//! posts, stumps, see-through leaves), coloured from the catalog.
//!
//! [`gallery`] builds every species at a range of sizes on a patch of uneven ground and lays
//! the views out on one sheet per species, with and without leaves. `tools/tree-render` is
//! the command-line front end.

use std::collections::HashMap;

use anyhow::Result;

use super::{Species, TreeRequest, Trees};
use crate::catalog::{BlockState, Catalog};
use crate::column::Rect;
use crate::noise::hash64;
use crate::sink::{BlockId, BlockSink, Blocks, Palette, AIR};
use crate::world::Area;

/// An RGB picture.
#[derive(Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 3]>,
}

impl Image {
    pub fn new(width: u32, height: u32, fill: [u8; 3]) -> Self {
        Self {
            width,
            height,
            pixels: vec![fill; (width * height) as usize],
        }
    }

    pub fn put(&mut self, x: i32, y: i32, c: [u8; 3]) {
        if x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height {
            self.pixels[(y as u32 * self.width + x as u32) as usize] = c;
        }
    }

    /// Copies `other` with its top-left corner at `(x, y)`.
    pub fn blit(&mut self, other: &Image, x: i32, y: i32) {
        for j in 0..other.height as i32 {
            for i in 0..other.width as i32 {
                self.put(
                    x + i,
                    y + j,
                    other.pixels[(j * other.width as i32 + i) as usize],
                );
            }
        }
    }

    /// Writes `text` (A–Z, 0–9 and a little punctuation) in a 5×7 font, `scale` pixels per dot.
    pub fn text(&mut self, x: i32, y: i32, text: &str, scale: i32, c: [u8; 3]) {
        let mut cx = x;
        for ch in text.chars() {
            let rows = glyph(ch.to_ascii_uppercase());
            for (j, row) in rows.iter().enumerate() {
                for i in 0..5 {
                    if row & (0x10 >> i) != 0 {
                        for dy in 0..scale {
                            for dx in 0..scale {
                                self.put(cx + i * scale + dx, y + j as i32 * scale + dy, c);
                            }
                        }
                    }
                }
            }
            cx += 6 * scale;
        }
    }

    pub fn png(&self) -> Result<Vec<u8>> {
        crate::png::encode_rgb(self.width, self.height, &self.pixels)
    }
}

/// How to look at a scene.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// Compass angle of the camera in degrees: 0 looks north (towards −z).
    pub yaw: f32,
    /// Degrees the camera looks down.
    pub pitch: f32,
    /// Pixels per block.
    pub scale: f32,
    /// Draw leaves (off shows the bare wood).
    pub leaves: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            yaw: 30.0,
            pitch: 12.0,
            scale: 12.0,
            leaves: true,
        }
    }
}

type V = [f32; 3];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn mul(m: &[[f32; 3]; 3], v: V) -> V {
    [dot(m[0], v), dot(m[1], v), dot(m[2], v)]
}

fn mul_t(m: &[[f32; 3]; 3], v: V) -> V {
    [
        m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2],
        m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2],
        m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2],
    ]
}

fn matmul(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// Rotation about axis 0 (x), 1 (y) or 2 (z), right-handed, as block model JSON uses.
fn rotation(axis: usize, degrees: f32) -> [[f32; 3]; 3] {
    let (s, c) = degrees.to_radians().sin_cos();
    match axis {
        0 => [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
        1 => [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]],
        _ => [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
    }
}

/// How a box is filled.
#[derive(Clone, Copy, PartialEq)]
enum Fill {
    Solid,
    /// See-through foliage: a 4-pixel lattice with this share of cells filled.
    Leaves(f32),
}

/// A box of a block model in pixels (0..16 is the block), placed by `world = m · p + t`.
#[derive(Clone, Copy)]
struct Shape {
    min: V,
    max: V,
    m: [[f32; 3]; 3],
    t: V,
    fill: Fill,
}

impl Shape {
    fn new(min: V, max: V) -> Self {
        Self {
            min,
            max,
            m: IDENTITY,
            t: [0.0; 3],
            fill: Fill::Solid,
        }
    }

    /// Rotated about `origin` like a model element.
    fn rotated(self, axis: usize, degrees: f32, origin: V) -> Self {
        self.turn(rotation(axis, degrees), origin)
    }

    /// Applies a rotation about `origin` on top of the current placement.
    fn turn(mut self, r: [[f32; 3]; 3], origin: V) -> Self {
        self.m = matmul(&r, &self.m);
        self.t = add3(mul(&r, sub(self.t, origin)), origin);
        self
    }

    /// The shape turned a quarter turn `n` times clockwise seen from above (north → east).
    fn quarter_turns(self, n: usize) -> Self {
        let mut s = self;
        for _ in 0..n {
            s = s.turn(rotation(1, -90.0), [8.0, 8.0, 8.0]);
        }
        s
    }

    /// Bounds in pixels after placement.
    fn bounds(&self) -> (V, V) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for i in 0..8 {
            let p = [
                if i & 1 == 0 { self.min[0] } else { self.max[0] },
                if i & 2 == 0 { self.min[1] } else { self.max[1] },
                if i & 4 == 0 { self.min[2] } else { self.max[2] },
            ];
            let w = add3(mul(&self.m, p), self.t);
            for a in 0..3 {
                lo[a] = lo[a].min(w[a]);
                hi[a] = hi[a].max(w[a]);
            }
        }
        (lo, hi)
    }
}

fn add3(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// The solid boxes of a block state in pixels, 0..16 being its own cell (leaves and plants
/// left out, bent pieces boxed whole): what tests use to check that wood holds together.
#[cfg(test)]
pub(crate) fn solid_boxes(state: &BlockState, catalog: &Catalog) -> Vec<(V, V)> {
    let model = Model::of(state, catalog);
    if model.leaves {
        return Vec::new();
    }
    model
        .shapes
        .iter()
        .filter(|s| matches!(s.fill, Fill::Solid))
        .map(Shape::bounds)
        .collect()
}

/// The shapes and colours of one block state.
struct Model {
    shapes: Vec<Shape>,
    side: V,
    top: V,
    leaves: bool,
}

fn rgb(c: u32) -> V {
    [
        ((c >> 16) & 0xFF) as f32 / 255.0,
        ((c >> 8) & 0xFF) as f32 / 255.0,
        (c & 0xFF) as f32 / 255.0,
    ]
}

fn prop(s: &BlockState, key: &str) -> Option<u32> {
    s.get(key).and_then(|v| v.parse().ok())
}

fn is_true(s: &BlockState, key: &str) -> bool {
    s.get(key) == Some("true")
}

fn facing_turns(s: &BlockState) -> usize {
    match s.get("facing") {
        Some("east") => 1,
        Some("south") => 2,
        Some("west") => 3,
        _ => 0,
    }
}

/// Arm, post and the bend of a lone arm for each branch thickness: post half-width, arm
/// bottom, and the bent element of the `_n` model.
fn branch_shapes(s: &BlockState, thick: usize) -> Vec<Shape> {
    let (w, arm_y) = [(4.0, 8.0), (3.0, 10.0), (2.0, 12.0)][thick];
    let (lo, hi) = (8.0 - w, 8.0 + w);
    let mut out = Vec::new();
    let dirs = ["north", "east", "south", "west"];
    let arms: Vec<usize> = (0..4).filter(|&d| is_true(s, dirs[d])).collect();
    let up = is_true(s, "up");
    if up {
        out.push(Shape::new([lo, 0.0, lo], [hi, 16.0, hi]));
    }
    for &d in &arms {
        out.push(Shape::new([lo, arm_y, 0.0], [hi, 16.0, 8.0 + w]).quarter_turns(d));
    }
    if !up && arms.len() == 1 {
        let bend = match thick {
            0 => Shape::new([4.0, 14.9, -2.1], [12.0, 22.95, 9.2]).rotated(
                0,
                -45.0,
                [8.0, 18.85, 9.25],
            ),
            1 => Shape::new([5.0, 16.9, 0.4], [11.0, 22.95, 9.2]).rotated(
                0,
                -45.0,
                [8.0, 18.85, 10.75],
            ),
            _ => {
                Shape::new([6.0, 15.5, 0.4], [10.0, 19.6, 9.2]).rotated(0, -22.5, [8.0, 17.5, 9.25])
            }
        };
        out.push(bend.quarter_turns(arms[0]));
    }
    if out.is_empty() {
        out.push(Shape::new([lo, 0.0, lo], [hi, 16.0, hi]));
    }
    out
}

impl Model {
    fn of(state: &BlockState, catalog: &Catalog) -> Self {
        let info = catalog.block(&state.name);
        let side = info
            .and_then(|b| b.side_rgb.or(b.top_rgb))
            .unwrap_or(0x808080);
        let top = info.and_then(|b| b.top_rgb.or(b.side_rgb)).unwrap_or(side);
        let name = state.name.as_str();
        let layer = prop(state, "layer").unwrap_or(1) as usize;
        // `down` (and `extension_toggle` on vertical slabs) repeat the piece in the block below,
        // reaching through a Conquest layer to the ground.
        let down = if is_true(state, "down") || is_true(state, "extension_toggle") {
            -16.0
        } else {
            0.0
        };
        let leafy = [
            "leaves",
            "needles",
            "blossoms",
            "deciduous_branches",
            "lilac",
            "_bush",
        ]
        .iter()
        .any(|k| name.contains(k));
        let mut shapes = Vec::new();
        if name.contains("weeping_willow") {
            let mut s = Shape::new([2.0, -12.0, 2.0], [14.0, 16.0, 14.0]);
            s.fill = Fill::Leaves(0.4);
            shapes.push(s);
        } else if leafy {
            let mut s = Shape::new([0.0; 3], [16.0; 3]);
            // Bare twigs are mostly holes.
            s.fill = Fill::Leaves(if name.contains("dead_") { 0.22 } else { 0.62 });
            shapes.push(s);
        } else if name.ends_with("_log_pillar") || name.ends_with("_log_stump") {
            let w = super::plan::PILLAR_PX[layer.clamp(1, 5) - 1] as f32 / 2.0;
            shapes.push(Shape::new(
                [8.0 - w, down, 8.0 - w],
                [8.0 + w, 16.0, 8.0 + w],
            ));
            if name.ends_with("_stump") {
                // Root flares around the foot.
                let (r, h) = (w * 0.9, 3.0 + w * 0.4);
                for d in 0..4 {
                    shapes.push(
                        Shape::new([8.0 - r, down, 8.0 - w - r * 0.6], [8.0 + r, h, 8.0])
                            .quarter_turns(d),
                    );
                }
            }
        } else if name.contains("_log_vertical_quarter") {
            let s = super::plan::QUARTER_PX[layer.clamp(1, 5) - 1] as f32;
            // Facing north fills the south-east corner.
            let q = Shape::new([16.0 - s, down, 16.0 - s], [16.0, 16.0, 16.0]);
            shapes.push(q.quarter_turns(facing_turns(state)));
        } else if name.contains("_log_vertical_slab") {
            let t = super::plan::SLAB_PX[layer.clamp(1, 4) - 1] as f32;
            // Facing north lies against the south side.
            let q = Shape::new([0.0, down, 16.0 - t], [16.0, 16.0, 16.0]);
            shapes.push(q.quarter_turns(facing_turns(state)));
        } else if name.ends_with("_large_branch") {
            shapes = branch_shapes(state, 0);
        } else if name.ends_with("_log_branch") {
            shapes = branch_shapes(state, 1);
        } else if name.ends_with("_small_branch") {
            shapes = branch_shapes(state, 2);
        } else if let Some(l) = prop(state, "layers").filter(|_| name.ends_with("_layer")) {
            shapes.push(Shape::new([0.0; 3], [16.0, 2.0 * l as f32, 16.0]));
        } else if info.is_some_and(|b| !b.opaque) && !name.contains("water") {
            // Plants: two crossed, see-through planes, as tall as their `layers` say.
            let h = prop(state, "layers").map_or(13.0, |l| 2.0 * l as f32);
            for angle in [45.0, -45.0] {
                let mut s =
                    Shape::new([0.0, 0.0, 7.5], [16.0, h, 8.5]).rotated(1, angle, [8.0, 8.0, 8.0]);
                s.fill = Fill::Leaves(0.45);
                shapes.push(s);
            }
        } else {
            shapes.push(Shape::new([0.0; 3], [16.0; 3]));
        }
        Self {
            shapes,
            side: rgb(side),
            top: rgb(top),
            leaves: leafy,
        }
    }
}

/// A shape placed in the world, in block units.
#[derive(Clone, Copy)]
struct Placed {
    model: u32,
    shape: u16,
    cell: [i32; 3],
}

struct Hit {
    t: f32,
    normal: V,
    model: u32,
    /// Position in block units, for texture noise.
    at: V,
}

/// Ray against a placed shape: entry distance and outward normal in world space.
fn intersect(shape: &Shape, cell: [i32; 3], o: V, d: V, seed: u64) -> Option<(f32, V)> {
    // Into the shape's own frame, in pixels.
    let base = [
        cell[0] as f32 * 16.0,
        cell[1] as f32 * 16.0,
        cell[2] as f32 * 16.0,
    ];
    let lo_ = sub(scale3(o, 16.0), add3(base, shape.t));
    let lo = mul_t(&shape.m, lo_);
    let ld = mul_t(&shape.m, d);
    let (mut t0, mut t1, mut axis0, mut sign0) = (f32::MIN, f32::MAX, 0, 1.0);
    for a in 0..3 {
        if ld[a].abs() < 1e-9 {
            if lo[a] < shape.min[a] || lo[a] > shape.max[a] {
                return None;
            }
            continue;
        }
        let inv = 1.0 / ld[a];
        let (mut ta, mut tb) = ((shape.min[a] - lo[a]) * inv, (shape.max[a] - lo[a]) * inv);
        let mut s = -1.0;
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
            s = 1.0;
        }
        if ta > t0 {
            t0 = ta;
            axis0 = a;
            sign0 = s;
        }
        t1 = t1.min(tb);
    }
    if t0 > t1 || t1 < 0.0 {
        return None;
    }
    let normal = |axis: usize, sign: f32| {
        let mut n = [0.0; 3];
        n[axis] = sign;
        mul(&shape.m, n)
    };
    match shape.fill {
        Fill::Solid => Some((t0.max(0.0) / 16.0, normal(axis0, sign0))),
        Fill::Leaves(density) => {
            // Walk the 4-pixel lattice inside the box and stop at the first filled cell.
            let mut t = t0.max(0.0) + 0.01;
            let (mut axis, mut sign) = (axis0, sign0);
            while t < t1 {
                let p = add3(lo, scale3(ld, t));
                let c = [
                    ((p[0] - shape.min[0]) / 4.0).floor() as i32,
                    ((p[1] - shape.min[1]) / 4.0).floor() as i32,
                    ((p[2] - shape.min[2]) / 4.0).floor() as i32,
                ];
                let h = hash64(
                    seed ^ ((c[0] as u64 & 0xFF)
                        | (c[1] as u64 & 0xFF) << 8
                        | (c[2] as u64 & 0xFF) << 16),
                );
                if (h >> 40) as f32 / (1u64 << 24) as f32 <= density {
                    return Some((t / 16.0, normal(axis, sign)));
                }
                // Step to the next lattice cell.
                let mut best = (f32::MAX, 0, 1.0);
                for a in 0..3 {
                    if ld[a].abs() < 1e-9 {
                        continue;
                    }
                    let edge = shape.min[a] + 4.0 * (c[a] + (ld[a] > 0.0) as i32) as f32;
                    let te = (edge - lo[a]) / ld[a];
                    if te < best.0 {
                        best = (te, a, if ld[a] > 0.0 { -1.0 } else { 1.0 });
                    }
                }
                t = best.0 + 0.01;
                axis = best.1;
                sign = best.2;
            }
            None
        }
    }
}

fn scale3(a: V, k: f32) -> V {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// Draws the blocks of `rect` between `y0` and `y1` (inclusive) seen as `view` says.
pub fn render(
    sink: &dyn BlockSink,
    catalog: &Catalog,
    rect: Rect,
    y0: i32,
    y1: i32,
    view: &View,
) -> Image {
    // Models per block id, and every shape filed under each cell it reaches into.
    let mut models: Vec<Model> = Vec::new();
    let mut model_of: HashMap<BlockId, u32> = HashMap::new();
    let mut grid: HashMap<[i32; 3], Vec<Placed>> = HashMap::new();
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for (x, z) in rect.columns() {
        for y in y0..=y1 {
            let id = sink.get(x, y, z);
            if id == AIR {
                continue;
            }
            let m = *model_of.entry(id).or_insert_with(|| {
                let state = sink.palette().state(id);
                models.push(Model::of(&state, catalog));
                models.len() as u32 - 1
            });
            if models[m as usize].leaves && !view.leaves {
                continue;
            }
            for (n, shape) in models[m as usize].shapes.iter().enumerate() {
                let (slo, shi) = shape.bounds();
                let a = [
                    x + (slo[0] / 16.0).floor() as i32,
                    y + (slo[1] / 16.0).floor() as i32,
                    z + (slo[2] / 16.0).floor() as i32,
                ];
                let b = [
                    x + ((shi[0] - 0.001) / 16.0).floor() as i32,
                    y + ((shi[1] - 0.001) / 16.0).floor() as i32,
                    z + ((shi[2] - 0.001) / 16.0).floor() as i32,
                ];
                for i in a[0]..=b[0] {
                    for j in a[1]..=b[1] {
                        for k in a[2]..=b[2] {
                            grid.entry([i, j, k]).or_default().push(Placed {
                                model: m,
                                shape: n as u16,
                                cell: [x, y, z],
                            });
                        }
                    }
                }
                let cell = [x, y, z];
                for a in 0..3 {
                    lo[a] = lo[a].min(cell[a] as f32 + slo[a] / 16.0);
                    hi[a] = hi[a].max(cell[a] as f32 + shi[a] / 16.0);
                }
            }
        }
    }
    if grid.is_empty() {
        return Image::new(1, 1, [255, 255, 255]);
    }

    let (yaw, pitch) = (view.yaw.to_radians(), view.pitch.to_radians());
    let d = [
        -pitch.cos() * yaw.sin(),
        -pitch.sin(),
        -pitch.cos() * yaw.cos(),
    ];
    let right = [yaw.cos(), 0.0, -yaw.sin()];
    let up = [
        right[1] * d[2] - right[2] * d[1],
        right[2] * d[0] - right[0] * d[2],
        right[0] * d[1] - right[1] * d[0],
    ];
    let (mut umin, mut umax, mut vmin, mut vmax) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for i in 0..8 {
        let p = [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ];
        let (u, v) = (dot(p, right), dot(p, up));
        umin = umin.min(u);
        umax = umax.max(u);
        vmin = vmin.min(v);
        vmax = vmax.max(v);
    }
    let pad = 1.0;
    let (umin, vmax) = (umin - pad, vmax + pad);
    let width = ((umax - umin + pad) * view.scale).ceil() as u32;
    let height = ((vmax - vmin + pad) * view.scale).ceil() as u32;
    let centre = scale3(add3(lo, hi), 0.5);
    let back = length3(sub(hi, lo)) + 4.0;
    let light = normalize([-0.45, 0.8, 0.4]);
    let (cu, cv) = (dot(centre, right), dot(centre, up));

    let mut img = Image::new(width, height, [0, 0, 0]);
    for j in 0..height {
        // A soft sky behind the trees.
        let sky = {
            let t = j as f32 / height as f32;
            [
                (178.0 + 50.0 * t) as u8,
                (206.0 + 34.0 * t) as u8,
                (232.0 + 18.0 * t) as u8,
            ]
        };
        for i in 0..width {
            let u = umin + (i as f32 + 0.5) / view.scale;
            let v = vmax - (j as f32 + 0.5) / view.scale;
            let o = add3(
                centre,
                add3(
                    scale3(right, u - cu),
                    add3(scale3(up, v - cv), scale3(d, -back)),
                ),
            );
            let colour = match trace(&grid, &models, o, d, back * 2.0) {
                Some(hit) => shade(&models[hit.model as usize], &hit, light),
                None => sky,
            };
            img.pixels[(j * width + i) as usize] = colour;
        }
    }
    img
}

fn length3(a: V) -> f32 {
    dot(a, a).sqrt()
}

fn normalize(a: V) -> V {
    scale3(a, 1.0 / length3(a))
}

/// Walks the cells along a ray and returns the nearest hit.
fn trace(
    grid: &HashMap<[i32; 3], Vec<Placed>>,
    models: &[Model],
    o: V,
    d: V,
    max_t: f32,
) -> Option<Hit> {
    let mut cell = [
        o[0].floor() as i32,
        o[1].floor() as i32,
        o[2].floor() as i32,
    ];
    let step = [
        d[0].signum() as i32,
        d[1].signum() as i32,
        d[2].signum() as i32,
    ];
    let mut next = [0.0f32; 3];
    let mut delta = [f32::MAX; 3];
    for a in 0..3 {
        if d[a].abs() > 1e-9 {
            let edge = cell[a] as f32 + if d[a] > 0.0 { 1.0 } else { 0.0 };
            next[a] = (edge - o[a]) / d[a];
            delta[a] = 1.0 / d[a].abs();
        } else {
            next[a] = f32::MAX;
        }
    }
    let mut t_cell = 0.0;
    while t_cell < max_t {
        let t_exit = next[0].min(next[1]).min(next[2]);
        if let Some(list) = grid.get(&cell) {
            let mut best: Option<Hit> = None;
            for p in list {
                let model = &models[p.model as usize];
                let shape = &model.shapes[p.shape as usize];
                let seed = hash64(
                    (p.cell[0] as u64).wrapping_mul(73856093)
                        ^ (p.cell[1] as u64).wrapping_mul(19349663)
                        ^ (p.cell[2] as u64).wrapping_mul(83492791)
                        ^ p.shape as u64,
                );
                if let Some((t, n)) = intersect(shape, p.cell, o, d, seed) {
                    if t >= t_cell - 1e-4
                        && t <= t_exit + 1e-4
                        && best.as_ref().is_none_or(|b| t < b.t)
                    {
                        best = Some(Hit {
                            t,
                            normal: n,
                            model: p.model,
                            at: add3(o, scale3(d, t)),
                        });
                    }
                }
            }
            if best.is_some() {
                return best;
            }
        }
        let a = if next[0] <= next[1] && next[0] <= next[2] {
            0
        } else if next[1] <= next[2] {
            1
        } else {
            2
        };
        t_cell = next[a];
        next[a] += delta[a];
        cell[a] += step[a];
    }
    None
}

fn shade(model: &Model, hit: &Hit, light: V) -> [u8; 3] {
    let n = hit.normal;
    let base = if n[1] > 0.5 { model.top } else { model.side };
    let diffuse = dot(n, light).max(0.0);
    let mut k = 0.62 + 0.45 * diffuse;
    if n[1] < -0.5 {
        k *= 0.7;
    }
    // A little texture: noise per pixel of the block face.
    let px = [
        (hit.at[0] * 16.0 - n[0] * 0.5).floor() as i64,
        (hit.at[1] * 16.0 - n[1] * 0.5).floor() as i64,
        (hit.at[2] * 16.0 - n[2] * 0.5).floor() as i64,
    ];
    let h = hash64((px[0] as u64) ^ (px[1] as u64) << 21 ^ (px[2] as u64) << 42);
    let grain = if model.leaves { 0.22 } else { 0.12 };
    k *= 1.0 - grain / 2.0 + grain * ((h >> 40) as f32 / (1u64 << 24) as f32);
    [
        (base[0] * k * 255.0).clamp(0.0, 255.0) as u8,
        (base[1] * k * 255.0).clamp(0.0, 255.0) as u8,
        (base[2] * k * 255.0).clamp(0.0, 255.0) as u8,
    ]
}

/// One species at several sizes on a patch of uneven grass, drawn with and without leaves,
/// with labels. `seed` varies the trees. `top` cuts the view off that many blocks above the
/// ground, to look closely at the feet.
#[allow(clippy::too_many_arguments)]
pub fn sheet(
    trees: &Trees,
    blocks: &Blocks,
    catalog: &Catalog,
    species: Species,
    sizes: &[f32],
    seed: u64,
    view: &View,
    top: Option<i32>,
) -> Result<Image> {
    let grass = blocks.id("minecraft:grass_block", &[("snowy", "false")])?;
    let layers: Vec<BlockId> = (1..=7)
        .map(|l| {
            blocks.id(
                "conquest:grass_block_layer",
                &[("layers", &l.to_string()), ("waterlogged", "false")],
            )
        })
        .collect::<Result<_>>()?;
    let mut cols: Vec<(Image, Image, f32)> = Vec::new();
    for (n, &size) in sizes.iter().enumerate() {
        let (h, crown) = trees.shape(&TreeRequest {
            species,
            x: 0,
            y: 0,
            z: 0,
            size,
            seed: 0,
        });
        let reach = match top {
            Some(_) => 5,
            None => (crown.ceil() as i32 + 2).min(super::MAX_REACH + 1),
        };
        let rect = Rect::new(-reach, -reach, reach + 1, reach + 1);
        let mut area = Area::new(rect, blocks.palette);
        // Gently uneven ground with Conquest layers, as the generator makes it.
        for (x, z) in rect.columns() {
            let g = 64.0 + 0.6 * ((x as f32 * 0.31).sin() + (z as f32 * 0.23).cos());
            let top = g.floor() as i32;
            area.fill_column(x, z, 60, top, grass);
            let l = ((g - g.floor()) * 8.0) as usize;
            if l > 0 {
                area.set(x, top, z, layers[l - 1]);
            }
        }
        let g = 64.0 + 0.6 * (0.0f32.sin() + 0.0f32.cos());
        let req = TreeRequest {
            species,
            x: 0,
            y: g.floor() as i32 + (g.fract() * 8.0 >= 7.5) as i32,
            z: 0,
            size,
            seed: seed.wrapping_add(n as u64 * 7919),
        };
        trees.build(&req, &mut area);
        let y1 = top.map_or(64 + h.ceil() as i32 + 6, |t| 64 + t);
        let with = render(&area, catalog, rect, 60, y1, view);
        let bare = render(
            &area,
            catalog,
            rect,
            60,
            y1,
            &View {
                leaves: false,
                ..*view
            },
        );
        cols.push((with, bare, size));
    }
    let gap = 16;
    let label_h = 40;
    let width = cols
        .iter()
        .map(|c| c.0.width.max(c.1.width) as i32 + gap)
        .sum::<i32>()
        + gap;
    let row = cols
        .iter()
        .map(|c| c.0.height.max(c.1.height))
        .max()
        .unwrap_or(1) as i32;
    let mut img = Image::new(
        width as u32,
        (2 * row + 3 * gap + 2 * label_h) as u32,
        [250, 250, 248],
    );
    img.text(
        gap,
        10,
        &format!("{} - {}", species.name(), "with leaves / bare wood"),
        3,
        [40, 40, 40],
    );
    let mut x = gap;
    for (with, bare, size) in &cols {
        let y_top = label_h + gap;
        img.blit(with, x, y_top + row - with.height as i32);
        img.blit(bare, x, y_top + 2 * row + gap - bare.height as i32);
        img.text(
            x,
            y_top + 2 * row + gap + 8,
            &format!("SIZE {size:.2}"),
            2,
            [60, 60, 60],
        );
        x += with.width.max(bare.width) as i32 + gap;
    }
    Ok(img)
}

/// Sheets for the given species, one picture each, named by species id. With `feet`, only
/// the lowest blocks round each trunk, close up.
pub fn gallery(
    catalog: &Catalog,
    species: &[Species],
    seed: u64,
    view: &View,
    feet: bool,
) -> Result<Vec<(String, Image)>> {
    let palette = Palette::new();
    let blocks = Blocks::new(catalog, &palette);
    let trees = Trees::new(&blocks)?;
    let sizes = [0.05, 0.2, 0.4, 0.6, 0.8, 1.0];
    let mut out = Vec::new();
    for &species in species {
        let top = feet.then_some(5);
        let img = sheet(&trees, &blocks, catalog, species, &sizes, seed, view, top)?;
        let id = serde_json::to_string(&species)?
            .trim_matches('"')
            .to_string();
        out.push((id, img));
    }
    Ok(out)
}

/// 5×7 glyphs, one row per byte, high bit on the left.
fn glyph(c: char) -> [u8; 7] {
    match c {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        '/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_turn_like_block_models() {
        // A north arm turned once points east.
        let s = Shape::new([4.0, 8.0, 0.0], [12.0, 16.0, 8.0]).quarter_turns(1);
        let (lo, hi) = s.bounds();
        assert!((lo[0] - 8.0).abs() < 1e-4 && (hi[0] - 16.0).abs() < 1e-4);
        assert!((lo[2] - 4.0).abs() < 1e-4 && (hi[2] - 12.0).abs() < 1e-4);
    }

    #[test]
    fn a_ray_hits_a_post() {
        let s = Shape::new([4.0, 0.0, 4.0], [12.0, 16.0, 12.0]);
        let hit = intersect(&s, [0, 0, 0], [0.5, 0.5, 5.0], [0.0, 0.0, -1.0], 0);
        let (t, n) = hit.unwrap();
        assert!((t - 4.25).abs() < 1e-4);
        assert_eq!(n, [0.0, 0.0, 1.0]);
        assert!(intersect(&s, [0, 0, 0], [0.1, 0.5, 5.0], [0.0, 0.0, -1.0], 0).is_none());
    }
}
