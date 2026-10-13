//! Phase 0 test map: a square meadow of Conquest Reforged layer blocks with a pond, a sandy
//! shore, meadow plants and a few oaks built from Conquest branch pieces.
//!
//! It exists to check in the game that the world format, lighting, water, block states and
//! neighbour-dependent states (branch connections, plants sitting on layers) come out right.

use anyhow::Result;

use crate::catalog::{BlockState, Catalog};
use crate::noise::{fbm2, Rng};
use crate::world::{World, MIN_Y};

/// Water fills every empty cell up to and including this Y.
pub const WATER_Y: i32 = 63;

pub struct TestWorldOptions {
    pub size: i32,
    pub seed: u64,
}

pub struct TestWorld {
    pub world: World,
    pub spawn: (i32, i32, i32),
    pub trees: usize,
    pub plants: usize,
}

struct Blocks {
    bedrock: BlockState,
    stone: BlockState,
    dirt: BlockState,
    grass: BlockState,
    sand: BlockState,
    gravel: BlockState,
    water: BlockState,
    log: BlockState,
    leaves: BlockState,
    grass_layers: Vec<BlockState>,
    clover_layers: Vec<BlockState>,
    sand_layers: Vec<BlockState>,
    gravel_layers_wet: Vec<BlockState>,
    plants: Vec<String>,
    large_branch: String,
    branch: String,
    small_branch: String,
}

/// States for layers 1..=7 (index 0 is layers=1).
fn layer_states(c: &Catalog, id: &str, waterlogged: bool) -> Result<Vec<BlockState>> {
    let wl = if waterlogged { "true" } else { "false" };
    (1..=7)
        .map(|l| c.state(id, &[("layers", &l.to_string()), ("waterlogged", wl)]))
        .collect()
}

impl Blocks {
    fn new(c: &Catalog) -> Result<Self> {
        let plants = [
            "conquest:common_meadow_grass",
            "conquest:grass",
            "conquest:timothy_grass",
            "conquest:kentucky_bluegrass",
            "conquest:buttercups",
            "conquest:red_clover_flowers",
            "conquest:oxeye_daisy",
        ];
        for p in plants {
            c.default_state(p)?;
        }
        for b in [
            "conquest:oak_log_large_branch",
            "conquest:oak_log_branch",
            "conquest:oak_log_small_branch",
        ] {
            c.default_state(b)?;
        }
        Ok(Self {
            bedrock: c.default_state("minecraft:bedrock")?,
            stone: c.default_state("minecraft:stone")?,
            dirt: c.default_state("minecraft:dirt")?,
            grass: c.state("minecraft:grass_block", &[("snowy", "false")])?,
            sand: c.default_state("minecraft:sand")?,
            gravel: c.default_state("minecraft:gravel")?,
            water: c.state("minecraft:water", &[("level", "0")])?,
            log: c.state("minecraft:oak_log", &[("axis", "y")])?,
            leaves: c.state(
                "minecraft:oak_leaves",
                &[("persistent", "true"), ("distance", "1")],
            )?,
            grass_layers: layer_states(c, "conquest:grass_block_layer", false)?,
            clover_layers: layer_states(c, "conquest:clover_covered_grass_layer", false)?,
            sand_layers: layer_states(c, "conquest:sand_layer", false)?,
            gravel_layers_wet: layer_states(c, "conquest:gravel_layer", true)?,
            plants: plants.iter().map(|s| s.to_string()).collect(),
            large_branch: "conquest:oak_log_large_branch".into(),
            branch: "conquest:oak_log_branch".into(),
            small_branch: "conquest:oak_log_small_branch".into(),
        })
    }
}

/// Terrain height (top of the ground) at a column, in blocks, and the distance to the pond
/// centre in pond radii.
fn height(seed: u64, size: i32, x: i32, z: i32) -> (f64, f64) {
    let (fx, fz) = (x as f64, z as f64);
    let mut h = 66.5
        + 4.5 * fbm2(seed, fx / 70.0, fz / 70.0, 4)
        + 1.2 * fbm2(seed ^ 0xA5, fx / 18.0, fz / 18.0, 2);
    // A pond: a smooth bowl that dips below the water line.
    let (px, pz, r) = (size as f64 * 0.70, size as f64 * 0.36, size as f64 * 0.11);
    let d = ((fx - px).powi(2) + (fz - pz).powi(2)).sqrt() / r;
    if d < 1.6 {
        let t = (1.0 - d / 1.6).max(0.0);
        h -= 9.0 * t * t * (3.0 - 2.0 * t);
    }
    (h, d)
}

