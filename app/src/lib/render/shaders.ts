// GLSL for render mode: the sky, the path tracer, the denoiser and the final picture.
//
// The tracer walks the block grid of the scene (see scene.ts) with a height pyramid that
// skips the air above the ground, then the cells of each column it crosses. Light comes from
// the sun (with soft shadows), the sky, fog and water. Every frame adds one sample per pixel
// to running averages; the denoiser smooths what is still noisy, guided by normals and depth,
// and leaves textures alone because it works on light divided by the surface colour.

const HEADER = /* glsl */ `#version 300 es
precision highp float;
precision highp int;
precision highp usampler2D;
precision highp sampler2D;
`;

export const FULLSCREEN_VS = /* glsl */ `#version 300 es
const vec2 P[3] = vec2[3](vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
void main() { gl_Position = vec4(P[gl_VertexID], 0.0, 1.0); }
`;

/** Single scattering in a Rayleigh and Mie atmosphere (after Nishita), into a
 * latitude-longitude sky texture. Directions below the horizon fade to a dim haze. */
export const SKY_FS =
  HEADER +
  /* glsl */ `
uniform vec3 uSunDir;
uniform vec2 uSize;
uniform float uSunPower;
out vec4 oColor;

const float PI = 3.14159265;
const float R_PLANET = 6371e3;
const float R_ATMOS = 6471e3;
const vec3 K_RLH = vec3(5.5e-6, 13.0e-6, 22.4e-6);
const float K_MIE = 21e-6;
const float SH_RLH = 8e3;
const float SH_MIE = 1.2e3;
const float G = 0.758;

vec2 sphere(vec3 r0, vec3 rd, float sr) {
  float a = dot(rd, rd);
  float b = 2.0 * dot(rd, r0);
  float c = dot(r0, r0) - sr * sr;
  float d = b * b - 4.0 * a * c;
  if (d < 0.0) return vec2(1e5, -1e5);
  return vec2((-b - sqrt(d)) / (2.0 * a), (-b + sqrt(d)) / (2.0 * a));
}

vec3 atmosphere(vec3 r, vec3 pSun) {
  vec3 r0 = vec3(0.0, R_PLANET + 100.0, 0.0);
  vec2 p = sphere(r0, r, R_ATMOS);
  if (p.x > p.y) return vec3(0.0);
  p.y = min(p.y, sphere(r0, r, R_PLANET).x > 0.0 ? sphere(r0, r, R_PLANET).x : p.y);
  float iStep = (p.y - max(p.x, 0.0)) / 16.0;
  float iTime = max(p.x, 0.0);
  vec3 totalRlh = vec3(0.0);
  vec3 totalMie = vec3(0.0);
  float iOdRlh = 0.0;
  float iOdMie = 0.0;
  float mu = dot(r, pSun);
  float mumu = mu * mu;
  float gg = G * G;
  float pRlh = 3.0 / (16.0 * PI) * (1.0 + mumu);
  float pMie = 3.0 / (8.0 * PI) * ((1.0 - gg) * (mumu + 1.0)) / (pow(1.0 + gg - 2.0 * mu * G, 1.5) * (2.0 + gg));
  for (int i = 0; i < 16; i++) {
    vec3 iPos = r0 + r * (iTime + iStep * 0.5);
    float iHeight = length(iPos) - R_PLANET;
    float odStepRlh = exp(-iHeight / SH_RLH) * iStep;
    float odStepMie = exp(-iHeight / SH_MIE) * iStep;
    iOdRlh += odStepRlh;
    iOdMie += odStepMie;
    float jStep = sphere(iPos, pSun, R_ATMOS).y / 8.0;
    float jTime = 0.0;
    float jOdRlh = 0.0;
    float jOdMie = 0.0;
    for (int j = 0; j < 8; j++) {
      vec3 jPos = iPos + pSun * (jTime + jStep * 0.5);
      float jHeight = length(jPos) - R_PLANET;
      jOdRlh += exp(-jHeight / SH_RLH) * jStep;
      jOdMie += exp(-jHeight / SH_MIE) * jStep;
      jTime += jStep;
    }
    vec3 attn = exp(-(K_MIE * (iOdMie + jOdMie) + K_RLH * (iOdRlh + jOdRlh)));
    totalRlh += odStepRlh * attn;
    totalMie += odStepMie * attn;
    iTime += iStep;
  }
  return uSunPower * (pRlh * K_RLH * totalRlh + pMie * K_MIE * totalMie);
}

void main() {
  vec2 uv = gl_FragCoord.xy / uSize;
  float phi = (uv.x - 0.5) * 2.0 * PI;
  float theta = uv.y * PI;
  vec3 d = vec3(sin(theta) * cos(phi), cos(theta), sin(theta) * sin(phi));
  vec3 c;
  if (d.y >= 0.0) {
    c = atmosphere(normalize(vec3(d.x, max(d.y, 0.002), d.z)), uSunDir);
  } else {
    vec3 h = atmosphere(normalize(vec3(d.x, 0.002, d.z)), uSunDir);
    c = h * mix(1.0, 0.25, smoothstep(0.0, 0.35, -d.y));
  }
  // A faint night sky so the scene never goes fully black.
  c += vec3(0.0015, 0.002, 0.004);
  oColor = vec4(c, 1.0);
}
`;

