//! What each block state looks like to the renderer: its quads with textures and tints, read
//! from the game's models where they can be found, otherwise shaped from the catalog.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::Deserialize;
use vistaforge_core::catalog::{BlockState, Catalog};

use super::assets::Assets;
use super::model::{cube_order, is_full_cube, Dir, Models, Quad};
use super::texture::Texture;
use super::tint::{ColorMaps, Tint};

/// The parts of a catalog entry the renderer uses, which the core's catalog doesn't keep.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CatalogFace {
    pub rgb: String,
    #[serde(default)]
    pub coverage: Option<f32>,
    #[serde(default)]
    pub texture: Option<String>,
    #[serde(default, rename = "tintIndex")]
    pub tint_index: Option<i32>,
    #[serde(default)]
    pub tint: Option<String>,
}

impl CatalogFace {
    fn rgb(&self) -> u32 {
        u32::from_str_radix(&self.rgb, 16).unwrap_or(0x808080)
    }

    fn tint(&self) -> Tint {
        match self
            .tint
            .as_deref()
            .and_then(|t| u32::from_str_radix(t, 16).ok())
        {
            Some(t) => Tint::from_catalog(t),
            None => Tint::None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CatalogBlock {
    #[serde(default)]
    pub opaque: Option<bool>,
    #[serde(default)]
    pub top: Option<CatalogFace>,
    #[serde(default)]
    pub side: Option<CatalogFace>,
}

#[derive(Deserialize)]
struct RawCatalog {
    blocks: HashMap<String, CatalogBlock>,
}

/// Reads the face colours, textures and tints from a catalog file.
pub fn catalog_faces(path: &Path) -> Result<HashMap<String, CatalogBlock>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(serde_json::from_str::<RawCatalog>(&text)
        .with_context(|| format!("parsing {}", path.display()))?
        .blocks)
}

/// A quad ready for the GPU: its texture by key, and what tints it.
#[derive(Clone, Debug)]
pub struct Face {
    pub quad: Quad,
    pub tint: Tint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Air,
    Water,
    Solid,
}

/// How a block state renders.
#[derive(Clone, Debug)]
pub struct RenderBlock {
    pub kind: Kind,
    /// One face list per variant.
    pub slots: Vec<Vec<Face>>,
    /// Every variant is the whole cell with opaque textures, faces sorted down, up, north,
    /// south, west, east: nothing behind it can be seen.
    pub full_cube: bool,
    pub waterlogged: bool,
    /// Shaped from the game's model rather than guessed from the catalog.
    pub from_model: bool,
    /// Average colour, 0xRRGGBB.
    pub color: u32,
}

/// Counts for the UI.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Stats {
    pub models: usize,
    pub fallback: usize,
    pub textures: usize,
    pub missing_textures: usize,
    /// A few of the textures and blocks that could not be found.
    pub missing: Vec<String>,
}

pub struct Library {
    pub assets: Option<Assets>,
    faces: HashMap<String, CatalogBlock>,
    pub colormaps: ColorMaps,
    blocks: HashMap<BlockState, Arc<RenderBlock>>,
    textures: HashMap<String, Option<Arc<Texture>>>,
    models: HashMap<String, Option<Arc<serde_json::Value>>>,
    pub stats: Stats,
}

const AIR_LIKE: [&str; 6] = [
    "minecraft:air",
    "minecraft:cave_air",
    "minecraft:void_air",
    "minecraft:light",
    "minecraft:barrier",
    "minecraft:structure_void",
];

impl Library {
    pub fn new(assets: Option<Assets>, faces: HashMap<String, CatalogBlock>) -> Self {
        let colormaps = ColorMaps::load(assets.as_ref());
        Self {
            assets,
            faces,
            colormaps,
            blocks: HashMap::new(),
            textures: HashMap::new(),
            models: HashMap::new(),
            stats: Stats::default(),
        }
    }

    fn note_missing(&mut self, what: String) {
        if self.stats.missing.len() < 12 && !self.stats.missing.contains(&what) {
            self.stats.missing.push(what);
        }
    }

