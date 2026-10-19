<script lang="ts">
  import { untrack } from 'svelte';
  import { app } from '../lib/app.svelte';
  import { DEFAULT_SETTINGS, TIME_PRESETS, type RenderSettings, type TimePreset } from '../lib/render/settings';
  import { AREA_SIZES, render } from '../lib/render/state.svelte';
  import { PathTracer, unsupported } from '../lib/render/tracer';
  import Range from './Range.svelte';
  import Section from './Section.svelte';
  import SunDial from './SunDial.svelte';

  let canvas = $state<HTMLCanvasElement | null>(null);
  let minimap = $state<HTMLCanvasElement | null>(null);
  let tracer = $state.raw<PathTracer | null>(null);
  /** Bumped to start a fresh tracer on a new canvas after the graphics card dropped the old one. */
  let generation = $state(0);
  /** Bumped to start a fresh tracer on the same canvas once its context is back. */
  let revive = $state(0);
  let keepContext = false;
  let panel = $state(true);
  const problem = unsupported();
  let rememberTimer: ReturnType<typeof setTimeout> | undefined;

  const SAMPLES = [64, 256, 1024, 4096];
  /** Where the camera looks, for the sun dial. */
  let view = $state<number | null>(null);
  const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

  $effect(() => {
    void generation;
    void revive;
    const c = canvas;
    if (!c || problem) return;
    let t: PathTracer;
    try {
      t = new PathTracer(c, untrack(() => $state.snapshot(render.settings) as RenderSettings), (s) => (render.status = s));
    } catch (e) {
      render.error = errorText(e);
      return;
    }
    tracer = t;
    (window as unknown as { __vistaforgeTracer?: PathTracer }).__vistaforgeTracer = t;
    return () => {
      t.dispose(keepContext);
      keepContext = false;
      tracer = null;
    };
  });

  // After the graphics card was reset, the browser gives the canvas its context back: carry on
  // there (a new canvas may be refused a context after a reset).
  $effect(() => {
    const c = canvas;
    if (!c) return;
    const restored = () => {
      keepContext = true;
      render.status = null;
      revive++;
    };
    c.addEventListener('webglcontextrestored', restored);
    return () => c.removeEventListener('webglcontextrestored', restored);
  });

  $effect(() => {
    const t = tracer;
    if (!t) return;
    const look = () => {
      const d = t.camera.getWorldDirection(t.camera.position.clone());
      if (Math.hypot(d.x, d.z) < 1e-3) return;
      const a = (Math.atan2(d.x, -d.z) * 180) / Math.PI;
      view = (a + 360) % 360;
    };
    look();
    t.controls.addEventListener('change', look);
    return () => t.controls.removeEventListener('change', look);
  });

  $effect(() => {
    const t = tracer;
    const s = render.scene;
    if (!t || !s) return;
    try {
      t.setScene(s);
    } catch (e) {
      render.error = errorText(e);
    }
  });

  $effect(() => {
    const s = $state.snapshot(render.settings) as RenderSettings;
    tracer?.setSettings(s);
    clearTimeout(rememberTimer);
    rememberTimer = setTimeout(() => render.remember(), 400);
  });

  // The area on a small map of the whole world.
  $effect(() => {
    const c = minimap;
    const p = app.preview;
    const a = render.area;
    const world = render.mapSize;
    if (!c) return;
    const ctx = c.getContext('2d');
    if (!ctx) return;
    const w = c.width;
    ctx.fillStyle = '#22272c';
    ctx.fillRect(0, 0, w, w);
    if (p && p.colors.length === p.res * p.res * 4) {
      const img = document.createElement('canvas');
      img.width = p.res;
      img.height = p.res;
      img.getContext('2d')?.putImageData(new ImageData(new Uint8ClampedArray(p.colors), p.res, p.res), 0, 0);
      ctx.imageSmoothingEnabled = true;
      ctx.drawImage(img, 0, 0, w, w);
    }
    const k = w / world;
    ctx.strokeStyle = '#d2a64c';
    ctx.lineWidth = 2;
    ctx.fillStyle = 'rgba(210, 166, 76, 0.18)';
    ctx.fillRect(a.x0 * k, a.z0 * k, a.size * k, a.size * k);
    ctx.strokeRect(a.x0 * k + 1, a.z0 * k + 1, Math.max(a.size * k - 2, 2), Math.max(a.size * k - 2, 2));
  });

  function moveArea(e: MouseEvent) {
    const c = minimap;
    if (!c) return;
    const r = c.getBoundingClientRect();
    const world = render.mapSize;
    render.center = { x: ((e.clientX - r.left) / r.width) * world, z: ((e.clientY - r.top) / r.height) * world };
  }

  function set<K extends keyof RenderSettings>(key: K, value: RenderSettings[K]) {
    render.settings[key] = value;
  }

  function preset(p: TimePreset) {
    render.settings.sunAzimuth = p.azimuth;
    render.settings.sunElevation = p.elevation;
    render.settings.night = p.night;
  }

  const activePreset = $derived(
    TIME_PRESETS.find(
      (p) =>
        p.night === render.settings.night &&
        Math.abs(p.azimuth - render.settings.sunAzimuth) < 0.5 &&
        Math.abs(p.elevation - render.settings.sunElevation) < 0.5,
    )?.name ?? null,
  );

  function seconds(s: number): string {
    if (s < 60) return `${Math.round(s)} s`;
    const m = Math.floor(s / 60);
    return `${m}:${String(Math.round(s - m * 60)).padStart(2, '0')}`;
  }

  const statusText = $derived.by(() => {
    if (render.building) return `Building the scene… ${Math.round(render.progress * 100)}%`;
    const s = render.status;
    if (!render.scene || !s) return '';
    if (s.compiling) return 'Preparing the renderer…';
    if (s.done) return `Finished: ${s.samples} samples in ${seconds(s.seconds)}`;
    return `Rendering: ${s.samples} of ${s.target} samples · ${seconds(s.seconds)}`;
  });

  const source = $derived(render.scene?.header.source ?? null);
  const folderName = $derived(source?.game_dir?.split(/[\\/]/).filter(Boolean).pop() ?? null);
  const sizes = $derived(AREA_SIZES.filter((_, i) => i === 0 || AREA_SIZES[i - 1] < render.mapSize));
  const lostError = $derived(render.status?.error ?? null);
