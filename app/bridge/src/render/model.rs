//! Minecraft block models: a block state's blockstate file picks models, and each model's
//! cuboid elements become textured quads, the way the game bakes them.
//!
//! Coordinates are in blocks (0..1 across a cell), texture coordinates in 0..1 of a texture.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map, Value};
use vistaforge_core::catalog::BlockState;

use super::assets::Assets;

/// Variants of a block that the renderer picks from by position, like the game's weighted
/// random models.
pub const MAX_SLOTS: usize = 4;

/// A textured parallelogram facing `cross(v, u)`, seen only from the front as in the game.
#[derive(Clone, Debug, PartialEq)]
pub struct Quad {
    /// The corner where the texture's top-left lands.
    pub o: [f32; 3],
    /// Edge along the texture's u (right).
    pub u: [f32; 3],
    /// Edge along the texture's v (down).
    pub v: [f32; 3],
    /// Texture coordinates at `o`, and their change along `u` and along `v`.
    pub uv: [[f32; 2]; 3],
    /// Texture id, e.g. `minecraft:block/stone`.
    pub texture: String,
    /// The model's `tintindex`, or -1.
    pub tint: i32,
}

/// What a block state looks like: one quad list per variant.
#[derive(Clone, Debug, Default)]
pub struct Shape {
    pub slots: Vec<Vec<Quad>>,
}