    /// A texture by key: a texture id, or `#…` for the stand-ins made from catalog colours.
    pub fn texture(&mut self, key: &str) -> Option<Arc<Texture>> {
        if let Some(t) = self.textures.get(key) {
            return t.clone();
        }
        let t = if let Some(rest) = key.strip_prefix('#') {
            let mut parts = rest.split(':');
            let kind = parts.next().unwrap_or("");
            let rgb = parts
                .next()
                .and_then(|p| u32::from_str_radix(p, 16).ok())
                .unwrap_or(0x808080);
            let coverage = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0.5);
            Some(Arc::new(match kind {
                "leaves" => Texture::speckled(rgb, coverage, false, rgb),
                "plant" => Texture::speckled(rgb, coverage, true, rgb),
                _ => Texture::solid(rgb),
            }))
        } else {
            let t = self
                .assets
                .as_ref()
                .and_then(|a| Texture::load(a, key))
                .map(Arc::new);
            if t.is_some() {
                self.stats.textures += 1;
            } else {
                self.stats.missing_textures += 1;
                self.note_missing(format!("texture {key}"));
            }
            t
        };
        self.textures.insert(key.to_string(), t.clone());
        t
    }

    /// How `state` renders. `catalog` answers class questions.
    pub fn block(&mut self, state: &BlockState, catalog: &Catalog) -> Arc<RenderBlock> {
        if let Some(b) = self.blocks.get(state) {
            return b.clone();
        }
        let b = Arc::new(self.resolve(state, catalog));
        self.blocks.insert(state.clone(), b.clone());
        b
    }

    fn resolve(&mut self, state: &BlockState, catalog: &Catalog) -> RenderBlock {
        let info = self.faces.get(&state.name).cloned().unwrap_or_default();
        let side = info.side.clone().or(info.top.clone()).unwrap_or_default();
        let top = info.top.clone().or(info.side.clone()).unwrap_or_default();
        let waterlogged = state.get("waterlogged") == Some("true");
        let mut block = RenderBlock {
            kind: Kind::Solid,
            slots: Vec::new(),
            full_cube: false,
            waterlogged,
            from_model: false,
            color: side.rgb(),
        };
        if AIR_LIKE.contains(&state.name.as_str()) {
            block.kind = Kind::Air;
            return block;
        }
        if state.name == "minecraft:water" || state.name == "minecraft:bubble_column" {
            block.kind = Kind::Water;
            block.slots.push(Vec::new());
            return block;
        }

        // The tint of a face with a `tintindex`, as the catalog saw it.
        let tint_for = |index: i32| {
            [&info.top, &info.side]
                .into_iter()
                .flatten()
                .find(|f| f.tint_index == Some(index))
                .or_else(|| {
                    [&info.top, &info.side]
                        .into_iter()
                        .flatten()
                        .find(|f| f.tint.is_some())
                })
                .map(CatalogFace::tint)
                .unwrap_or(Tint::None)
        };

        let shape = match self.assets.as_ref() {
            Some(assets) => {
                let mut models = Models::with_cache(assets, std::mem::take(&mut self.models));
                let shape = models.shape(state);
                self.models = models.into_cache();
                shape.filter(|s| !s.is_empty())
            }
            None => None,
        };
        match shape {
            Some(shape) => {
                block.from_model = true;
                self.stats.models += 1;
                for slot in shape.slots {
                    let faces = slot
                        .into_iter()
                        .map(|quad| {
                            let tint = if quad.tint >= 0 {
                                tint_for(quad.tint)
                            } else {
                                Tint::None
                            };
                            Face { quad, tint }
                        })
                        .collect();
                    block.slots.push(faces);
                }
                // Textures the game has but this install lacks get the catalog's colour.
                for slot in &mut block.slots {
                    for face in slot.iter_mut() {
                        if self.texture(&face.quad.texture).is_none() {
                            face.quad.texture = format!("#solid:{:06x}", side.rgb());
                        }
                    }
                }
            }
            None => {
                self.stats.fallback += 1;
                if self.assets.is_some() {
                    self.note_missing(format!("model of {}", state.name));
                }
                let faces = self.fallback(state, catalog, &info, &top, &side);
                block.slots.push(faces);
            }
        }

        let opaque_textures = block
            .slots
            .iter()
            .flatten()
            .all(|f| self.texture(&f.quad.texture).is_some_and(|t| t.opaque()));
        let cube = block
            .slots
            .iter()
            .all(|s| is_full_cube(&s.iter().map(|f| f.quad.clone()).collect::<Vec<_>>()));
        block.full_cube = cube && opaque_textures && !waterlogged && info.opaque.unwrap_or(true);
        if block.full_cube {
            for slot in &mut block.slots {
                let sorted = cube_order(&slot.iter().map(|f| f.quad.clone()).collect::<Vec<_>>());
                *slot = sorted
                    .into_iter()
                    .map(|q| {
                        let tint = slot
                            .iter()
                            .find(|f| f.quad == q)
                            .map(|f| f.tint)
                            .unwrap_or(Tint::None);
                        Face { quad: q, tint }
                    })
                    .collect();
            }
        }
        if let Some(t) = block
            .slots
            .first()
            .and_then(|s| s.first())
            .and_then(|f| self.texture(&f.quad.texture))
        {
            block.color = t.average();
        }
        block
    }