export const TRACE_FS =
  HEADER +
  /* glsl */ `
uniform usampler2D uColumns;
uniform sampler2D uWaterCol;
uniform usampler2D uVoxels;
uniform usampler2D uTops;
uniform usampler2D uStates;
uniform sampler2D uQuads;
uniform sampler2D uAtlas;
uniform sampler2D uSky;

uniform int uSide;
uniform int uLevels;
uniform int uVoxW;
uniform int uStateW;
uniform int uQuadW;
uniform int uTile;
uniform int uTilesPerRow;
uniform vec3 uBoxMin;
uniform vec3 uBoxMax;
uniform vec2 uOrigin;

uniform vec3 uCamPos;
uniform vec3 uCamRight;
uniform vec3 uCamUp;
uniform vec3 uCamFwd;
uniform float uTanHalfFov;
uniform float uAspect;
uniform float uAperture;
uniform float uFocus;
uniform vec2 uRes;
uniform uint uFrame;

uniform vec3 uSunDir;
uniform vec3 uSunE;
uniform float uSunCos;
uniform vec3 uAmbient;
uniform float uFogDensity;
uniform float uFogBase;
uniform float uFogHeight;
uniform float uFogFar;
uniform vec3 uFogColor;
uniform float uWaterClarity;
uniform float uWaves;
uniform int uBounces;
uniform float uCloudY;
uniform float uCloudCover;

layout(location = 0) out vec4 oLm;
layout(location = 1) out vec4 oM;
layout(location = 2) out vec4 oLa;
layout(location = 3) out vec4 oGuide;

const float PI = 3.14159265;
const float FAR = 1e6;
const int SURF = 1;
const int WENTER = 2;
const int WEXIT = 3;
const uint FULL = 1u;
const uint WATER = 2u;
const uint WATERLOGGED = 4u;
const uint WATER_TOP = 8u;

uint rng;
uint pcg(uint v) {
  uint s = v * 747796405u + 2891336453u;
  uint w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
  return (w >> 22u) ^ w;
}
float rand() {
  rng = pcg(rng);
  return float(rng >> 8u) * (1.0 / 16777216.0);
}

vec3 unpackRgb(uint v) {
  return vec3(float((v >> 16u) & 255u), float((v >> 8u) & 255u), float(v & 255u)) / 255.0;
}
vec3 toLinear(vec3 c) {
  return mix(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), step(0.04045, c));
}
float lum(vec3 c) { return dot(c, vec3(0.2126, 0.7152, 0.0722)); }

uvec4 stateTexel(uint s, int k) {
  int i = int(s) * 3 + k;
  return texelFetch(uStates, ivec2(i % uStateW, i / uStateW), 0);
}
vec4 quadTexel(int q, int k) {
  int i = q * 5 + k;
  return texelFetch(uQuads, ivec2(i % uQuadW, i / uQuadW), 0);
}
uint voxelAt(uint i) {
  return texelFetch(uVoxels, ivec2(int(i % uint(uVoxW)), int(i / uint(uVoxW))), 0).r;
}
vec4 atlasTexel(int tile, vec2 uv) {
  ivec2 base = ivec2(tile % uTilesPerRow, tile / uTilesPerRow) * uTile;
  ivec2 p = clamp(ivec2(floor(uv * float(uTile))), ivec2(0), ivec2(uTile - 1));
  return texelFetch(uAtlas, base + p, 0);
}
ivec2 columnOf(vec2 p) {
  return clamp(ivec2(floor(p)), ivec2(0), ivec2(uSide - 1));
}
vec3 tintOf(float kind, float fixedRgb, ivec2 c) {
  if (kind < 0.5) return vec3(1.0);
  uvec4 col = texelFetch(uColumns, c, 0);
  vec3 s;
  if (kind < 1.5) s = unpackRgb(col.z);
  else if (kind < 2.5) s = unpackRgb(col.w);
  else if (kind < 3.5) s = texelFetch(uWaterCol, c, 0).rgb;
  else s = unpackRgb(uint(fixedRgb));
  return toLinear(s);
}
uint hash3(ivec3 c) {
  return pcg(uint(c.x) * 73856093u ^ uint(c.y) * 19349663u ^ uint(c.z) * 83492791u);
}

struct Hit {
  float t;
  vec3 n;
  vec3 albedo;
  int kind;
};

// The surface point of quad qi at offset p from its corner o: texture coordinates.
vec2 quadUv(int qi, vec3 U, vec3 V, vec3 n, vec3 p, vec4 q3, vec4 q4) {
  float nn = dot(n, n);
  float a = dot(cross(V, p), n) / nn;
  float b = dot(cross(p, U), n) / nn;
  return q3.xy + q3.zw * a + q4.xy * b;
}

// What the ray meets in one cell, between tc0 and tc1, having come in through the face whose
// outward normal is en.
bool cellHit(uint s, ivec3 cell, vec3 ro, vec3 rd, vec3 inv, float tc0, float tc1, vec3 en, bool inWater, inout Hit h) {
  uvec4 hd = stateTexel(s, 0);
  uint flags = hd.x;
  bool wet = (flags & (WATER | WATERLOGGED)) != 0u;
  if (inWater && !wet && (flags & FULL) == 0u) {
    h.t = tc0;
    h.n = en;
    h.kind = WEXIT;
    return true;
  }
  float best = tc1;
  bool found = false;
  if (wet) {
    float wh = (flags & WATER_TOP) != 0u ? 0.875 : 1.0;
    vec3 a = (vec3(cell) - ro) * inv;
    vec3 b = (vec3(cell) + vec3(1.0, wh, 1.0) - ro) * inv;
    vec3 lo = min(a, b);
    vec3 hi = max(a, b);
    float tn = max(max(lo.x, lo.y), lo.z);
    float tf = min(min(hi.x, hi.y), hi.z);
    if (!inWater) {
      if (tn <= tf && tn >= tc0 - 1e-4 && tn <= best) {
        best = tn;
        h.n = tn == lo.y ? vec3(0.0, -sign(rd.y), 0.0) : (tn == lo.x ? vec3(-sign(rd.x), 0.0, 0.0) : vec3(0.0, 0.0, -sign(rd.z)));
        h.kind = WENTER;
        found = true;
      }
    } else if (wh < 1.0 && rd.y > 0.0) {
      float ts = (float(cell.y) + wh - ro.y) * inv.y;
      if (ts >= tc0 - 1e-4 && ts <= best) {
        best = ts;
        h.n = vec3(0.0, -1.0, 0.0);
        h.kind = WEXIT;
        found = true;
      }
    }
  }
  if ((flags & WATER) == 0u) {
    uint slots = max(hd.y, 1u);
    uint slot = slots > 1u ? hash3(cell + ivec3(uOrigin.x, 0, uOrigin.y)) % slots : 0u;
    uvec4 pr = stateTexel(s, 1 + int(slot >> 1u));
    int off = int((slot & 1u) == 0u ? pr.x : pr.z);
    int cnt = int((slot & 1u) == 0u ? pr.y : pr.w);
    vec3 base = vec3(cell);
    if ((flags & FULL) != 0u) {
      if (tc0 <= best) {
        int face = en.y < -0.5 ? 0 : en.y > 0.5 ? 1 : en.z < -0.5 ? 2 : en.z > 0.5 ? 3 : en.x < -0.5 ? 4 : 5;
        int qi = off + face;
        vec4 q0 = quadTexel(qi, 0);
        vec4 q1 = quadTexel(qi, 1);
        vec4 q2 = quadTexel(qi, 2);
        vec3 U = q1.xyz;
        vec3 V = q2.xyz;
        vec3 n = cross(V, U);
        vec2 uv = quadUv(qi, U, V, n, ro + rd * tc0 - (base + q0.xyz), quadTexel(qi, 3), quadTexel(qi, 4));
        best = tc0;
        found = true;
        h.kind = SURF;
        h.n = en;
        h.albedo = atlasTexel(int(q0.w), uv).rgb * tintOf(q1.w, q2.w, cell.xz);
      }
    } else {
      for (int i = 0; i < 64; i++) {
        if (i >= cnt) break;
        int qi = off + i;
        vec4 q0 = quadTexel(qi, 0);
        vec4 q1 = quadTexel(qi, 1);
        vec4 q2 = quadTexel(qi, 2);
        vec3 o = base + q0.xyz;
        vec3 U = q1.xyz;
        vec3 V = q2.xyz;
        vec3 n = cross(V, U);
        float dn = dot(n, rd);
        if (dn >= 0.0) continue;
        float t = dot(n, o - ro) / dn;
        if (t < tc0 - 1e-4 || t > best) continue;
        vec3 p = ro + rd * t - o;
        float nn = dot(n, n);
        float a = dot(cross(V, p), n) / nn;
        float b = dot(cross(p, U), n) / nn;
        if (a < 0.0 || a > 1.0 || b < 0.0 || b > 1.0) continue;
        vec4 q3 = quadTexel(qi, 3);
        vec4 q4 = quadTexel(qi, 4);
        vec4 tex = atlasTexel(int(q0.w), q3.xy + q3.zw * a + q4.xy * b);
        if (q4.z > 0.5 && tex.a < 0.5) continue;
        best = t;
        found = true;
        h.kind = SURF;
        h.n = normalize(n);
        h.albedo = tex.rgb * tintOf(q1.w, q2.w, cell.xz);
      }
    }
  }
  if (found) h.t = best;
  return found;
}

// The cells of column c the ray crosses between tA and tB.
bool columnHit(ivec2 c, vec3 ro, vec3 rd, vec3 inv, float tA, float tB, bool inWater, inout Hit h) {
  uvec4 col = texelFetch(uColumns, c, 0);
  int count = int(col.y >> 16u);
  if (count == 0) return false;
  int bottom = int(col.y & 0xFFFFu) - 64;
  int topEx = bottom + count;
  float tx = ((rd.x > 0.0 ? float(c.x) : float(c.x + 1)) - ro.x) * inv.x;
  float tz = ((rd.z > 0.0 ? float(c.y) : float(c.y + 1)) - ro.z) * inv.z;
  vec3 en = tx > tz ? vec3(-sign(rd.x), 0.0, 0.0) : vec3(0.0, 0.0, -sign(rd.z));
  int stepY = rd.y >= 0.0 ? 1 : -1;
  int y = int(floor(ro.y + rd.y * tA + float(stepY) * 1e-4));
  float tNext = abs(rd.y) < 1e-9 ? FAR : (float(y + (stepY > 0 ? 1 : 0)) - ro.y) * inv.y;
  float tDelta = abs(inv.y);
  float tc0 = tA;
  for (int k = 0; k < 4096; k++) {
    if (tc0 >= tB) break;
    float tc1 = min(tNext, tB);
    if (stepY > 0 && y >= topEx && !inWater) break;
    if (y < bottom) {
      // Solid ground below the stored cells.
      uint s = voxelAt(col.x);
      uvec4 hd = stateTexel(s, 0);
      h.t = tc0;
      h.n = en;
      h.kind = SURF;
      h.albedo = toLinear(unpackRgb(hd.z));
      return true;
    }
    uint s = y < topEx ? voxelAt(col.x + uint(y - bottom)) : 0u;
    if (s != 0u) {
      if (cellHit(s, ivec3(c.x, y, c.y), ro, rd, inv, tc0, tc1, en, inWater, h)) return true;
    } else if (inWater) {
      h.t = tc0;
      h.n = en;
      h.kind = WEXIT;
      return true;
    }
    if (tNext >= tB) break;
    tc0 = tNext;
    tNext += tDelta;
    y += stepY;
    en = vec3(0.0, float(-stepY), 0.0);
  }
  return false;
}

float cloudNoise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  float a = float(pcg(uint(int(i.x)) * 1597u ^ uint(int(i.y)) * 51749u) & 1023u) / 1023.0;
  float b = float(pcg(uint(int(i.x) + 1) * 1597u ^ uint(int(i.y)) * 51749u) & 1023u) / 1023.0;
  float c = float(pcg(uint(int(i.x)) * 1597u ^ uint(int(i.y) + 1) * 51749u) & 1023u) / 1023.0;
  float d = float(pcg(uint(int(i.x) + 1) * 1597u ^ uint(int(i.y) + 1) * 51749u) & 1023u) / 1023.0;
  return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}
bool cloudAt(ivec2 cell) {
  vec2 p = vec2(cell);
  float n = cloudNoise(p * 0.11) * 0.7 + cloudNoise(p * 0.37 + 17.0) * 0.3;
  return n > 1.0 - uCloudCover * 0.75;
}

// Minecraft's flat clouds: 12×12-block cells, 4 blocks thick.
bool cloudHit(vec3 ro, vec3 rd, vec3 inv, float tMax, inout Hit h) {
  if (uCloudCover <= 0.0 || abs(rd.y) < 1e-6) return false;
  float ta = (uCloudY - ro.y) * inv.y;
  float tb = (uCloudY + 4.0 - ro.y) * inv.y;
  float t = max(min(ta, tb), 0.0);
  float t1 = min(max(ta, tb), tMax);
  if (t >= t1) return false;
  vec2 o = (ro.xz + uOrigin) / 12.0;
  vec2 d = rd.xz / 12.0;
  vec2 p = o + d * (t + 1e-3);
  ivec2 cell = ivec2(floor(p));
  ivec2 st = ivec2(d.x >= 0.0 ? 1 : -1, d.y >= 0.0 ? 1 : -1);
  vec2 di = vec2(abs(d.x) < 1e-9 ? 1e9 : 1.0 / d.x, abs(d.y) < 1e-9 ? 1e9 : 1.0 / d.y);
  vec2 tNext = (vec2(cell + max(st, ivec2(0))) - o) * di;
  vec2 tDelta = abs(di);
  vec3 n = vec3(0.0, -sign(rd.y), 0.0);
  for (int i = 0; i < 48; i++) {
    if (t >= t1) return false;
    if (cloudAt(cell)) {
      h.t = t;
      h.n = n;
      h.albedo = vec3(0.9);
      h.kind = SURF;
      return true;
    }
    if (tNext.x < tNext.y) {
      t = tNext.x;
      tNext.x += tDelta.x;
      cell.x += st.x;
      n = vec3(-float(st.x), 0.0, 0.0);
    } else {
      t = tNext.y;
      tNext.y += tDelta.y;
      cell.y += st.y;
      n = vec3(0.0, 0.0, -float(st.y));
    }
  }
  return false;
}

// The first thing the ray meets before tMax: a surface, or the water's edge.
bool trace(vec3 ro, vec3 rd, float tMax, bool inWater, out Hit h) {
  h.kind = 0;
  h.t = tMax;
  h.n = vec3(0.0, 1.0, 0.0);
  h.albedo = vec3(0.0);
  vec3 inv = 1.0 / vec3(abs(rd.x) < 1e-9 ? 1e-9 : rd.x, abs(rd.y) < 1e-9 ? 1e-9 : rd.y, abs(rd.z) < 1e-9 ? 1e-9 : rd.z);
  vec3 a = (uBoxMin - ro) * inv;
  vec3 b = (uBoxMax - ro) * inv;
  vec3 lo = min(a, b);
  vec3 hi = max(a, b);
  float t = max(max(lo.x, lo.y), max(lo.z, 0.0));
  float tExit = min(min(hi.x, hi.y), min(hi.z, tMax));
  bool found = false;
  if (t < tExit) {
    int level = uLevels - 1;
    for (int i = 0; i < 3000; i++) {
      if (t >= tExit) break;
      ivec2 c = columnOf(ro.xz + rd.xz * (t + 1e-4 + t * 1e-6));
      ivec2 cell = c >> level;
      float size = float(1 << level);
      vec2 lo2 = vec2(cell) * size;
      vec2 hi2 = lo2 + size;
      vec2 bx = (vec2(rd.x > 0.0 ? hi2.x : lo2.x, rd.z > 0.0 ? hi2.y : lo2.y) - ro.xz) * inv.xz;
      float tEnd = min(min(bx.x, bx.y), tExit);
      float top = float(texelFetch(uTops, cell, level).r) - 64.0;
      bool above = min(ro.y + rd.y * t, ro.y + rd.y * tEnd) >= top;
      if (!above && level > 0) {
        level--;
        continue;
      }
      if (!above && columnHit(c, ro, rd, inv, t, tEnd, inWater, h)) {
        found = true;
        break;
      }
      t = max(tEnd, t + 1e-4);
      if (level < uLevels - 1) {
        ivec2 nc = columnOf(ro.xz + rd.xz * (t + 1e-4 + t * 1e-6)) >> (level + 1);
        if (nc != (cell >> 1)) level++;
      }
    }
  }
  if (!inWater && cloudHit(ro, rd, inv, found ? h.t : tMax, h)) found = true;
  return found;
}

// Fog: dense below uFogBase, thinning upwards over uFogHeight blocks.
float fogF(float y) {
  float d = y - uFogBase;
  return d < 0.0 ? uFogDensity * d : uFogDensity * uFogHeight * (1.0 - exp(-d / uFogHeight));
}
float fogAt(float y) {
  return uFogDensity * (y < uFogBase ? 1.0 : exp(-(y - uFogBase) / uFogHeight));
}
float fogDepth(vec3 ro, vec3 rd, float t) {
  if (uFogDensity <= 0.0) return 0.0;
  if (abs(rd.y) < 1e-4) return fogAt(ro.y) * t;
  return max((fogF(ro.y + rd.y * t) - fogF(ro.y)) / rd.y, 0.0);
}
// Towards the sky: to the top of the fog when the ray climbs, else far away.
float fogDepthOut(vec3 ro, vec3 rd) {
  if (uFogDensity <= 0.0) return 0.0;
  if (rd.y > 1e-3) return max((uFogDensity * uFogHeight - fogF(ro.y)) / rd.y, 0.0);
  return fogDepth(ro, rd, uFogFar);
}
float phaseHG(float c) {
  float g = 0.6;
  return (1.0 - g * g) / (4.0 * PI * pow(1.0 + g * g - 2.0 * g * c, 1.5));
}

vec3 skyRad(vec3 d) {
  vec2 uv = vec2(atan(d.z, d.x) / (2.0 * PI) + 0.5, acos(clamp(d.y, -1.0, 1.0)) / PI);
  return textureLod(uSky, uv, 0.0).rgb;
}

vec3 frame(vec3 n, out vec3 b) {
  vec3 t = normalize(abs(n.y) < 0.999 ? cross(n, vec3(0.0, 1.0, 0.0)) : cross(n, vec3(1.0, 0.0, 0.0)));
  b = cross(n, t);
  return t;
}
vec3 cosineDir(vec3 n) {
  float r1 = rand();
  float r2 = rand();
  float phi = 2.0 * PI * r1;
  float r = sqrt(r2);
  vec3 b;
  vec3 t = frame(n, b);
  return normalize(t * (r * cos(phi)) + b * (r * sin(phi)) + n * sqrt(max(0.0, 1.0 - r2)));
}
vec3 sunSample() {
  float c = mix(1.0, uSunCos, rand());
  float s = sqrt(max(0.0, 1.0 - c * c));
  float phi = 2.0 * PI * rand();
  vec3 b;
  vec3 t = frame(uSunDir, b);
  return normalize(t * (s * cos(phi)) + b * (s * sin(phi)) + uSunDir * c);
}
vec3 waveNormal(vec2 p) {
  vec2 g = vec2(0.0);
  g += vec2(0.8, 0.6) * cos(dot(p, vec2(0.8, 0.6)) * 1.3) * 1.3 * 0.08;
  g += vec2(-0.5, 0.86) * cos(dot(p, vec2(-0.5, 0.86)) * 2.1 + 1.7) * 2.1 * 0.05;
  g += vec2(0.2, -0.98) * cos(dot(p, vec2(0.2, -0.98)) * 3.7 + 4.1) * 3.7 * 0.025;
  g += vec2(-0.93, -0.37) * cos(dot(p, vec2(-0.93, -0.37)) * 5.3 + 2.3) * 5.3 * 0.012;
  return normalize(vec3(-g.x * uWaves, 1.0, -g.y * uWaves));
}
vec3 waterSigma(vec3 p) {
  vec3 w = toLinear(texelFetch(uWaterCol, columnOf(p.xz), 0).rgb);
  return -log(clamp(w * 0.9 + 0.04, 0.02, 0.98)) / uWaterClarity;
}
vec3 waterGlow(vec3 p) {
  vec3 w = toLinear(texelFetch(uWaterCol, columnOf(p.xz), 0).rgb);
  return w * (uAmbient + uSunE * max(uSunDir.y, 0.0) / PI) * 0.12;
}

void main() {
  rng = pcg(uint(gl_FragCoord.x) * 1973u + uint(gl_FragCoord.y) * 9277u + uFrame * 26699u);
  vec2 pix = gl_FragCoord.xy + vec2(rand(), rand()) - 0.5;
  vec2 ndc = pix / uRes * 2.0 - 1.0;
  vec3 rd = normalize(uCamFwd + ndc.x * uTanHalfFov * uAspect * uCamRight + ndc.y * uTanHalfFov * uCamUp);
  vec3 ro = uCamPos;
  if (uAperture > 0.0) {
    vec3 focal = ro + rd * (uFocus / dot(rd, uCamFwd));
    float r = sqrt(rand()) * uAperture;
    float a = 2.0 * PI * rand();
    ro += uCamRight * (r * cos(a)) + uCamUp * (r * sin(a));
    rd = normalize(focal - ro);
  }

  // The light that reaches the camera is La + M * Ls: La before the first matte surface, M
  // that surface's colour (and whatever dimmed the way to it), Ls the light falling on it.
  vec3 La = vec3(0.0);
  vec3 Ls = vec3(0.0);
  vec3 M = vec3(0.0);
  bool matte = false;
  vec3 thr = vec3(1.0);
  bool inWater = false;
  bool camera = true;
  int diffuse = 0;
  int specular = 0;
  vec4 guide = vec4(0.0);
  bool guided = false;

  // Shadow rays waiting to be traced: one towards the sun from a point in the fog, one from
  // the last surface. They go before the path carries on, through the one trace call below
  // (each call site of trace would be compiled in full).
  bool wantFog = false;
  vec3 fogO = vec3(0.0), fogD = vec3(0.0), fogW = vec3(0.0);
  bool fogMatte = false;
  bool wantSun = false;
  vec3 sunO = vec3(0.0), sunD = vec3(0.0), sunW = vec3(0.0);
  bool sunWater = false;
  bool havePath = true;

  bool shadow = false;
  bool shadowFog = false;
  vec3 so = vec3(0.0);
  vec3 sd = vec3(0.0);
  vec3 sT = vec3(1.0);
  bool sWater = false;

  for (int it = 0; it < 48; it++) {
    if (!shadow) {
      if (wantFog) {
        shadow = true;
        shadowFog = true;
        so = fogO;
        sd = fogD;
        sT = vec3(1.0);
        sWater = false;
        wantFog = false;
      } else if (wantSun) {
        shadow = true;
        shadowFog = false;
        so = sunO;
        sd = sunD;
        sT = vec3(1.0);
        sWater = sunWater;
        wantSun = false;
      } else if (!havePath) {
        break;
      }
    }
    Hit h;
    bool any = trace(shadow ? so : ro, shadow ? sd : rd, FAR, shadow ? sWater : inWater, h);

    if (shadow) {
      // Through water surfaces without bending (no caustics), dimmed by the water crossed.
      if (any && (h.kind == WENTER || h.kind == WEXIT)) {
        if (sWater) sT *= exp(-waterSigma(so) * h.t);
        so += sd * (h.t + 2e-3);
        sWater = h.kind == WENTER;
        continue;
      }
      if (!any && !sWater) {
        vec3 c = sT * exp(-fogDepthOut(so, sd));
        if (!shadowFog) Ls += sunW * c;
        else if (fogMatte) Ls += fogW * c;
        else La += fogW * c;
      }
      shadow = false;
      continue;
    }

    float segEnd = any ? h.t : uFogFar;
    if (!guided) {
      guided = true;
      guide = any ? vec4(h.n, h.t) : vec4(-rd, 1e5);
    }
    // Fog along the way: light scattered towards us from one point of it, and what it dims.
    if (!inWater && uFogDensity > 0.0) {
      float od = fogDepth(ro, rd, segEnd);
      float sigma = od / max(segEnd, 1e-3);
      float keep = exp(-od);
      if (sigma > 1e-7 && od > 1e-6) {
        float s = min(-log(1.0 - rand() * (1.0 - keep)) / sigma, segEnd);
        vec3 ps = ro + rd * s;
        float pdf = sigma * exp(-sigma * s) / (1.0 - keep);
        vec3 w = thr * exp(-fogDepth(ro, rd, s)) * fogAt(ps.y) / pdf * uFogColor;
        vec3 add = w * uAmbient;
        if (matte) Ls += add;
        else La += add;
        fogO = ps;
        fogD = sunSample();
        fogW = w * uSunE * phaseHG(dot(rd, uSunDir));
        fogMatte = matte;
        wantFog = uSunE.g > 0.0;
      }
      thr *= keep;
    }
    if (inWater) {
      vec3 tw = exp(-waterSigma(ro) * segEnd);
      vec3 glow = thr * (1.0 - tw) * waterGlow(ro);
      if (matte) Ls += glow;
      else La += glow;
      thr *= tw;
    }
    if (!any) {
      if (!inWater) {
        vec3 sky = skyRad(rd);
        // The sun itself only where nothing matte came before (that light is in the shadow
        // rays), and a glint on the water kept to a few times a sunlit white wall, so that the
        // rare rays that find it don't speckle the picture.
        if (camera && dot(rd, uSunDir) > uSunCos) {
          vec3 sun = uSunE / (2.0 * PI * (1.0 - uSunCos));
          if (specular > 0) sun *= min(1.0, 2.0 * lum(uSunE) / max(lum(thr * sun), 1e-6));
          sky += sun;
        }
        if (matte) Ls += thr * sky;
        else La += thr * sky;
      }
      havePath = false;
      continue;
    }
    vec3 p = ro + rd * h.t;
    if (h.kind == WENTER || h.kind == WEXIT) {
      specular++;
      if (specular > 6) {
        havePath = false;
        continue;
      }
      vec3 n = h.n;
      if (abs(n.y) > 0.5 && uWaves > 0.0) {
        vec3 w = waveNormal(p.xz + uOrigin);
        w.y *= sign(n.y);
        if (dot(w, rd) < 0.0) n = w;
      }
      bool entering = h.kind == WENTER;
      float cosi = clamp(-dot(rd, n), 0.0, 1.0);
      vec3 refr = refract(rd, n, entering ? 1.0 / 1.333 : 1.333);
      float f = dot(refr, refr) < 1e-6 ? 1.0 : 0.0204 + 0.9796 * pow(1.0 - cosi, 5.0);
      if (rand() < f) {
        rd = reflect(rd, n);
        ro = p + h.n * 2e-3;
      } else {
        rd = normalize(refr);
        ro = p - h.n * 2e-3;
        inWater = entering;
      }
      continue;
    }

    // A matte surface: light from the sun (a shadow ray), then a bounce.
    vec3 alb = clamp(h.albedo, 0.0, 1.0);
    vec3 weight = vec3(1.0);
    if (!matte) {
      matte = true;
      M = thr * alb;
      thr = vec3(1.0);
    } else {
      weight = thr * alb;
      thr = weight;
    }
    camera = false;
    vec3 n = h.n;
    vec3 o = p + n * 2e-3;
    vec3 l = sunSample();
    float cl = dot(n, l);
    if (cl > 0.0 && uSunE.g > 0.0) {
      sunO = o;
      sunD = l;
      sunW = weight * uSunE * cl / PI;
      sunWater = inWater;
      wantSun = true;
    }
    if (diffuse >= uBounces) {
      havePath = false;
      continue;
    }
    diffuse++;
    if (diffuse > 2) {
      float q = clamp(max(thr.r, max(thr.g, thr.b)), 0.1, 0.95);
      if (rand() > q) {
        havePath = false;
        continue;
      }
      thr /= q;
    }
    ro = o;
    rd = cosineDir(n);
  }

  float e = matte ? lum(Ls) : 0.0;
  oLm = vec4(M * Ls, e * e);
  oM = vec4(M, e);
  float a = lum(La);
  oLa = vec4(La, a * a);
  oGuide = guide;
}
`;