</script>

<div class="render" data-testid="render-view">
  <div class="stage" class:shifted={panel}>
    {#key generation}
      <canvas bind:this={canvas} data-testid="render-canvas"></canvas>
    {/key}

    <div class="bar">
      {#if render.stale && !render.building}
        <span class="changed">The map has changed since this render.</span>
        <button class="btn small primary" onclick={() => render.build()} data-testid="render-again">Render again</button>
      {/if}
      {#if statusText}<span class="status" data-testid="render-status">{statusText}</span>{/if}
      {#if render.scene}
        <button class="btn small" onclick={() => tracer?.resetCamera()} title="Look at the whole area">Reset view</button>
        <button
          class="btn small"
          disabled={!render.status || render.status.samples === 0}
          onclick={() => tracer && render.saveImage(() => tracer!.picture())}>Save image…</button
        >
      {/if}
      <button class="btn small" onclick={() => (panel = !panel)} aria-pressed={panel}>Settings</button>
    </div>

    {#if render.scene}
      <div class="help">Drag to turn · right-drag to move · wheel to zoom</div>
    {/if}

    {#if problem}
      <div class="overlay"><div class="card">{problem} The preview still works.</div></div>
    {:else if lostError}
      <div class="overlay">
        <div class="card">
          <p class="error-text">{lostError}</p>
          <button class="btn primary" onclick={() => generation++}>Start again</button>
        </div>
      </div>
    {:else if !render.scene}
      <div class="overlay">
        <div class="card">
          <p>
            Render mode draws the map the way the game does: block textures from your modpack, sunlight with
            soft shadows, sky, clouds, fog and clear water. The picture starts grainy and clears up as it renders.
          </p>
          <p class="muted">Pick the area under Area, then press Render.</p>
          <button class="btn primary" disabled={render.building || !app.catalog} onclick={() => render.build()} data-testid="render-build">
            {render.building ? 'Building…' : 'Render'}
          </button>
          {#if render.error}<p class="error-text">{render.error}</p>{/if}
        </div>
      </div>
    {/if}
  </div>

  {#if panel}
    <aside class="panel" data-testid="render-panel">
      <Section title="Area">
        <button class="minimap" onclick={moveArea} title="Click to move the area">
          <canvas bind:this={minimap} width="200" height="200"></canvas>
        </button>
        <div class="row">
          <span class="segmented" title="Side of the area, in blocks">
            {#each sizes as s (s)}
              <button class:on={render.area.size === s} onclick={() => ((render.size = s), render.remember())}>{s}</button>
            {/each}
          </span>
        </div>
        <button class="btn primary wide" disabled={render.building || !app.catalog} onclick={() => render.build()}>
          {render.building ? `Building… ${Math.round(render.progress * 100)}%` : render.scene ? 'Render again' : 'Render'}
        </button>
        {#if render.error && render.scene}<p class="error-text small">{render.error}</p>{/if}
      </Section>

      <Section title="Time of day">
        <div class="presets">
          {#each TIME_PRESETS as p (p.name)}
            <button class:on={activePreset === p.name} onclick={() => preset(p)} data-time={p.name}>{p.name}</button>
          {/each}
        </div>
        <SunDial
          azimuth={render.settings.sunAzimuth}
          elevation={render.settings.sunElevation}
          night={render.settings.night}
          {view}
          onchange={(az, el) => {
            render.settings.sunAzimuth = az;
            render.settings.sunElevation = el;
          }}
        />
        <Range
          label={render.settings.night ? 'Moon size' : 'Sun size'}
          help="A bigger sun gives softer shadow edges."
          value={render.settings.sunSize}
          min={1}
          max={20}
          step={0.5}
          normal={DEFAULT_SETTINGS.sunSize}
          format={(v) => `×${v}`}
          oninput={(v) => set('sunSize', v)}
        />
        <Range
          label={render.settings.night ? 'Moonlight' : 'Sunlight'}
          value={render.settings.sunBrightness}
          min={0}
          max={3}
          step={0.05}
          normal={DEFAULT_SETTINGS.sunBrightness}
          format={(v) => `${Math.round(v * 100)}%`}
          oninput={(v) => set('sunBrightness', v)}
        />
      </Section>

      <Section title="Sky and clouds">
        <Range
          label="Sky light"
          help="How much light comes from the sky itself: it fills the shadows."
          value={render.settings.skyBrightness}
          min={0}
          max={3}
          step={0.05}
          normal={DEFAULT_SETTINGS.skyBrightness}
          format={(v) => `${Math.round(v * 100)}%`}
          oninput={(v) => set('skyBrightness', v)}
        />
        <div class="field">
          <span>Clouds</span>
          <span class="segmented" title="Real-looking soft clouds, or Minecraft's flat blocky ones">
            <button class:on={render.settings.cloudStyle === 'soft'} onclick={() => set('cloudStyle', 'soft')}>Soft</button>
            <button class:on={render.settings.cloudStyle === 'blocky'} onclick={() => set('cloudStyle', 'blocky')}>Blocky</button>
          </span>
        </div>
        <Range
          label="Cover"
          help="How much of the sky is cloud. Clouds cast shadows, and light shines through their gaps in fog."
          value={render.settings.clouds}
          min={0}
          max={1}
          step={0.01}
          normal={DEFAULT_SETTINGS.clouds}
          format={(v) => (v === 0 ? 'None' : `${Math.round(v * 100)}%`)}
          oninput={(v) => set('clouds', v)}
        />
        <Range
          label="Cloud height"
          value={render.settings.cloudHeight}
          min={80}
          max={400}
          step={1}
          normal={DEFAULT_SETTINGS.cloudHeight}
          format={(v) => `Y ${Math.round(v)}`}
          oninput={(v) => set('cloudHeight', v)}
        />
        {#if render.settings.cloudStyle === 'soft'}
          <Range
            label="Thickness"
            help="From the clouds' flat undersides to their tops."
            value={render.settings.cloudThickness}
            min={10}
            max={200}
            step={1}
            normal={DEFAULT_SETTINGS.cloudThickness}
            format={(v) => `${Math.round(v)} blocks`}
            oninput={(v) => set('cloudThickness', v)}
          />
          <Range
            label="Size"
            help="How big the clouds are across."
            value={render.settings.cloudSize}
            min={0.3}
            max={3}
            step={0.05}
            normal={DEFAULT_SETTINGS.cloudSize}
            format={(v) => `×${v.toFixed(2)}`}
            oninput={(v) => set('cloudSize', v)}
          />
        {/if}
      </Section>

      <Section title="Fog">
        <Range
          label="Amount"
          help="Mist that thickens in the distance and in the valleys, lit by the sun."
          value={render.settings.fog}
          min={0}
          max={1}
          step={0.01}
          normal={DEFAULT_SETTINGS.fog}
          format={(v) => (v === 0 ? 'None' : `${Math.round(v * 100)}%`)}
          oninput={(v) => set('fog', v)}
        />
        <Range
          label="Level"
          help="Below this height the fog is thickest."
          value={render.settings.fogLevel}
          min={-64}
          max={320}
          step={1}
          normal={DEFAULT_SETTINGS.fogLevel}
          format={(v) => `Y ${Math.round(v)}`}
          oninput={(v) => set('fogLevel', v)}
        />
        <Range
          label="Fade"
          help="Over how many blocks above the level the fog thins out."
          value={render.settings.fogFade}
          min={1}
          max={160}
          step={1}
          normal={DEFAULT_SETTINGS.fogFade}
          format={(v) => `${Math.round(v)} blocks`}
          oninput={(v) => set('fogFade', v)}
        />
      </Section>

      <Section title="Water">
        <Range
          label="Clearness"
          help="How deep you can see into the water, in blocks."
          value={render.settings.waterClarity}
          min={1}
          max={40}
          step={0.5}
          normal={DEFAULT_SETTINGS.waterClarity}
          format={(v) => `${v} blocks`}
          oninput={(v) => set('waterClarity', v)}
        />
        <Range
          label="Waves"
          value={render.settings.waves}
          min={0}
          max={1.5}
          step={0.01}
          normal={DEFAULT_SETTINGS.waves}
          format={(v) => (v === 0 ? 'Still' : `${Math.round(v * 100)}%`)}
          oninput={(v) => set('waves', v)}
        />
      </Section>

      <Section title="Camera">
        <Range
          label="Field of view"
          value={render.settings.fov}
          min={15}
          max={100}
          step={1}
          normal={DEFAULT_SETTINGS.fov}
          format={(v) => `${Math.round(v)}°`}
          oninput={(v) => set('fov', v)}
        />
        <Range
          label="Depth of field"
          help="Blurs what is nearer or farther than the point you turn around."
          value={render.settings.aperture}
          min={0}
          max={1}
          step={0.01}
          normal={DEFAULT_SETTINGS.aperture}
          format={(v) => (v === 0 ? 'Off' : `${Math.round(v * 100)}%`)}
          oninput={(v) => set('aperture', v)}
        />
        <Range
          label="Glow"
          help="Bright light spreading around itself, as through a real lens: a halo round the sun, glinting water."
          value={render.settings.glow}
          min={0}
          max={1}
          step={0.01}
          normal={DEFAULT_SETTINGS.glow}
          format={(v) => (v === 0 ? 'Off' : `${Math.round(v * 100)}%`)}
          oninput={(v) => set('glow', v)}
        />
        <Range
          label="Vignette"
          help="Darker corners, as in an old lens."
          value={render.settings.vignette}
          min={0}
          max={1}
          step={0.01}
          normal={DEFAULT_SETTINGS.vignette}
          format={(v) => (v === 0 ? 'Off' : `${Math.round(v * 100)}%`)}
          oninput={(v) => set('vignette', v)}
        />
        <Range
          label="Exposure"
          help="Brighter or darker picture, in camera stops."
          value={render.settings.exposure}
          min={-3}
          max={3}
          step={0.1}
          normal={DEFAULT_SETTINGS.exposure}
          format={(v) => `${v > 0 ? '+' : ''}${v.toFixed(1)}`}
          oninput={(v) => set('exposure', v)}
        />
      </Section>

      <Section title="Quality">
        <div class="field">
          <span>Samples</span>
          <span class="segmented" title="More samples: a cleaner picture that takes longer">
            {#each SAMPLES as n (n)}
              <button class:on={render.settings.samples === n} onclick={() => set('samples', n)}>{n}</button>
            {/each}
          </span>
        </div>
        <Range
          label="Light bounces"
          help="More bounces light the shadows and indoors more truly, but render slower."
          value={render.settings.bounces}
          min={1}
          max={6}
          step={1}
          normal={DEFAULT_SETTINGS.bounces}
          format={(v) => `${v}`}
          oninput={(v) => set('bounces', v)}
        />
        <Range
          label="Resolution"
          help="Share of the screen's pixels rendered. Lower is faster."
          value={render.settings.scale}
          min={0.25}
          max={1}
          step={0.05}
          normal={DEFAULT_SETTINGS.scale}
          format={(v) => `${Math.round(v * 100)}%`}
          oninput={(v) => set('scale', v)}
        />
        <label class="check"
          ><input type="checkbox" checked={render.settings.denoise} onchange={(e) => set('denoise', e.currentTarget.checked)} />
          Remove noise</label
        >
      </Section>

      <Section title="Textures">
        {#if source}
          {#if folderName}
            <p class="small">
              From <span class="mono">{folderName}</span>: {source.mods} mods{source.packs
                ? `, ${source.packs} resource packs`
                : ''}{source.minecraft_jar ? ' and Minecraft itself' : ''}.
              {source.models} kinds of blocks here use their own models{source.fallback
                ? `; ${source.fallback} without them are drawn as plain blocks in their catalog colours`
                : ''}.
            </p>
            {#if !source.minecraft_jar}
              <p class="small muted">Minecraft’s own files were not found, so vanilla blocks may look plain.</p>
            {/if}
            {#if source.missing_textures > 0}
              <p class="small muted" title={source.missing.join('\n')}>{source.missing_textures} textures were missing.</p>
            {/if}
          {:else}
            <p class="small">The game files were not found, so blocks are shown in their catalog colours.</p>
          {/if}
        {:else}
          <p class="small muted">Textures are read from your game folder when you render; nothing is copied.</p>
        {/if}
        <button class="btn small" onclick={() => render.pickGameDir()}>Choose game folder…</button>
        {#if render.gameDir}<p class="small mono path" title={render.gameDir}>{render.gameDir}</p>{/if}
      </Section>
    </aside>
  {/if}
</div>

<style>
  .render {
    position: absolute;
    inset: 0;
    background: #a9c6dc;
  }

  .stage {
    position: absolute;
    inset: 0;
    overflow: hidden;
  }

  canvas {
    display: block;
    width: 100%;
    height: 100%;
    outline: none;
  }

  /* Along the bottom, left of the settings panel when it is open. */
  .bar {
    position: absolute;
    bottom: 10px;
    right: 10px;
    max-width: calc(100% - 20px);
    flex-wrap: wrap;
    justify-content: flex-end;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 8px;
    border-radius: var(--radius);
    background: rgba(18, 21, 24, 0.82);
    border: 1px solid var(--line);
  }

  .status {
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  .help {
    position: absolute;
    left: 10px;
    top: 48px;
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 11px;
    color: var(--muted);
    background: rgba(18, 21, 24, 0.7);
    pointer-events: none;
  }

  .overlay {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 50px 16px;
    background: rgba(18, 21, 24, 0.55);
  }

  .shifted .overlay {
    right: 260px;
  }

  .shifted .bar {
    right: 270px;
    max-width: calc(100% - 280px);
  }

  .card {
    max-width: 380px;
    padding: 16px 18px;
    border-radius: 10px;
    background: var(--panel);
    border: 1px solid var(--line);
    text-align: center;
  }

  .card p {
    margin: 0 0 12px;
  }

  .changed {
    color: var(--accent);
    font-size: 12px;
  }

  .panel {
    position: absolute;
    top: 10px;
    right: 10px;
    bottom: 10px;
    width: 250px;
    overflow-y: auto;
    border-radius: var(--radius);
    background: rgba(26, 30, 34, 0.94);
    border: 1px solid var(--line);
  }

  .minimap {
    display: block;
    width: 100%;
    padding: 0;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: none;
    overflow: hidden;
    cursor: crosshair;
  }

  .minimap canvas {
    width: 100%;
    height: auto;
    aspect-ratio: 1;
  }

  .row,
  .field {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin: 8px 0;
  }

  .wide {
    width: 100%;
    justify-content: center;
  }

  .presets {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin: 4px 0 6px;
  }

  .presets button {
    flex: 1 0 auto;
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel-2);
    color: var(--muted);
    font-size: 12px;
  }

  .presets button:hover {
    color: var(--text);
  }

  .presets button.on {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
  }

  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 6px;
  }

  .small {
    font-size: 12px;
    margin: 6px 0;
  }

  .muted {
    color: var(--muted);
  }

  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