    /// A shape guessed from the block's class and properties, textured with the catalog's
    /// face textures (when the game has them) or its average colours.
    fn fallback(
        &mut self,
        state: &BlockState,
        catalog: &Catalog,
        info: &CatalogBlock,
        top: &CatalogFace,
        side: &CatalogFace,
    ) -> Vec<Face> {
        let is = |c: &str| catalog.is_a(&state.name, c);
        let name = state.name.as_str();
        let leaves = is("Leaves") || is("class_2397") || name.ends_with("leaves");
        let opaque = info.opaque.unwrap_or(true);
        let coverage = side.coverage.unwrap_or(1.0);
        let plant = !leaves
            && (is("PlantsDense")
                || is("Shrub")
                || is("Sapling")
                || is("class_2261")
                || (!opaque && coverage < 0.9));
        let pick = |f: &CatalogFace, kind: &str| -> String {
            match (&f.texture, self.assets.as_ref()) {
                (Some(t), Some(a)) if a.has(&texture_path(t)) => t.clone(),
                _ => match kind {
                    "leaves" | "plant" => {
                        format!("#{kind}:{:06x}:{:.2}", f.rgb(), f.coverage.unwrap_or(0.5))
                    }
                    _ => format!("#solid:{:06x}", f.rgb()),
                },
            }
        };
        let (t_top, t_side) = if leaves {
            (pick(top, "leaves"), pick(side, "leaves"))
        } else if plant {
            (pick(side, "plant"), pick(side, "plant"))
        } else {
            (pick(top, "solid"), pick(side, "solid"))
        };
        let (tint_top, tint_side) = (top.tint(), side.tint());
        let num = |k: &str| state.get(k).and_then(|v| v.parse::<f32>().ok());

        if plant {
            return cross(&t_side, tint_side);
        }
        let boxes: Vec<([f32; 3], [f32; 3])> =
            if is("BranchLarge") || is("Branch") || is("BranchSmall") {
                let r = if is("BranchLarge") {
                    5.0
                } else if is("BranchSmall") {
                    2.0
                } else {
                    3.5
                };
                let (lo, hi) = (8.0 - r, 8.0 + r);
                let mut b = vec![([lo, 0.0, lo], [hi, 16.0, hi])];
                let on = |k: &str| state.get(k) == Some("true");
                if on("north") {
                    b.push(([lo, lo, 0.0], [hi, hi, lo]));
                }
                if on("south") {
                    b.push(([lo, lo, hi], [hi, hi, 16.0]));
                }
                if on("west") {
                    b.push(([0.0, lo, lo], [lo, hi, hi]));
                }
                if on("east") {
                    b.push(([hi, lo, lo], [16.0, hi, hi]));
                }
                b
            } else if is("LogPillar") || is("Stump") || is("Pillar") {
                let r = 2.0 + num("layer").unwrap_or(3.0) * 1.2;
                vec![([8.0 - r, 0.0, 8.0 - r], [8.0 + r, 16.0, 8.0 + r])]
            } else if let (Some(layers), false) = (num("layers"), is("Slab") || is("class_2482")) {
                vec![([0.0; 3], [16.0, 2.0 * layers, 16.0])]
            } else if let Some(t) = state.get("type").filter(|_| is("Slab") || is("class_2482")) {
                match t {
                    "top" => vec![([0.0, 8.0, 0.0], [16.0; 3])],
                    "double" => vec![([0.0; 3], [16.0; 3])],
                    _ => vec![([0.0; 3], [16.0, 8.0, 16.0])],
                }
            } else if let (Some(facing), true) = (state.get("facing"), is("VerticalSlab")) {
                let t = 4.0 * num("layer").unwrap_or(2.0);
                match facing {
                    "north" => vec![([0.0; 3], [16.0, 16.0, t])],
                    "south" => vec![([0.0, 0.0, 16.0 - t], [16.0; 3])],
                    "west" => vec![([0.0; 3], [t, 16.0, 16.0])],
                    _ => vec![([16.0 - t, 0.0, 0.0], [16.0; 3])],
                }
            } else {
                vec![([0.0; 3], [16.0; 3])]
            };
        let mut faces = Vec::new();
        for (from, to) in boxes {
            for dir in Dir::ALL {
                let (texture, tint) = match dir {
                    Dir::Up | Dir::Down => (&t_top, tint_top),
                    _ => (&t_side, tint_side),
                };
                faces.push(Face {
                    quad: box_face(dir, from, to, texture),
                    tint,
                });
            }
        }
        faces
    }
}