/** Turns the running averages into light per unit of surface colour and the light before the
 * first surface, each with the variance of its mean, and packs the guides the filter needs. */
export const PREPARE_FS =
  HEADER +
  /* glsl */ `
uniform sampler2D uLm;
uniform sampler2D uM;
uniform sampler2D uLa;
uniform sampler2D uGuide;
uniform float uN;
uniform ivec2 uSize;
layout(location = 0) out vec4 oE;
layout(location = 1) out vec4 oLa;
layout(location = 2) out vec4 oG;

const float ALBEDO_EPS = 0.01;
float lum(vec3 c) { return dot(c, vec3(0.2126, 0.7152, 0.0722)); }
vec2 oct(vec3 n) {
  n /= abs(n.x) + abs(n.y) + abs(n.z);
  vec2 e = n.xz;
  if (n.y < 0.0) e = (1.0 - abs(e.yx)) * vec2(e.x >= 0.0 ? 1.0 : -1.0, e.y >= 0.0 ? 1.0 : -1.0);
  return e;
}
float light(ivec2 p) {
  return lum(texelFetch(uLm, p, 0).rgb / (texelFetch(uM, p, 0).rgb + ALBEDO_EPS));
}
float depth(ivec2 p) { return texelFetch(uGuide, clamp(p, ivec2(0), uSize - 1), 0).w; }

void main() {
  ivec2 p = ivec2(gl_FragCoord.xy);
  vec4 lm = texelFetch(uLm, p, 0);
  vec4 m = texelFetch(uM, p, 0);
  vec4 la = texelFetch(uLa, p, 0);
  vec3 e = lm.rgb / (m.rgb + ALBEDO_EPS);
  // Over the samples so far: the spread of one sample, then of their mean.
  float bessel = uN / max(uN - 1.0, 1.0);
  float vE = max(lm.a - m.a * m.a, 0.0) * bessel / uN;
  float vA = max(la.a - lum(la.rgb) * lum(la.rgb), 0.0) * bessel / uN;
  // With few samples, the spread over similar neighbours says more about the noise.
  vec4 gc = texelFetch(uGuide, p, 0);
  vec3 nc = normalize(gc.xyz + 1e-6);
  vec2 s1 = vec2(0.0), s2 = vec2(0.0);
  float sw = 0.0;
  for (int y = -2; y <= 2; y++) {
    for (int x = -2; x <= 2; x++) {
      ivec2 q = clamp(p + ivec2(x, y), ivec2(0), uSize - 1);
      vec4 g = texelFetch(uGuide, q, 0);
      float w = max(dot(nc, normalize(g.xyz + 1e-6)), 0.0);
      w *= abs(g.w - gc.w) < 0.05 * gc.w + 0.1 ? 1.0 : 0.0;
      vec2 l = vec2(light(q), lum(texelFetch(uLa, q, 0).rgb));
      s1 += w * l;
      s2 += w * l * l;
      sw += w;
    }
  }
  vec2 vS = sw > 0.0 ? max(s2 / sw - (s1 / sw) * (s1 / sw), vec2(0.0)) : vec2(0.0);
  float k = clamp((uN - 1.0) / 8.0, 0.0, 1.0);
  vE = mix(max(vS.x, vE), vE, k);
  vA = mix(max(vS.y, vA), vA, k);
  float grad = max(abs(depth(p + ivec2(1, 0)) - depth(p - ivec2(1, 0))), abs(depth(p + ivec2(0, 1)) - depth(p - ivec2(0, 1)))) * 0.5;
  oE = vec4(e, vE);
  oLa = vec4(la.rgb, vA);
  oG = vec4(oct(nc), gc.w, grad);
}
`;