impl Shape {
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.is_empty())
    }

    pub fn textures(&self) -> impl Iterator<Item = &str> {
        self.slots.iter().flatten().map(|q| q.texture.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Dir {
    pub const ALL: [Dir; 6] = [Dir::Down, Dir::Up, Dir::North, Dir::South, Dir::West, Dir::East];

    fn name(self) -> &'static str {
        match self {
            Dir::Down => "down",
            Dir::Up => "up",
            Dir::North => "north",
            Dir::South => "south",
            Dir::West => "west",
            Dir::East => "east",
        }
    }

    pub fn normal(self) -> [f32; 3] {
        match self {
            Dir::Down => [0.0, -1.0, 0.0],
            Dir::Up => [0.0, 1.0, 0.0],
            Dir::North => [0.0, 0.0, -1.0],
            Dir::South => [0.0, 0.0, 1.0],
            Dir::West => [-1.0, 0.0, 0.0],
            Dir::East => [1.0, 0.0, 0.0],
        }
    }

    /// The direction a normal points along, if it is axis-aligned.
    pub fn of(n: [f32; 3]) -> Option<Dir> {
        Dir::ALL.into_iter().find(|d| {
            let m = d.normal();
            (0..3).all(|i| (n[i] - m[i]).abs() < 1e-3)
        })
    }

    /// The texture coordinates (0..16) the game gives a point of a face looking this way when
    /// the face has no `uv`; also what `uvlock` keeps fixed to the world.
    pub fn planar_uv(self, p: [f32; 3]) -> [f32; 2] {
        let [x, y, z] = p;
        match self {
            Dir::Down => [x, 16.0 - z],
            Dir::Up => [x, z],
            Dir::North => [16.0 - x, 16.0 - y],
            Dir::South => [x, 16.0 - y],
            Dir::West => [z, 16.0 - y],
            Dir::East => [16.0 - z, 16.0 - y],
        }
    }

    /// The face of the box `from..to` (0..16): its top-left corner and its u and v edges.
    fn corners(self, f: [f32; 3], t: [f32; 3]) -> ([f32; 3], [f32; 3], [f32; 3]) {
        let (w, h, d) = (t[0] - f[0], t[1] - f[1], t[2] - f[2]);
        match self {
            Dir::Down => ([f[0], f[1], t[2]], [w, 0.0, 0.0], [0.0, 0.0, -d]),
            Dir::Up => ([f[0], t[1], f[2]], [w, 0.0, 0.0], [0.0, 0.0, d]),
            Dir::North => ([t[0], t[1], f[2]], [-w, 0.0, 0.0], [0.0, -h, 0.0]),
            Dir::South => ([f[0], t[1], t[2]], [w, 0.0, 0.0], [0.0, -h, 0.0]),
            Dir::West => ([f[0], t[1], f[2]], [0.0, 0.0, d], [0.0, -h, 0.0]),
            Dir::East => ([t[0], t[1], t[2]], [0.0, 0.0, -d], [0.0, -h, 0.0]),
        }
    }
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn normalize(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-12);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// Right-handed rotation of `p` about `axis` (0 x, 1 y, 2 z) by `deg` degrees, around `c`.
fn rotate(p: [f32; 3], axis: usize, deg: f32, c: [f32; 3]) -> [f32; 3] {
    let (s, co) = deg.to_radians().sin_cos();
    let [x, y, z] = sub(p, c);
    let r = match axis {
        0 => [x, y * co - z * s, y * s + z * co],
        1 => [x * co + z * s, y, -x * s + z * co],
        _ => [x * co - y * s, x * s + y * co, z],
    };
    add(r, c)
}

impl Quad {
    pub fn normal(&self) -> [f32; 3] {
        normalize(cross(self.v, self.u))
    }

    /// The four corners: top-left, top-right, bottom-right, bottom-left.
    pub fn corners(&self) -> [[f32; 3]; 4] {
        [
            self.o,
            add(self.o, self.u),
            add(add(self.o, self.u), self.v),
            add(self.o, self.v),
        ]
    }

    fn map(&self, f: impl Fn([f32; 3]) -> [f32; 3]) -> Quad {
        let o = f(self.o);
        Quad {
            o,
            u: sub(f(add(self.o, self.u)), o),
            v: sub(f(add(self.o, self.v)), o),
            ..self.clone()
        }
    }
}

/// A variant from a blockstate file.
#[derive(Clone, Debug, PartialEq)]
struct Variant {
    model: String,
    x: i32,
    y: i32,
    uvlock: bool,
    weight: u32,
}

fn variant(v: &Value) -> Option<Variant> {
    let int = |k: &str| v.get(k).and_then(Value::as_i64).unwrap_or(0) as i32;
    Some(Variant {
        model: v.get("model")?.as_str()?.to_string(),
        x: int("x"),
        y: int("y"),
        uvlock: v.get("uvlock").and_then(Value::as_bool).unwrap_or(false),
        weight: v.get("weight").and_then(Value::as_u64).unwrap_or(1).max(1) as u32,
    })
}

fn variants(v: &Value) -> Vec<Variant> {
    match v {
        Value::Array(list) => list.iter().filter_map(variant).collect(),
        other => variant(other).into_iter().collect(),
    }
}

/// `ns:path` with the default namespace filled in.
fn full_id(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

fn split_id(id: &str) -> (&str, &str) {
    id.split_once(':').unwrap_or(("minecraft", id))
}

/// Does a blockstate key like `facing=north,half=top` (or "" / "normal") match the state?
fn key_matches(key: &str, state: &BlockState) -> bool {
    if key.is_empty() || key == "normal" {
        return true;
    }
    key.split(',').all(|pair| match pair.split_once('=') {
        Some((k, v)) => state.get(k.trim()) == Some(v.trim()),
        None => false,
    })
}

/// A multipart `when`: properties with `a|b` alternatives, or `OR` / `AND` lists.
fn when_matches(when: &Value, state: &BlockState) -> bool {
    let Some(obj) = when.as_object() else {
        return true;
    };
    if let Some(list) = obj.get("OR").and_then(Value::as_array) {
        return list.iter().any(|w| when_matches(w, state));
    }
    if let Some(list) = obj.get("AND").and_then(Value::as_array) {
        return list.iter().all(|w| when_matches(w, state));
    }
    obj.iter().all(|(k, v)| {
        let want = match v {
            Value::String(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            _ => return false,
        };
        let have = state.get(k);
        want.split('|').any(|w| Some(w) == have)
    })
}

/// A model file merged with its parents: the textures map and the elements that apply.
struct Merged {
    textures: Map<String, Value>,
    elements: Option<Vec<Value>>,
}

/// Reads models from the assets and bakes block states, remembering what it has read.
pub struct Models<'a> {
    assets: &'a Assets,
    files: HashMap<String, Option<Arc<Value>>>,
}

impl<'a> Models<'a> {
    pub fn new(assets: &'a Assets) -> Self {
        Self::with_cache(assets, HashMap::new())
    }

    /// Starts with model files read earlier, kept from [`Models::into_cache`].
    pub fn with_cache(assets: &'a Assets, files: HashMap<String, Option<Arc<Value>>>) -> Self {
        Self { assets, files }
    }

    pub fn into_cache(self) -> HashMap<String, Option<Arc<Value>>> {
        self.files
    }

    fn model_file(&mut self, id: &str) -> Option<Arc<Value>> {
        let id = full_id(id);
        if let Some(v) = self.files.get(&id) {
            return v.clone();
        }
        let (ns, path) = split_id(&id);
        let v = self
            .assets
            .json(&format!("assets/{ns}/models/{path}.json"))
            .map(Arc::new);
        self.files.insert(id, v.clone());
        v
    }

    fn merged(&mut self, id: &str) -> Option<Merged> {
        let mut textures = Map::new();
        let mut elements = None;
        let mut current = Some(id.to_string());
        let mut found = false;
        for _ in 0..32 {
            let Some(cur) = current.take() else { break };
            if cur.starts_with("builtin/") || cur.starts_with("minecraft:builtin/") {
                break;
            }
            let Some(file) = self.model_file(&cur) else {
                break;
            };
            found = true;
            if let Some(t) = file.get("textures").and_then(Value::as_object) {
                for (k, v) in t {
                    // The child's own definition wins over its parents'.
                    textures.entry(k.clone()).or_insert_with(|| v.clone());
                }
            }
            if elements.is_none() {
                elements = file.get("elements").and_then(Value::as_array).cloned();
            }
            current = file.get("parent").and_then(Value::as_str).map(String::from);
        }
        found.then_some(Merged { textures, elements })
    }

    /// The quads of one model, rotated as the variant says.
    fn bake(&mut self, v: &Variant) -> Option<Vec<Quad>> {
        let merged = self.merged(&v.model)?;
        let mut quads = Vec::new();
        for element in merged.elements.iter().flatten() {
            element_quads(element, &merged.textures, &mut quads);
        }
        Some(
            quads
                .into_iter()
                .map(|q| variant_rotation(q, v))
                .collect(),
        )
    }

    /// The shape of a block state, or `None` when its blockstate file or models are missing.
    pub fn shape(&mut self, state: &BlockState) -> Option<Shape> {
        let (ns, name) = split_id(&state.name);
        let def = self
            .assets
            .json(&format!("assets/{ns}/blockstates/{name}.json"))?;
        // Each slot is a list of variants (one per multipart part) baked together.
        let mut slots: Vec<Vec<Variant>> = Vec::new();
        if let Some(map) = def.get("variants").and_then(Value::as_object) {
            let chosen = map.iter().find(|(k, _)| key_matches(k, state))?;
            for v in weighted(&variants(chosen.1)) {
                slots.push(vec![v]);
            }
        } else if let Some(parts) = def.get("multipart").and_then(Value::as_array) {
            let applied: Vec<Vec<Variant>> = parts
                .iter()
                .filter(|p| p.get("when").is_none_or(|w| when_matches(w, state)))
                .filter_map(|p| p.get("apply").map(variants))
                .filter(|v| !v.is_empty())
                .collect();
            let n = applied.iter().map(|a| a.len().min(MAX_SLOTS)).max().unwrap_or(1);
            for i in 0..n {
                slots.push(applied.iter().map(|a| a[i % a.len()].clone()).collect());
            }
        } else {
            return None;
        }
        let mut shape = Shape::default();
        let mut baked = 0;
        for slot in slots {
            let mut quads = Vec::new();
            for v in &slot {
                if let Some(q) = self.bake(v) {
                    quads.extend(q);
                    baked += 1;
                }
            }
            shape.slots.push(quads);
        }
        (baked > 0 || shape.slots.is_empty()).then_some(shape)
    }
}

/// Up to [`MAX_SLOTS`] picks from a weighted list, in proportion to the weights.
fn weighted(list: &[Variant]) -> Vec<Variant> {
    if list.len() <= 1 {
        return list.to_vec();
    }
    if list.len() >= MAX_SLOTS {
        // Keep the heaviest.
        let mut sorted = list.to_vec();
        sorted.sort_by_key(|v| std::cmp::Reverse(v.weight));
        sorted.truncate(MAX_SLOTS);
        return sorted;
    }
    let total: u32 = list.iter().map(|v| v.weight).sum();
    let mut out: Vec<Variant> = list.to_vec();
    // Repeat the heavier variants into the spare slots.
    while out.len() < MAX_SLOTS {
        let have = |v: &Variant| out.iter().filter(|o| *o == v).count() as f32;
        let best = list
            .iter()
            .max_by(|a, b| {
                let need = |v: &Variant| v.weight as f32 / total as f32 * MAX_SLOTS as f32 - have(v);
                need(a).total_cmp(&need(b))
            })
            .unwrap()
            .clone();
        if best.weight as f32 / total as f32 * MAX_SLOTS as f32 - have(&best) < 0.5 {
            break;
        }
        out.push(best);
    }
    out
}

/// Follows `#name` references to a texture id.
fn resolve_texture(name: &str, textures: &Map<String, Value>) -> Option<String> {
    let mut cur = name.to_string();
    for _ in 0..16 {
        match cur.strip_prefix('#') {
            Some(key) => cur = textures.get(key)?.as_str()?.to_string(),
            None => return Some(full_id(&cur)),
        }
    }
    None
}

fn vec3(v: Option<&Value>) -> Option<[f32; 3]> {
    let a = v?.as_array()?;
    Some([
        a.first()?.as_f64()? as f32,
        a.get(1)?.as_f64()? as f32,
        a.get(2)?.as_f64()? as f32,
    ])
}

fn element_quads(element: &Value, textures: &Map<String, Value>, out: &mut Vec<Quad>) {
    let (Some(from), Some(to)) = (vec3(element.get("from")), vec3(element.get("to"))) else {
        return;
    };
    let Some(faces) = element.get("faces").and_then(Value::as_object) else {
        return;
    };
    let rotation = element.get("rotation").and_then(|r| {
        let origin = vec3(r.get("origin")).unwrap_or([8.0; 3]);
        let axis = match r.get("axis")?.as_str()? {
            "x" => 0,
            "y" => 1,
            _ => 2,
        };
        let angle = r.get("angle")?.as_f64()? as f32;
        let rescale = r.get("rescale").and_then(Value::as_bool).unwrap_or(false);
        Some((origin, axis, angle, rescale))
    });
    for dir in Dir::ALL {
        let Some(face) = faces.get(dir.name()) else {
            continue;
        };
        let Some(texture) = face
            .get("texture")
            .and_then(Value::as_str)
            .and_then(|t| resolve_texture(t, textures))
        else {
            continue;
        };
        let (o, u, v) = dir.corners(from, to);
        // Texture rectangle: given, or where the face lies on the block.
        let [u1, v1, u2, v2] = match face.get("uv").and_then(Value::as_array) {
            Some(a) if a.len() == 4 => {
                let f = |i: usize| a[i].as_f64().unwrap_or(0.0) as f32;
                [f(0), f(1), f(2), f(3)]
            }
            _ => {
                let a = dir.planar_uv(o);
                let b = dir.planar_uv(add(add(o, u), v));
                [a[0], a[1], b[0], b[1]]
            }
        };
        // Corner texture coordinates, top-left, top-right, bottom-left, after the face's own
        // rotation, which turns the texture clockwise on the face.
        let corner = [[u1, v1], [u2, v1], [u2, v2], [u1, v2]];
        let turn = ((face.get("rotation").and_then(Value::as_i64).unwrap_or(0) / 90).rem_euclid(4)) as usize;
        let at = |i: usize| corner[(i + 4 - turn) % 4];
        let (tl, tr, bl) = (at(0), at(1), at(3));
        let s = 1.0 / 16.0;
        let mut quad = Quad {
            o,
            u,
            v,
            uv: [
                [tl[0] * s, tl[1] * s],
                [(tr[0] - tl[0]) * s, (tr[1] - tl[1]) * s],
                [(bl[0] - tl[0]) * s, (bl[1] - tl[1]) * s],
            ],
            texture,
            tint: face.get("tintindex").and_then(Value::as_i64).unwrap_or(-1) as i32,
        };
        if let Some((origin, axis, angle, rescale)) = rotation {
            quad = quad.map(|p| {
                let mut r = rotate(p, axis, angle, origin);
                if rescale && angle != 0.0 {
                    let k = 1.0 / angle.to_radians().cos();
                    for (i, c) in r.iter_mut().enumerate() {
                        if i != axis {
                            *c = origin[i] + (*c - origin[i]) * k;
                        }
                    }
                }
                r
            });
        }
        out.push(quad.map(|p| p.map(|c| c / 16.0)));
    }
}

/// The blockstate's whole-model rotation (x first, then y, clockwise in 90° steps), and
/// `uvlock`, which keeps textures fixed to the world rather than turning with the model.
fn variant_rotation(q: Quad, v: &Variant) -> Quad {
    let c = [0.5; 3];
    let mut q = q;
    if v.x.rem_euclid(360) != 0 {
        q = q.map(|p| rotate(p, 0, -(v.x as f32), c));
    }
    if v.y.rem_euclid(360) != 0 {
        q = q.map(|p| rotate(p, 1, -(v.y as f32), c));
    }
    if v.uvlock && (v.x.rem_euclid(360) != 0 || v.y.rem_euclid(360) != 0) {
        if let Some(dir) = Dir::of(q.normal()) {
            let at = |p: [f32; 3]| dir.planar_uv(p.map(|c| c * 16.0)).map(|t| t / 16.0);
            let o = at(q.o);
            let u = at(add(q.o, q.u));
            let w = at(add(q.o, q.v));
            q.uv = [o, [u[0] - o[0], u[1] - o[1]], [w[0] - o[0], w[1] - o[1]]];
        }
    }
    // Snap the tiny float error of the rotations, so faces stay exactly on the cell edges.
    let snap = |a: [f32; 3]| a.map(|c| (c * 4096.0).round() / 4096.0);
    q.o = snap(q.o);
    q.u = snap(q.u);
    q.v = snap(q.v);
    q
}

/// True when the quads are exactly the six faces of the whole cell, one each, in any order.
pub fn is_full_cube(quads: &[Quad]) -> bool {
    quads.len() == 6
        && Dir::ALL.iter().all(|&d| {
            quads.iter().filter(|q| Dir::of(q.normal()) == Some(d)).count() == 1
        })
        && quads.iter().all(|q| {
            let c = q.corners();
            let n = q.normal();
            let axis = (0..3).find(|&i| n[i].abs() > 0.5).unwrap();
            let plane = if n[axis] > 0.0 { 1.0 } else { 0.0 };
            c.iter().all(|p| (p[axis] - plane).abs() < 1e-4)
                && (0..3).filter(|&i| i != axis).all(|i| {
                    let lo = c.iter().map(|p| p[i]).fold(f32::MAX, f32::min);
                    let hi = c.iter().map(|p| p[i]).fold(f32::MIN, f32::max);
                    lo.abs() < 1e-4 && (hi - 1.0).abs() < 1e-4
                })
        })
}

/// The six faces of a full cube sorted down, up, north, south, west, east.
pub fn cube_order(quads: &[Quad]) -> Vec<Quad> {
    Dir::ALL
        .iter()
        .filter_map(|&d| quads.iter().find(|q| Dir::of(q.normal()) == Some(d)).cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::assets::tests::{temp_dir, zip_file};
    use super::super::assets::{Assets, Stack};
    use super::*;

    fn state(name: &str, props: &[(&str, &str)]) -> BlockState {
        BlockState {
            name: name.into(),
            props: props.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }

    fn assets(name: &str, files: &[(&str, &str)]) -> Assets {
        let dir = temp_dir(name);
        let jar = dir.join("pack.jar");
        let files: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (*n, d.as_bytes())).collect();
        zip_file(&jar, &files);
        Assets::from_stack(&Stack {
            minecraft_jar: Some(jar),
            ..Stack::default()
        })
        .unwrap()
    }

    const CUBE: &str = r##"{"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{
        "down":{"texture":"#down","cullface":"down"},"up":{"texture":"#up","cullface":"up"},
        "north":{"texture":"#north"},"south":{"texture":"#south"},
        "west":{"texture":"#west"},"east":{"texture":"#east"}}}]}"##;
    const CUBE_ALL: &str = r##"{"parent":"block/cube","textures":{"particle":"#all","down":"#all",
        "up":"#all","north":"#all","east":"#all","south":"#all","west":"#all"}}"##;
    const COLUMN: &str = r##"{"parent":"minecraft:block/cube","textures":{"down":"#end","up":"#end",
        "north":"#side","east":"#side","south":"#side","west":"#side"}}"##;

    #[test]
    fn bakes_cubes_with_parents_and_rotations() {
        let a = assets(
            "cube",
            &[
                ("assets/minecraft/models/block/cube.json", CUBE),
                ("assets/minecraft/models/block/cube_all.json", CUBE_ALL),
                ("assets/minecraft/models/block/cube_column.json", COLUMN),
                (
                    "assets/minecraft/models/block/stone.json",
                    r#"{"parent":"minecraft:block/cube_all","textures":{"all":"minecraft:block/stone"}}"#,
                ),
                (
                    "assets/minecraft/blockstates/stone.json",
                    r#"{"variants":{"":[{"model":"block/stone"},{"model":"block/stone","y":90},
                       {"model":"block/stone","y":180},{"model":"block/stone","y":270}]}}"#,
                ),
                (
                    "assets/test/models/block/log.json",
                    r#"{"parent":"block/cube_column","textures":{"end":"test:block/log_top","side":"test:block/log"}}"#,
                ),
                (
                    "assets/test/blockstates/log.json",
                    r#"{"variants":{"axis=y":{"model":"test:block/log"},
                       "axis=z":{"model":"test:block/log","x":90},
                       "axis=x":{"model":"test:block/log","x":90,"y":90}}}"#,
                ),
            ],
        );
        let mut m = Models::new(&a);
        let stone = m.shape(&state("minecraft:stone", &[])).unwrap();
        assert_eq!(stone.slots.len(), 4);
        assert!(stone.slots.iter().all(|s| is_full_cube(s)));
        assert!(stone.textures().all(|t| t == "minecraft:block/stone"));

        // A log lying along z shows its end texture to the north and south.
        let log = m.shape(&state("test:log", &[("axis", "z")])).unwrap();
        assert_eq!(log.slots.len(), 1);
        let quads = cube_order(&log.slots[0]);
        assert!(is_full_cube(&quads));
        let tex: Vec<&str> = quads.iter().map(|q| q.texture.as_str()).collect();
        assert_eq!(
            tex,
            ["test:block/log", "test:block/log", "test:block/log_top", "test:block/log_top", "test:block/log", "test:block/log"]
        );
        // Along x: ends east and west.
        let log = m.shape(&state("test:log", &[("axis", "x")])).unwrap();
        let quads = cube_order(&log.slots[0]);
        assert_eq!(quads[4].texture, "test:block/log_top");
        assert_eq!(quads[5].texture, "test:block/log_top");
        // Unknown blocks have no shape.
        assert!(m.shape(&state("test:missing", &[])).is_none());
    }

    #[test]
    fn faces_map_textures_like_the_game() {
        let a = assets(
            "faces",
            &[(
                "assets/minecraft/models/block/slab.json",
                r##"{"textures":{"t":"block/x"},"elements":[{"from":[0,0,0],"to":[16,8,16],"faces":{
                    "north":{"texture":"#t"},"up":{"texture":"#t","uv":[0,0,16,16],"rotation":90}}}]}"##,
            ), ("assets/minecraft/blockstates/slab.json", r#"{"variants":{"":{"model":"block/slab"}}}"#)],
        );
        let shape = Models::new(&a).shape(&state("minecraft:slab", &[])).unwrap();
        let north = shape.slots[0].iter().find(|q| Dir::of(q.normal()) == Some(Dir::North)).unwrap();
        // Seen from the north, the top-left of the texture is at the north-east top corner, and
        // a half-height face shows the lower half of the texture.
        assert_eq!(north.o, [1.0, 0.5, 0.0]);
        assert_eq!(north.u, [-1.0, 0.0, 0.0]);
        assert_eq!(north.v, [0.0, -0.5, 0.0]);
        assert_eq!(north.uv, [[0.0, 0.5], [1.0, 0.0], [0.0, 0.5]]);
        // Turning the texture 90° clockwise puts its bottom-left at the face's top-left.
        let up = shape.slots[0].iter().find(|q| Dir::of(q.normal()) == Some(Dir::Up)).unwrap();
        assert_eq!(up.uv[0], [0.0, 1.0]);
        assert_eq!(up.uv[1], [0.0, -1.0]);
        assert_eq!(up.uv[2], [1.0, 0.0]);
        assert_eq!(up.texture, "minecraft:block/x");
    }

    #[test]
    fn multipart_and_crosses() {
        let a = assets(
            "multipart",
            &[
                (
                    "assets/test/models/block/post.json",
                    r##"{"textures":{"t":"test:block/wood"},"elements":[{"from":[6,0,6],"to":[10,16,10],"faces":{"up":{"texture":"#t"}}}]}"##,
                ),
                (
                    "assets/test/models/block/arm.json",
                    r##"{"textures":{"t":"test:block/wood"},"elements":[{"from":[6,6,0],"to":[10,10,6],"faces":{"north":{"texture":"#t","tintindex":0}}}]}"##,
                ),
                (
                    "assets/test/blockstates/branch.json",
                    r#"{"multipart":[{"apply":{"model":"test:block/post"}},
                        {"when":{"north":"true"},"apply":{"model":"test:block/arm"}},
                        {"when":{"OR":[{"east":"true"},{"west":"true|maybe"}]},"apply":{"model":"test:block/arm","y":90}}]}"#,
                ),
                (
                    "assets/test/models/block/cross.json",
                    r##"{"textures":{"cross":"test:block/fern"},"elements":[{"from":[0.8,0,8],"to":[15.2,16,8],
                        "rotation":{"origin":[8,8,8],"axis":"y","angle":45,"rescale":true},
                        "faces":{"north":{"texture":"#cross"},"south":{"texture":"#cross"}}}]}"##,
                ),
                ("assets/test/blockstates/fern.json", r#"{"variants":{"":{"model":"test:block/cross"}}}"#),
            ],
        );
        let mut m = Models::new(&a);
        let mut s = |p: &[(&str, &str)]| m.shape(&state("test:branch", p)).unwrap().slots[0].clone();
        assert_eq!(s(&[("north", "false"), ("east", "false"), ("west", "false")]).len(), 1);
        let both = s(&[("north", "true"), ("east", "false"), ("west", "true")]);
        assert_eq!(both.len(), 3);
        // The arm turned by y=90 faces east: its texture side points the other way round.
        assert_eq!(Dir::of(both[2].normal()), Some(Dir::East));
        assert_eq!(both[1].tint, 0);

        let fern = m.shape(&state("test:fern", &[])).unwrap();
        let q = &fern.slots[0][0];
        // A diagonal plane through the centre, stretched back to the full cell width.
        let c = q.corners();
        assert!((c[0][0] - c[0][2]).abs() < 1e-3 || (c[0][0] + c[0][2] - 1.0).abs() < 1e-3);
        let width = (q.u[0].powi(2) + q.u[2].powi(2)).sqrt();
        assert!((width - 1.272).abs() < 0.01, "width {width}");
    }

    #[test]
    fn uvlock_keeps_textures_on_the_world() {
        let a = assets(
            "uvlock",
            &[
                (
                    "assets/test/models/block/half.json",
                    r##"{"textures":{"t":"test:block/t"},"elements":[{"from":[0,0,0],"to":[16,16,8],"faces":{"up":{"texture":"#t"}}}]}"##,
                ),
                (
                    "assets/test/blockstates/half.json",
                    r#"{"variants":{"facing=east":{"model":"test:block/half","y":90,"uvlock":true}}}"#,
                ),
            ],
        );
        let shape = Models::new(&a).shape(&state("test:half", &[("facing", "east")])).unwrap();
        let up = &shape.slots[0][0];
        // Each corner's texture coordinates are its x and z, as on an unrotated top face.
        for (p, k) in [(up.o, [0.0, 0.0]), (add(up.o, up.u), [1.0, 0.0]), (add(up.o, up.v), [0.0, 1.0])] {
            let uv = [
                up.uv[0][0] + up.uv[1][0] * k[0] + up.uv[2][0] * k[1],
                up.uv[0][1] + up.uv[1][1] * k[0] + up.uv[2][1] * k[1],
            ];
            assert!((uv[0] - p[0]).abs() < 1e-4 && (uv[1] - p[2]).abs() < 1e-4, "{p:?} {uv:?}");
        }
    }
}