fn texture_path(id: &str) -> String {
    let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
    format!("assets/{ns}/textures/{path}.png")
}

/// One face of the box `from..to` (0..16), textured as a model face without `uv` would be.
fn box_face(dir: Dir, from: [f32; 3], to: [f32; 3], texture: &str) -> Quad {
    let s = 1.0 / 16.0;
    let corners = |f: [f32; 3], t: [f32; 3]| match dir {
        Dir::Down => (
            [f[0], f[1], t[2]],
            [t[0] - f[0], 0.0, 0.0],
            [0.0, 0.0, f[2] - t[2]],
        ),
        Dir::Up => (
            [f[0], t[1], f[2]],
            [t[0] - f[0], 0.0, 0.0],
            [0.0, 0.0, t[2] - f[2]],
        ),
        Dir::North => (
            [t[0], t[1], f[2]],
            [f[0] - t[0], 0.0, 0.0],
            [0.0, f[1] - t[1], 0.0],
        ),
        Dir::South => (
            [f[0], t[1], t[2]],
            [t[0] - f[0], 0.0, 0.0],
            [0.0, f[1] - t[1], 0.0],
        ),
        Dir::West => (
            [f[0], t[1], f[2]],
            [0.0, 0.0, t[2] - f[2]],
            [0.0, f[1] - t[1], 0.0],
        ),
        Dir::East => (
            [t[0], t[1], t[2]],
            [0.0, 0.0, f[2] - t[2]],
            [0.0, f[1] - t[1], 0.0],
        ),
    };
    let (o, u, v) = corners(from, to);
    let a = dir.planar_uv(o);
    let b = dir.planar_uv([o[0] + u[0] + v[0], o[1] + u[1] + v[1], o[2] + u[2] + v[2]]);
    Quad {
        o: o.map(|c| c * s),
        u: u.map(|c| c * s),
        v: v.map(|c| c * s),
        uv: [
            [a[0] * s, a[1] * s],
            [(b[0] - a[0]) * s, 0.0],
            [0.0, (b[1] - a[1]) * s],
        ],
        texture: texture.to_string(),
        tint: -1,
    }
}

/// Two crossed planes through the cell, both sides, as the game's `cross` model.
fn cross(texture: &str, tint: Tint) -> Vec<Face> {
    let k = 0.5 + std::f32::consts::FRAC_1_SQRT_2 * 0.5;
    let l = 1.0 - k;
    let plane = |o: [f32; 3], u: [f32; 3]| Face {
        quad: Quad {
            o,
            u,
            v: [0.0, -1.0, 0.0],
            uv: [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            texture: texture.to_string(),
            tint: 0,
        },
        tint,
    };
    vec![
        plane([l, 1.0, l], [k - l, 0.0, k - l]),
        plane([k, 1.0, k], [l - k, 0.0, l - k]),
        plane([l, 1.0, k], [k - l, 0.0, l - k]),
        plane([k, 1.0, l], [l - k, 0.0, k - l]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_boxes_face_outwards() {
        for dir in Dir::ALL {
            let q = box_face(dir, [0.0; 3], [16.0, 8.0, 16.0], "#solid:808080");
            let n = q.normal();
            assert_eq!(Dir::of(n), Some(dir), "{dir:?}");
        }
        let full: Vec<Quad> = Dir::ALL
            .iter()
            .map(|&d| box_face(d, [0.0; 3], [16.0; 3], "#x"))
            .collect();
        assert!(is_full_cube(&full));
        for f in cross("#x", Tint::Grass) {
            assert!(f.quad.normal()[1].abs() < 1e-6);
        }
    }
}