/** One pass of the edge-avoiding à-trous wavelet filter (as in SVGF): neighbours count when
 * they face the same way, lie at the same depth and differ by no more than the noise. */
export const ATROUS_FS =
  HEADER +
  /* glsl */ `
uniform sampler2D uE;
uniform sampler2D uLa;
uniform sampler2D uG;
uniform int uStep;
uniform ivec2 uSize;
layout(location = 0) out vec4 oE;
layout(location = 1) out vec4 oLa;

float lum(vec3 c) { return dot(c, vec3(0.2126, 0.7152, 0.0722)); }
vec3 unoct(vec2 e) {
  vec3 n = vec3(e.x, 1.0 - abs(e.x) - abs(e.y), e.y);
  if (n.y < 0.0) n.xz = (1.0 - abs(n.zx)) * vec2(n.x >= 0.0 ? 1.0 : -1.0, n.z >= 0.0 ? 1.0 : -1.0);
  return normalize(n);
}

void main() {
  ivec2 p = ivec2(gl_FragCoord.xy);
  vec4 ec = texelFetch(uE, p, 0);
  vec4 ac = texelFetch(uLa, p, 0);
  vec4 gc = texelFetch(uG, p, 0);
  vec3 nc = unoct(gc.xy);
  float le = lum(ec.rgb);
  float la = lum(ac.rgb);
  float sigE = 4.0 * sqrt(ec.a) + 1e-5;
  float sigA = 4.0 * sqrt(ac.a) + 1e-5;
  const float K[3] = float[3](0.375, 0.25, 0.0625);
  vec4 sumE = vec4(0.0);
  vec4 sumA = vec4(0.0);
  vec2 sumW = vec2(0.0);
  for (int y = -2; y <= 2; y++) {
    for (int x = -2; x <= 2; x++) {
      ivec2 q = p + ivec2(x, y) * uStep;
      if (q.x < 0 || q.y < 0 || q.x >= uSize.x || q.y >= uSize.y) continue;
      vec4 eq = texelFetch(uE, q, 0);
      vec4 aq = texelFetch(uLa, q, 0);
      vec4 gq = texelFetch(uG, q, 0);
      float k = K[abs(x)] * K[abs(y)];
      float wn = pow(max(dot(nc, unoct(gq.xy)), 0.0), 64.0);
      float wz = exp(-abs(gc.z - gq.z) / (gc.w * float(uStep) * length(vec2(x, y)) + 0.002 * gc.z + 1e-3));
      float we = k * wn * wz * exp(-abs(le - lum(eq.rgb)) / sigE);
      float wa = k * wn * wz * exp(-abs(la - lum(aq.rgb)) / sigA);
      sumE += vec4(we * eq.rgb, we * we * eq.a);
      sumA += vec4(wa * aq.rgb, wa * wa * aq.a);
      sumW += vec2(we, wa);
    }
  }
  sumW = max(sumW, vec2(1e-6));
  oE = vec4(sumE.rgb / sumW.x, sumE.a / (sumW.x * sumW.x));
  oLa = vec4(sumA.rgb / sumW.y, sumA.a / (sumW.y * sumW.y));
}
`;