/// Builds the test map in `[0, size)²`.
pub fn generate(catalog: &Catalog, opts: &TestWorldOptions) -> Result<TestWorld> {
    let b = Blocks::new(catalog)?;
    let mut world = World::new();
    let mut rng = Rng::new(opts.seed);
    let size = opts.size;
    for cx in 0..(size + 15) / 16 {
        for cz in 0..(size + 15) / 16 {
            world.touch_chunk(cx, cz);
        }
    }

    // Surface cell per column: the Y of the topmost ground cell (full block or layer) and its layers (8 = full).
    let mut surface = vec![(0i32, 8u8); (size * size) as usize];
    let idx = |x: i32, z: i32| (z * size + x) as usize;

    for x in 0..size {
        for z in 0..size {
            let (h, pond_distance) = height(opts.seed, size, x, z);
            let mut top = h.floor() as i32 - 1; // last full block
            let mut layers = ((h - h.floor()) * 8.0).round() as i32;
            if layers == 8 {
                top += 1;
                layers = 0;
            }
            let underwater = h < WATER_Y as f64 + 0.9;
            let beach = !underwater && pond_distance < 1.6 && h < WATER_Y as f64 + 1.8;
            let clover = fbm2(opts.seed ^ 0xC10, x as f64 / 24.0, z as f64 / 24.0, 2) > 0.25;

            world.set(x, MIN_Y, z, &b.bedrock);
            for y in MIN_Y + 1..=top {
                let state = if y <= top - 4 {
                    &b.stone
                } else if underwater {
                    &b.gravel
                } else if beach {
                    &b.sand
                } else if y == top && layers == 0 {
                    &b.grass
                } else {
                    &b.dirt
                };
                world.set(x, y, z, state);
            }
            let mut surface_y = top;
            if layers > 0 {
                let l = (layers - 1) as usize;
                let layer_y = top + 1;
                let state = if layer_y <= WATER_Y {
                    &b.gravel_layers_wet[l]
                } else if underwater || beach {
                    &b.sand_layers[l]
                } else if clover {
                    &b.clover_layers[l]
                } else {
                    &b.grass_layers[l]
                };
                world.set(x, layer_y, z, state);
                surface_y = layer_y;
            }
            for y in surface_y + 1..=WATER_Y {
                world.set(x, y, z, &b.water);
            }
            surface[idx(x, z)] = (surface_y, if layers > 0 { layers as u8 } else { 8 });
        }
    }

    // Oaks on a jittered grid, away from water, the edges and the spawn point.
    let spawn_xz = (size / 2, size / 2);
    let mut trees = 0;
    let cell = 40;
    for gx in 0..size / cell {
        for gz in 0..size / cell {
            let x = gx * cell + rng.range(8, cell - 8);
            let z = gz * cell + rng.range(8, cell - 8);
            if x < 8 || z < 8 || x >= size - 8 || z >= size - 8 {
                continue;
            }
            if (x - spawn_xz.0).abs() < 10 && (z - spawn_xz.1).abs() < 10 {
                continue;
            }
            let (sy, layers) = surface[idx(x, z)];
            if sy <= WATER_Y + 1 || rng.unit() < 0.25 {
                continue;
            }
            // A trunk replaces a partial layer so it doesn't float above it.
            let base = if layers < 8 { sy } else { sy + 1 };
            build_oak(&mut world, &b, &mut rng, x, base, z);
            trees += 1;
        }
    }
    connect_branches(&mut world, catalog);

    // Meadow plants on dry grass, lowered onto the layer below them.
    let mut plants = 0;
    for x in 0..size {
        for z in 0..size {
            let (sy, layers) = surface[idx(x, z)];
            if sy <= WATER_Y || rng.unit() > 0.16 {
                continue;
            }
            let ground = world.get(x, sy, z).name.clone();
            let grassy = ground == "minecraft:grass_block"
                || ground == "conquest:grass_block_layer"
                || ground == "conquest:clover_covered_grass_layer";
            if !grassy || !world.get(x, sy + 1, z).is_air() {
                continue;
            }
            let plant = rng.pick(&b.plants).clone();
            let state = catalog.state(
                &plant,
                &[
                    ("layers", &layers.to_string()),
                    ("offset_toggle", "false"),
                    ("waterlogged", "false"),
                ],
            )?;
            world.set(x, sy + 1, z, &state);
            plants += 1;
        }
    }

    // Spawn on top of whatever is at the centre column.
    let (sx, sz) = spawn_xz;
    let mut sy = surface[idx(sx, sz)].0 + 1;
    while !world.get(sx, sy, sz).is_air() {
        sy += 1;
    }
    Ok(TestWorld {
        world,
        spawn: (sx, sy, sz),
        trees,
        plants,
    })
}

