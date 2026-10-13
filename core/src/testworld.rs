//! Phase 0 test map: a square meadow of Conquest Reforged layer blocks with a pond, a sandy
//! shore, meadow plants and oaks built from Conquest trunk and branch pieces.
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
    pillar: BlockState,
    leaves: BlockState,
    grass_layers: Vec<BlockState>,
    clover_layers: Vec<BlockState>,
    sand_layers: Vec<BlockState>,
    gravel_layers_wet: Vec<BlockState>,
    sand_layers_wet: Vec<BlockState>,
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
            // layer=3 is the thickest pillar, 12 px wide.
            pillar: c.state(
                "conquest:oak_log_pillar",
                &[("layer", "3"), ("down", "false"), ("waterlogged", "false")],
            )?,
            leaves: c.state(
                "minecraft:oak_leaves",
                &[("persistent", "true"), ("distance", "1")],
            )?,
            grass_layers: layer_states(c, "conquest:grass_block_layer", false)?,
            clover_layers: layer_states(c, "conquest:clover_covered_grass_layer", false)?,
            sand_layers: layer_states(c, "conquest:sand_layer", false)?,
            gravel_layers_wet: layer_states(c, "conquest:gravel_layer", true)?,
            sand_layers_wet: layer_states(c, "conquest:wet_sand_layer", true)?,
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
    if d < 2.0 {
        // Gentle banks so layers can follow them without one-block steps.
        let t = (1.0 - d / 2.0).max(0.0);
        h -= 8.5 * t * t * (3.0 - 2.0 * t);
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
            let shallow = underwater && h > WATER_Y as f64 - 1.5;
            let beach = !underwater && pond_distance < 2.0 && h < WATER_Y as f64 + 1.6;
            let clover = fbm2(opts.seed ^ 0xC10, x as f64 / 24.0, z as f64 / 24.0, 2) > 0.25;

            world.set(x, MIN_Y, z, &b.bedrock);
            for y in MIN_Y + 1..=top {
                let state = if y <= top - 4 {
                    &b.stone
                } else if underwater && !shallow {
                    &b.gravel
                } else if underwater || beach {
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
                // Underwater layers always sit at or below the water line, so they are waterlogged.
                let state = if shallow {
                    &b.sand_layers_wet[l]
                } else if underwater {
                    &b.gravel_layers_wet[l]
                } else if beach {
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
            // Crowns reach about 10 blocks out from the trunk.
            let x = gx * cell + rng.range(11, cell - 11);
            let z = gz * cell + rng.range(11, cell - 11);
            if x < 11 || z < 11 || x >= size - 11 || z >= size - 11 {
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

const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

fn piece(name: &str) -> BlockState {
    // Connections are filled in later by `connect_branches`.
    BlockState {
        name: name.to_string(),
        props: vec![],
    }
}

/// An oak built the way Conquest builders do it: a tapering trunk (full logs, a 12 px pillar,
/// then large-branch posts), limbs that run out and climb while thinning from large to small
/// branches and fork at the ends, and loose leaf clumps on the twig ends, so the branch
/// structure stays visible under and between them.
fn build_oak(world: &mut World, b: &Blocks, rng: &mut Rng, x: i32, base: i32, z: i32) {
    let big = rng.unit() < 0.65;
    let logs = if big { rng.range(3, 4) } else { 2 };
    let pillars = rng.range(1, 2);
    let posts = if big { rng.range(4, 5) } else { 3 };
    for y in base..base + logs {
        world.set(x, y, z, &b.log);
    }
    for y in base + logs..base + logs + pillars {
        world.set(x, y, z, &b.pillar);
    }
    let crown = base + logs + pillars;
    for y in crown..crown + posts {
        world.set(x, y, z, &piece(&b.large_branch));
    }

    // One limb per side, at different heights so limbs never stack into walls.
    let mut tips = Vec::new();
    let sides = if big { 4 } else { 3 };
    let start = rng.range(0, 3) as usize;
    for i in 0..sides {
        let y = crown + i as i32 * (posts - 1) / sides as i32;
        grow_limb(world, b, rng, &mut tips, (x, y, z), (start + i) % 4, big);
    }

    // The leader carries on up, splits into two twigs and gets the top clump.
    let lead = rng.range(1, 2);
    for k in 0..lead {
        world.set(x, crown + posts + k, z, &piece(&b.branch));
    }
    let ty = crown + posts + lead - 1;
    tips.push((x, ty + 1, z));
    for k in 0..2 {
        let (dx, dz) = DIRS[(start + 1 + 2 * k) % 4];
        if place(world, x + dx, ty, z + dz, &b.small_branch) {
            tips.push((x + dx, ty, z + dz));
        }
    }

    // Loose, flattened clumps above the twig ends; the space under them stays open.
    let rx = if big { 2.4 } else { 1.9 };
    for (tx, ty, tz) in tips {
        let r = rx as i32 + 1;
        for dx in -r..=r {
            for dz in -r..=r {
                for dy in -1..=2 {
                    let d = (dx * dx + dz * dz) as f64 / (rx * rx)
                        + (dy as f64 - 0.4).powi(2) / (1.5 * 1.5);
                    if d > 1.0 || (d > 0.45 && rng.unit() < 0.3) {
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

/// Places a branch piece if the spot is free. Connections are filled in by `connect_branches`.
fn place(world: &mut World, x: i32, y: i32, z: i32, name: &str) -> bool {
    if !world.get(x, y, z).is_air() {
        return false;
    }
    world.set(x, y, z, &piece(name));
    true
}

/// One limb: a thick stretch out of the trunk, a climb, a thinner stretch further out (sometimes
/// with a sideways jog), then a fork into two small twigs whose ends get leaf clumps.
fn grow_limb(
    world: &mut World,
    b: &Blocks,
    rng: &mut Rng,
    tips: &mut Vec<(i32, i32, i32)>,
    from: (i32, i32, i32),
    dir: usize,
    big: bool,
) {
    let (mut cx, mut cy, mut cz) = from;
    let (dx, dz) = DIRS[dir];
    let side = DIRS[(dir + if rng.unit() < 0.5 { 1 } else { 3 }) % 4];

    let (inner, outer) = if big {
        (rng.range(2, 3), rng.range(2, 3))
    } else {
        (rng.range(1, 2), rng.range(1, 2))
    };

    for _ in 0..inner {
        cx += dx;
        cz += dz;
        place(world, cx, cy, cz, &b.large_branch);
    }
    for _ in 0..rng.range(1, 2) {
        cy += 1;
        place(world, cx, cy, cz, &b.branch);
    }
    for k in 0..outer {
        let (sx, sz) = if k == 1 && rng.unit() < 0.4 {
            side
        } else {
            (dx, dz)
        };
        cx += sx;
        cz += sz;
        place(world, cx, cy, cz, &b.branch);
    }

    // Fork: one twig carries on outwards, the other goes to the side.
    let (ox, oz) = (cx + dx, cz + dz);
    if place(world, ox, cy, oz, &b.small_branch) {
        tips.push((ox, cy, oz));
    }
    let (fx, fz) = (cx - side.0, cz - side.1);
    if place(world, fx, cy, fz, &b.small_branch) {
        if rng.unit() < 0.5 && place(world, fx - side.0, cy, fz - side.1, &b.small_branch) {
            tips.push((fx - side.0, cy, fz - side.1));
        } else {
            tips.push((fx, cy, fz));
        }
    }
}

/// Fills in the connection properties the game would compute on placement: an arm towards
/// every neighbouring branch piece, and a post (`up`) when the block below is sturdy, a branch
/// or a log pillar.
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
        // A pillar's top face is not sturdy, so the game itself would drop the post on the
        // next block update; keep it so the trunk reads as one piece.
        let up = is_branch(below) || sturdy(below) || catalog.is_a(&below.name, "LogPillar");
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
