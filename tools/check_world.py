"""Independent checker for a generated world: parses region files with nbtlib, validates the
1.20.1 chunk layout, block states (against the catalog) and 3D biomes, and renders a top-down
preview.

    python3 tools/check_world.py <world folder> <catalog.json> <preview.png> [--regions r.0.0,r.1.0]

By default every region file is checked."""
import sys, zlib, json, struct, io, math, os, collections
import numpy as np
import nbtlib
from PIL import Image

world, catalog_path, png = sys.argv[1], sys.argv[2], sys.argv[3]
only = None
if '--regions' in sys.argv:
    only = set(sys.argv[sys.argv.index('--regions') + 1].split(','))
cat = json.load(open(catalog_path))['blocks']

BIOMES = set('minecraft:' + b for b in '''badlands bamboo_jungle basalt_deltas beach birch_forest
cherry_grove cold_ocean crimson_forest dark_forest deep_cold_ocean deep_dark deep_frozen_ocean
deep_lukewarm_ocean deep_ocean desert dripstone_caves end_barrens end_highlands end_midlands
eroded_badlands flower_forest forest frozen_ocean frozen_peaks frozen_river grove ice_spikes
jagged_peaks jungle lukewarm_ocean lush_caves mangrove_swamp meadow mushroom_fields nether_wastes
ocean old_growth_birch_forest old_growth_pine_taiga old_growth_spruce_taiga plains river savanna
savanna_plateau small_end_islands snowy_beach snowy_plains snowy_slopes snowy_taiga
soul_sand_valley sparse_jungle stony_peaks stony_shore sunflower_plains swamp taiga the_end
the_void warm_ocean warped_forest windswept_forest windswept_gravelly_hills windswept_hills
windswept_savanna wooded_badlands'''.split())

lvl = nbtlib.load(world + '/level.dat')
data = lvl['Data']
print('DataVersion', int(data['DataVersion']), 'version', int(data['version']), 'Version', str(data['Version']['Name']))
border = float(data['BorderSize'])
print('spawn', int(data['SpawnX']), int(data['SpawnY']), int(data['SpawnZ']), 'border', float(data['BorderCenterX']), float(data['BorderCenterZ']), border)

def ceil_log2(n):
    return 0 if n <= 1 else (n - 1).bit_length()

def unpack(longs, bits, count):
    """Values packed `64 // bits` per long, low bits first, never spanning two longs."""
    per = 64 // bits
    a = np.array([int(v) & 0xFFFFFFFFFFFFFFFF for v in longs], dtype=np.uint64)
    shifts = np.arange(per, dtype=np.uint64) * np.uint64(bits)
    vals = (a[:, None] >> shifts[None, :]) & np.uint64((1 << bits) - 1)
    return vals.reshape(-1)[:count].astype(np.int64)

size = int(border)
top_y = np.full((size, size), -999, dtype=np.int32)
top_name = np.empty((size, size), dtype=object)
water_top = np.full((size, size), -999, dtype=np.int32)
states = collections.Counter()
checked = set()
biome_counts = collections.Counter()
biome_at_water = collections.Counter()   # biome of the cell holding each column's water surface
biome_above_water = collections.Counter()
problems = []
chunks = 0

def check_state(name, props):
    key = name + json.dumps(props, sort_keys=True)
    if key in checked:
        return
    checked.add(key)
    info = cat.get(name)
    if info is None:
        problems.append('unknown block ' + name)
        return
    want = info.get('properties', {})
    if set(want) != set(props):
        problems.append(f'{name}: props {sorted(props)} != {sorted(want)}')
    for k, v in props.items():
        if v not in want.get(k, []):
            problems.append(f'{name}: {k}={v} not allowed')

