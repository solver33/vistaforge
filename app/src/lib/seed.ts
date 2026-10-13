// Seeds stay below 2^53 so they survive JSON in the browser unchanged.

const MAX_SEED = Number.MAX_SAFE_INTEGER;

export function randomSeed(): number {
  // 48 random bits: plenty of variety, and short enough to read out.
  const hi = Math.floor(Math.random() * 2 ** 24);
  const lo = Math.floor(Math.random() * 2 ** 24);
  return hi * 2 ** 24 + lo;
}

/** A seed typed by the user: a number is used as is, any other text is hashed. */
export function parseSeed(text: string): number {
  const t = text.trim();
  if (/^\d+$/.test(t)) {
    const n = Number(t);
    if (n <= MAX_SEED) return n;
  }
  return hashText(t);
}

/** cyrb53: a fast 53-bit string hash. */
export function hashText(text: string): number {
  let h1 = 0xdeadbeef;
  let h2 = 0x41c6ce57;
  for (let i = 0; i < text.length; i++) {
    const ch = text.charCodeAt(i);
    h1 = Math.imul(h1 ^ ch, 2654435761);
    h2 = Math.imul(h2 ^ ch, 1597334677);
  }
  h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507);
  h1 ^= Math.imul(h2 ^ (h2 >>> 13), 3266489909);
  h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507);
  h2 ^= Math.imul(h1 ^ (h1 >>> 13), 3266489909);
  return 4294967296 * (2097151 & h2) + (h1 >>> 0);
}
