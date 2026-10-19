<script lang="ts">
  // The sky seen from above, north up: drag the sun (or the moon at night) around it. The
  // middle is straight overhead, the ring is the horizon, and the dark band outside it is just
  // below the horizon, for dusk.

  import { MIN_ELEVATION } from '../lib/render/settings';

  let {
    azimuth,
    elevation,
    night,
    view = null,
    onchange,
  }: {
    azimuth: number;
    elevation: number;
    night: boolean;
    /** Where the camera looks, degrees clockwise from north. */
    view?: number | null;
    onchange: (azimuth: number, elevation: number) => void;
  } = $props();

  /** The horizon's radius, and the band below it. */
  const R = 80;
  const BAND = R * 0.13;
  const D = Math.PI / 180;

  let svg = $state<SVGSVGElement | null>(null);
  let dragging = $state(false);

  function radius(el: number): number {
    return el >= 0 ? R * (1 - el / 90) : R + (Math.min(-el, -MIN_ELEVATION) / -MIN_ELEVATION) * BAND;
  }

  const pos = $derived.by(() => {
    const r = radius(elevation);
    return { x: Math.sin(azimuth * D) * r, y: -Math.cos(azimuth * D) * r };
  });

  /** A point of the dial, from the middle, to the light's place. */
  function place(x: number, y: number): void {
    const r = Math.hypot(x, y);
    let az = Math.atan2(x, -y) / D;
    if (az < 0) az += 360;
    const el = r <= R ? 90 * (1 - r / R) : Math.max(MIN_ELEVATION, (-(r - R) / BAND) * -MIN_ELEVATION);
    onchange(Math.round(az * 2) / 2, Math.round(el * 2) / 2);
  }

  function point(e: PointerEvent): [number, number] | null {
    if (!svg) return null;
    const box = svg.getBoundingClientRect();
    const k = 200 / box.width;
    return [(e.clientX - box.left) * k - 100, (e.clientY - box.top) * k - 100];
  }

  function down(e: PointerEvent) {
    const p = point(e);
    if (!p) return;
    dragging = true;
    svg?.setPointerCapture(e.pointerId);
    place(...p);
  }

  function move(e: PointerEvent) {
    if (!dragging) return;
    const p = point(e);
    if (p) place(...p);
  }

  function up(e: PointerEvent) {
    dragging = false;
    svg?.releasePointerCapture(e.pointerId);
  }

  function key(e: KeyboardEvent) {
    const steps: Record<string, [number, number]> = {
      ArrowLeft: [-5, 0],
      ArrowRight: [5, 0],
      ArrowUp: [0, 2],
      ArrowDown: [0, -2],
    };
    const s = steps[e.key];
    if (!s) return;
    e.preventDefault();
    onchange((azimuth + s[0] + 360) % 360, Math.min(90, Math.max(MIN_ELEVATION, elevation + s[1])));
  }

  const COMPASS = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW'];
  const where = $derived(COMPASS[Math.round(azimuth / 45) % 8]);
  const label = $derived(
    elevation < 0
      ? `${night ? 'Moon' : 'Sun'} below the horizon in the ${where}`
      : elevation >= 85
        ? `${night ? 'Moon' : 'Sun'} straight overhead`
        : `${night ? 'Moon' : 'Sun'} in the ${where}, ${Math.round(elevation)}° up`,
  );

  // The camera's field of view as a wedge from the middle.
  const wedge = $derived.by(() => {
    if (view === null) return null;
    const a = (view - 22) * D;
    const b = (view + 22) * D;
    const r = R * 0.55;
    return `M0 0 L${Math.sin(a) * r} ${-Math.cos(a) * r} A${r} ${r} 0 0 1 ${Math.sin(b) * r} ${-Math.cos(b) * r} Z`;
  });
</script>

<div class="dial" class:night>
  <svg
    bind:this={svg}
    viewBox="-100 -100 200 200"
    role="slider"
    tabindex="0"
    aria-label="Where the {night ? 'moon' : 'sun'} is: drag it around the sky"
    aria-valuetext={label}
    aria-valuenow={Math.round(elevation)}
    data-testid="sun-dial"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    onkeydown={key}
  >
    <defs>
      <radialGradient id="dial-sky">
        <stop offset="0%" class="sky-top" />
        <stop offset="100%" class="sky-edge" />
      </radialGradient>
      <radialGradient id="dial-glow">
        <stop offset="0%" class="glow-in" />
        <stop offset="100%" class="glow-out" />
      </radialGradient>
    </defs>
    <circle r={R + BAND} class="below" />
    <circle r={R} fill="url(#dial-sky)" class="sky" />
    <circle r={R * (2 / 3)} class="ring" />
    <circle r={R / 3} class="ring" />
    {#if wedge}<path d={wedge} class="view" />{/if}
    <text x="0" y={-R + 11} class="cardinal">N</text>
    <text x={R - 9} y="4" class="cardinal">E</text>
    <text x="0" y={R - 5} class="cardinal">S</text>
    <text x={-R + 9} y="4" class="cardinal">W</text>
    <circle cx={pos.x} cy={pos.y} r="16" fill="url(#dial-glow)" class="halo" />
    <circle cx={pos.x} cy={pos.y} r="7.5" class="light" />
  </svg>
  <div class="caption">{label}</div>
</div>

<style>
  .dial {
    margin: 4px 0 8px;
  }

  svg {
    display: block;
    width: 100%;
    max-width: 210px;
    margin: 0 auto;
    aspect-ratio: 1;
    touch-action: none;
    cursor: grab;
    border-radius: 50%;
  }

  svg:active {
    cursor: grabbing;
  }

  svg:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .below {
    fill: #1d2430;
    stroke: var(--line);
  }

  .sky-top {
    stop-color: #8ec3ec;
  }

  .sky-edge {
    stop-color: #dbe7ee;
  }

  .night .sky-top {
    stop-color: #0e1a33;
  }

  .night .sky-edge {
    stop-color: #23324f;
  }

  .sky {
    stroke: rgba(255, 255, 255, 0.35);
  }

  .ring {
    fill: none;
    stroke: rgba(30, 50, 70, 0.28);
    stroke-dasharray: 3 3;
  }

  .night .ring {
    stroke: rgba(200, 215, 255, 0.18);
  }

  .view {
    fill: rgba(20, 30, 40, 0.16);
  }

  .night .view {
    fill: rgba(200, 215, 255, 0.1);
  }

  .cardinal {
    font-size: 11px;
    font-weight: 600;
    text-anchor: middle;
    fill: rgba(20, 35, 50, 0.6);
    pointer-events: none;
  }

  .night .cardinal {
    fill: rgba(210, 220, 255, 0.55);
  }

  .glow-in {
    stop-color: #ffe9a8;
    stop-opacity: 0.9;
  }

  .glow-out {
    stop-color: #ffd36b;
    stop-opacity: 0;
  }

  .night .glow-in {
    stop-color: #dfe8ff;
    stop-opacity: 0.6;
  }

  .night .glow-out {
    stop-color: #b8c8ff;
    stop-opacity: 0;
  }

  .light {
    fill: #fff3c4;
    stroke: #e3a93c;
    stroke-width: 1.5;
  }

  .night .light {
    fill: #e6ebf5;
    stroke: #9aa8c2;
  }

  .halo {
    pointer-events: none;
  }

  .caption {
    text-align: center;
    color: var(--muted);
    font-size: 12px;
    margin-top: 4px;
  }
</style>
