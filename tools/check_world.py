"""Independent checker for a generated world: parses region files with nbtlib, validates the
1.20.1 chunk layout and block states against the catalog, and renders a top-down preview."""
import sys, zlib, json, struct, io, math, collections
import nbtlib
from PIL import Image

world, catalog_path, png = sys.argv[1], sys.argv[2], sys.argv[3]
cat = json.load(open(catalog_path))['blocks']

lvl = nbtlib.load(world + '/level.dat')
data = lvl['Data']
print('level.dat keys:', sorted(data.keys()))
print('DataVersion', int(data['DataVersion']), 'version', int(data['version']), 'Version', dict(data['Version']))
print('spawn', int(data['SpawnX']), int(data['SpawnY']), int(data['SpawnZ']), 'border', float(data['BorderCenterX']), float(data['BorderCenterZ']), float(data['BorderSize']))
wgs = data['WorldGenSettings']
print('dimensions', list(wgs['dimensions'].keys()), 'overworld gen', wgs['dimensions']['minecraft:overworld']['generator'].snbt()[:200])

def ceil_log2(n):
    return 0 if n <= 1 else (n - 1).bit_length()

def unpack(longs, bits, count):
    per = 64 // bits
    mask = (1 << bits) - 1
    out = []
    for i in range(count):
        v = longs[i // per] & 0xFFFFFFFFFFFFFFFF
        out.append((v >> ((i % per) * bits)) & mask)
    return out

raw = open(world + '/region/r.0.0.mca', 'rb').read()
assert len(raw) % 4096 == 0, 'region not sector aligned'
top = {}   # (x,z) -> (y, name)
states_seen = collections.Counter()
problems = []
chunks = 0
column = collections.defaultdict(dict)
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
    secs = nbt['sections']
    assert [int(s['Y']) for s in secs] == list(range(-4, 20)), 'section Ys'
    for s in secs:
        sy = int(s['Y'])
        bs = s['block_states']
        pal = [(str(p['Name']), {k: str(v) for k, v in p.get('Properties', {}).items()}) for p in bs['palette']]
        if len(pal) == 1:
            assert 'data' not in bs
            idx = [0] * 4096
        else:
            bits = max(4, ceil_log2(len(pal)))
            longs = [int(v) for v in bs['data']]
            assert len(longs) == math.ceil(4096 / (64 // bits)), ('data length', len(longs), bits)
            idx = unpack(longs, bits, 4096)
            assert max(idx) < len(pal)
        for name, props in pal:
            key = name + json.dumps(props, sort_keys=True)
            if key in states_seen:
                continue
            info = cat.get(name)
            if info is None:
                problems.append('unknown block ' + name)
            else:
                want = info.get('properties', {})
                if set(want) != set(props):
                    problems.append(f'{name}: props {sorted(props)} != {sorted(want)}')
                for k, v in props.items():
                    if v not in want.get(k, []):
                        problems.append(f'{name}: {k}={v} not allowed')
        for name, props in pal:
            states_seen[name + json.dumps(props, sort_keys=True)] += 0
        if len(pal) == 1 and pal[0][0] == 'minecraft:air':
            continue
        for i, pi in enumerate(idx):
            name = pal[pi][0]
            if name == 'minecraft:air':
                continue
            x, z, y = cx*16 + (i & 15), cz*16 + ((i >> 4) & 15), sy*16 + (i >> 8)
            states_seen[name + json.dumps(pal[pi][1], sort_keys=True)] += 1
            if (x, z) not in top or y > top[(x, z)][0]:
                top[(x, z)] = (y, name)
            if (x, z) in ((128, 128), (60, 60)):
                column[(x, z)][y] = name + (str(pal[pi][1]) if pal[pi][1] else '')
    assert 'Heightmaps' not in nbt and int(nbt['isLightOn']) == 0

print('chunks', chunks, 'distinct states', len(states_seen), 'problems', problems[:10] or 'none')
for name, n in sorted(((k, v) for k, v in states_seen.items()), key=lambda kv: -kv[1])[:12]:
    print(f'  {n:8d} {name}')
for xz, col in column.items():
    ys = sorted(col)[-6:]
    print('column', xz, [(y, col[y]) for y in ys])

# Top-down preview: top block colour with simple hill shading.
size = 256
img = Image.new('RGB', (size, size))
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
for (x, z), (y, name) in top.items():
    c = rgb(name)
    yw = top.get((x - 1, z - 1), (y, None))[0]
    shade = max(0.6, min(1.4, 1.0 + (y - yw) * 0.12))
    img.putpixel((x, z), tuple(min(255, int(((c >> s) & 255) * shade)) for s in (16, 8, 0)))
img = img.resize((768, 768), Image.NEAREST)
img.save(png)
print('preview', png)