regions = sorted(f for f in os.listdir(world + '/region') if f.endswith('.mca'))
for fname in regions:
    if only and fname[:-4] not in only:
        continue
    raw = open(world + '/region/' + fname, 'rb').read()
    assert len(raw) % 4096 == 0, 'region not sector aligned'
    for slot in range(1024):
        loc = struct.unpack('>I', raw[slot*4:slot*4+4])[0]
        if loc == 0:
            continue
        off, cnt = loc >> 8, loc & 0xFF
        length = struct.unpack('>I', raw[off*4096:off*4096+4])[0]
        assert raw[off*4096+4] == 2, 'not zlib'
        assert 4 + length <= cnt * 4096, 'chunk overflows its sectors'
        nbt = nbtlib.File.parse(io.BytesIO(zlib.decompress(raw[off*4096+5:off*4096+4+length])))
        chunks += 1
        cx, cz = int(nbt['xPos']), int(nbt['zPos'])
        assert slot == (cx & 31) + (cz & 31) * 32, 'slot mismatch'
        assert int(nbt['DataVersion']) == 3465 and str(nbt['Status']) == 'minecraft:full' and int(nbt['yPos']) == -4
        assert 'Heightmaps' not in nbt and int(nbt['isLightOn']) == 0
        secs = nbt['sections']
        # 24 sections in a normal world, more when a data pack makes the overworld taller.
        assert [int(s['Y']) for s in secs] == list(range(-4, -4 + len(secs))), 'section Ys'
        assert len(secs) >= 24, 'section count'
        # (y, z, x) arrays for the whole chunk: block names and biome names per 4×4×4 cell.
        col_top = np.full((16, 16), -999, dtype=np.int32)
        col_top_name = np.empty((16, 16), dtype=object)
        col_water = np.full((16, 16), -999, dtype=np.int32)
        cells_y = len(secs) * 4
        cell_biome = np.empty((cells_y, 4, 4), dtype=object)
        for s in secs:
            sy = int(s['Y'])
            bs = s['block_states']
            pal = [(str(p['Name']), {k: str(v) for k, v in p.get('Properties', {}).items()}) for p in bs['palette']]
            if len(pal) == 1:
                assert 'data' not in bs
                idx = np.zeros(4096, dtype=np.int64)
            else:
                bits = max(4, ceil_log2(len(pal)))
                assert len(bs['data']) == math.ceil(4096 / (64 // bits)), ('data length', len(bs['data']), bits)
                idx = unpack(bs['data'], bits, 4096)
                assert idx.max() < len(pal)
            for name, props in pal:
                check_state(name, props)
            counts = np.bincount(idx, minlength=len(pal))
            for (name, props), n in zip(pal, counts):
                if n:
                    states[name + json.dumps(props, sort_keys=True)] += int(n)
            grid = idx.reshape(16, 16, 16)   # y, z, x
            names = np.array([p[0] for p in pal], dtype=object)
            solid = names[grid] != 'minecraft:air'
            for ly in range(16):
                y = sy * 16 + ly
                layer = solid[ly]
                col_top[layer] = y
                col_top_name[layer] = names[grid[ly]][layer]
                col_water[names[grid[ly]] == 'minecraft:water'] = y

            bm = s['biomes']
            bpal = [str(b) for b in bm['palette']]
            assert 1 <= len(bpal) <= 8, ('biome palette size', len(bpal))
            for b in bpal:
                if b not in BIOMES:
                    problems.append('unknown biome ' + b)
            if len(bpal) == 1:
                assert 'data' not in bm
                bidx = np.zeros(64, dtype=np.int64)
            else:
                bits = ceil_log2(len(bpal))
                assert len(bm['data']) == math.ceil(64 / (64 // bits)), ('biome data length', len(bm['data']), bits)
                bidx = unpack(bm['data'], bits, 64)
                assert bidx.max() < len(bpal)
            bnames = np.array(bpal, dtype=object)[bidx].reshape(4, 4, 4)   # y, z, x
            cell_biome[(sy + 4) * 4:(sy + 4) * 4 + 4] = bnames
            for b, n in zip(*np.unique(bnames, return_counts=True)):
                biome_counts[b] += int(n)

        x0, z0 = cx * 16, cz * 16
        top_y[z0:z0+16, x0:x0+16] = col_top
        top_name[z0:z0+16, x0:x0+16] = col_top_name
        water_top[z0:z0+16, x0:x0+16] = col_water
        for lz in range(16):
            for lx in range(16):
                w = col_water[lz, lx]
                if w == -999:
                    continue
                cy = (w + 64) // 4
                biome_at_water[cell_biome[cy, lz // 4, lx // 4]] += 1
                if col_top[lz, lx] > w and cy + 1 < cells_y:
                    biome_above_water[cell_biome[cy + 1, lz // 4, lx // 4]] += 1

print('chunks', chunks, 'distinct states', len(states), 'problems', problems[:10] or 'none')
for name, n in states.most_common(12):
    print(f'  {n:10d} {name}')
print('biome cells', dict(biome_counts.most_common(8)))
print('biome at water surface', dict(biome_at_water.most_common(6)))
print('biome above water (columns with plants or trees in water)', dict(biome_above_water.most_common(6)))

# Top-down preview: top block colour with simple hill shading.
def rgb(name):
    info = cat.get(name, {})
    face = info.get('top') or {}
    c = int(face.get('rgb', '808080'), 16)
    if 'tint' in face:
        t = int(face['tint'], 16)
        c = sum(((((c >> s) & 255) * ((t >> s) & 255) // 255) << s) for s in (16, 8, 0))
    if name == 'minecraft:water':
        c = 0x3f76e4
    if name.endswith('leaves'):
        c = 0x4a7a2a
    return c
colors = {}
img = np.zeros((size, size, 3), dtype=np.uint8)
for z in range(size):
    for x in range(size):
        name = top_name[z, x]
        if name is None:
            continue
        if name not in colors:
            colors[name] = rgb(name)
        c = colors[name]
        y = top_y[z, x]
        yw = top_y[z - 1, x - 1] if x and z and top_name[z - 1, x - 1] is not None else y
        shade = max(0.6, min(1.4, 1.0 + (y - yw) * 0.12))
        img[z, x] = [min(255, int(((c >> s) & 255) * shade)) for s in (16, 8, 0)]
out = Image.fromarray(img)
if size < 768:
    out = out.resize((768, 768), Image.NEAREST)
out.save(png)
print('preview', png)