/// An oak: a full-log trunk, a thinner trunk of large branches above it, horizontal limbs
/// tapering from large to small branches, and leaf clusters at the limb ends and the top.
fn build_oak(world: &mut World, b: &Blocks, rng: &mut Rng, x: i32, base: i32, z: i32) {
    let trunk = rng.range(4, 6);
    for y in base..base + trunk {
        world.set(x, y, z, &b.log);
    }
    let crown_base = base + trunk;
    let crown = rng.range(3, 4);
    let large = |w: &mut World, x, y, z| {
        w.set(
            x,
            y,
            z,
            &BlockState {
                name: b.large_branch.clone(),
                props: vec![],
            },
        )
    };
    for y in crown_base..crown_base + crown {
        large(world, x, y, z);
    }

    let mut tips = vec![(x, crown_base + crown - 1, z)];
    let dirs = [(0, -1), (1, 0), (0, 1), (-1, 0)];
    let start = rng.range(0, 3) as usize;
    for i in 0..4 {
        if i == 3 && rng.unit() < 0.5 {
            continue;
        }
        let (dx, dz) = dirs[(start + i) % 4];
        let ly = crown_base + (i as i32 % crown.min(3));
        let len = rng.range(2, 4);
        for k in 1..=len {
            let name = if k == 1 {
                &b.large_branch
            } else if k < len {
                &b.branch
            } else {
                &b.small_branch
            };
            world.set(
                x + dx * k,
                ly,
                z + dz * k,
                &BlockState {
                    name: name.clone(),
                    props: vec![],
                },
            );
        }
        tips.push((x + dx * len, ly, z + dz * len));
    }

    for (tx, ty, tz) in tips {
        let r = 2 + (rng.unit() < 0.5) as i32;
        for dx in -r..=r {
            for dy in -1..=r {
                for dz in -r..=r {
                    let d2 = dx * dx + dz * dz + (dy * dy * 2);
                    if d2 > r * r + 1 || (d2 > r * r - 2 && rng.unit() < 0.35) {
                        continue;
                    }
                    let (px, py, pz) = (tx + dx, ty + dy, tz + dz);
                    if world.get(px, py, pz).is_air() {
                        world.set(px, py, pz, &b.leaves);
                    }
                }
            }
        }
    }
}

/// Fills in the connection properties the game would compute on placement: an arm towards
/// every neighbouring branch piece, and a post (`up`) when the block below is sturdy or a branch.
fn connect_branches(world: &mut World, catalog: &Catalog) {
    let is_branch = |s: &BlockState| catalog.is_a(&s.name, "BranchLarge");
    let sturdy = |s: &BlockState| {
        !s.is_air()
            && (catalog.block(&s.name).map(|i| i.opaque).unwrap_or(false)
                || catalog.is_a(&s.name, "class_2397")
                || catalog.is_a(&s.name, "Leaves"))
    };
    let mut cells = Vec::new();
    for (&(cx, cz), chunk) in &world.chunks {
        for (si, section) in chunk.sections.iter().enumerate() {
            let Some(section) = section else { continue };
            for (i, &id) in section.iter().enumerate() {
                if id != 0 && is_branch(world.palette.state(id)) {
                    let x = cx * 16 + (i & 15) as i32;
                    let z = cz * 16 + ((i >> 4) & 15) as i32;
                    let y = MIN_Y + si as i32 * 16 + (i >> 8) as i32;
                    cells.push((x, y, z));
                }
            }
        }
    }
    for (x, y, z) in cells {
        let name = world.get(x, y, z).name.clone();
        let flag = |v: bool| if v { "true" } else { "false" };
        let below = world.get(x, y - 1, z);
        let up = is_branch(below) || sturdy(below);
        let north = is_branch(world.get(x, y, z - 1));
        let east = is_branch(world.get(x + 1, y, z));
        let south = is_branch(world.get(x, y, z + 1));
        let west = is_branch(world.get(x - 1, y, z));
        let state = catalog
            .state(
                &name,
                &[
                    ("up", flag(up)),
                    ("north", flag(north)),
                    ("east", flag(east)),
                    ("south", flag(south)),
                    ("west", flag(west)),
                    ("waterlogged", "false"),
                ],
            )
            .expect("branch states are validated when the generator starts");
        world.set(x, y, z, &state);
    }
}