/** The picture: light times surface colour plus what came before, exposed and tone mapped. */
export const COMPOSITE_FS =
  HEADER +
  /* glsl */ `
uniform sampler2D uE;
uniform sampler2D uLaF;
uniform sampler2D uLm;
uniform sampler2D uM;
uniform sampler2D uLa;
uniform bool uDenoise;
uniform float uExposure;
out vec4 oColor;

vec3 aces(vec3 x) {
  const float a = 2.51, b = 0.03, c = 2.43, d = 0.59, e = 0.14;
  return clamp((x * (a * x + b)) / (x * (c * x + d) + e), 0.0, 1.0);
}
vec3 toSrgb(vec3 c) {
  return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(0.0031308, c));
}

void main() {
  ivec2 p = ivec2(gl_FragCoord.xy);
  vec3 c;
  if (uDenoise) {
    c = texelFetch(uLaF, p, 0).rgb + (texelFetch(uM, p, 0).rgb + 0.01) * texelFetch(uE, p, 0).rgb;
  } else {
    c = texelFetch(uLa, p, 0).rgb + texelFetch(uLm, p, 0).rgb;
  }
  c = aces(max(c, 0.0) * uExposure);
  // A little dither against banding in the sky.
  float n = fract(sin(dot(vec2(p), vec2(12.9898, 78.233))) * 43758.5453) - 0.5;
  oColor = vec4(toSrgb(c) + n / 255.0, 1.0);
}
`;
