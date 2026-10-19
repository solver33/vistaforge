<script lang="ts">
  import { formatInt, shortPath } from '../lib/format';
  import { DIMENSION_NAMES, WORLD_AREAS, world } from '../lib/world.svelte';
  import Section from './Section.svelte';

  const info = $derived(world.info);
  const dimensions = $derived(
    info ? [...new Set([info.dimension, ...info.dimensions])].sort((a, b) => order(a) - order(b)) : [],
  );
  const explored = $derived(info?.bounds ? [info.bounds[2] - info.bounds[0], info.bounds[3] - info.bounds[1]] : null);
  const center = $derived({ x: world.area.x0 + world.area.size / 2, z: world.area.z0 + world.area.size / 2 });
  /** Saved chunks the map leaves out, and why. */
  const notes = $derived.by(() => {
    const m = world.map;
    const out: string[] = [];
    if (!m) return out;
    if (m.old_chunks > 0) {
      out.push(
        `${formatInt(m.old_chunks)} chunks were saved by a Minecraft older than 1.18 and are left out. Visiting them in the game updates them.`,
      );
    }
    if (m.unfinished > 0) {
      out.push(`${formatInt(m.unfinished)} chunks at the edge of the explored land were never finished by the game and are left out.`);
    }
    if (m.failed > 0) out.push(`${formatInt(m.failed)} chunks could not be read.`);
    return out;
  });

  function order(d: string): number {
    const i = Object.keys(DIMENSION_NAMES).indexOf(d);
    return i < 0 ? 99 : i;
  }

  function setCenter(axis: 'x' | 'z', text: string) {
    const v = Number(text);
    if (!Number.isFinite(v)) return;
    world.centerOn(axis === 'x' ? v : center.x, axis === 'z' ? v : center.z);
  }
</script>

{#if info}
  <Section title="Minecraft world">
    <div class="head" data-testid="opened-world">
      <div class="name" title={info.name}>{info.name}</div>
      <div class="path mono" title={info.path}>{shortPath(info.path)}</div>
      <div class="faint facts">
        {info.version ? `Minecraft ${info.version} · ` : ''}{formatInt(info.chunks)} saved chunks{explored
          ? ` · ${formatInt(explored[0])} × ${formatInt(explored[1])} blocks explored`
          : ''}
      </div>
    </div>

    {#if dimensions.length > 1}
      <label class="field">
        <span class="label">Dimension</span>
        <select
          data-testid="dimension"
          value={info.dimension}
          disabled={world.opening}
          onchange={(e) => void world.setDimension(e.currentTarget.value)}
        >
          {#each dimensions as d (d)}
            <option value={d}>{DIMENSION_NAMES[d] ?? d}</option>
          {/each}
        </select>
      </label>
    {/if}

    <div class="field">
      <span class="label">Area shown in 3D, centred on</span>
      <div class="three">
        <label class="coord"
          ><span>X</span><input
            type="number"
            step="16"
            data-testid="area-x"
            value={center.x}
            onchange={(e) => setCenter('x', e.currentTarget.value)}
          /></label
        >
        <label class="coord"
          ><span>Z</span><input
            type="number"
            step="16"
            data-testid="area-z"
            value={center.z}
            onchange={(e) => setCenter('z', e.currentTarget.value)}
          /></label
        >
        <select
          data-testid="area-size"
          title="Side of the square, in blocks"
          value={world.area.size}
          onchange={(e) => world.setSize(Number(e.currentTarget.value))}
        >
          {#each WORLD_AREAS as s (s)}
            <option value={s}>{s}</option>
          {/each}
        </select>
      </div>
    </div>
    <div class="buttons">
      <button class="btn small" disabled={!info.player} onclick={() => info.player && world.centerOn(info.player[0], info.player[2])}
        >Go to player</button
      >
      <button class="btn small" onclick={() => world.centerOn(info.spawn[0], info.spawn[2])}>Go to spawn</button>
    </div>

    <p class="hint muted">
      Drag the square on the map to choose which part of the world the 3D view and render mode show. Minecraft worlds
      have no edge, so only that square is read in full. The world is only read: Vistaforge never changes it.
    </p>
    {#each notes as n (n)}
      <p class="note faint">{n}</p>
    {/each}

    <button class="btn wide" data-testid="close-world-panel" onclick={() => world.close()}>Close world</button>
  </Section>
{/if}

<style>
  .head {
    margin-bottom: 10px;
    min-width: 0;
  }

  .name {
    font-size: 14px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .path {
    margin-top: 2px;
    color: var(--faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .facts {
    margin-top: 4px;
    font-size: 12px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin-bottom: 8px;
    min-width: 0;
  }

  .label {
    color: var(--muted);
    font-size: 12px;
  }

  .three {
    display: grid;
    grid-template-columns: 1fr 1fr 1.1fr;
    gap: 6px;
  }

  .coord {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }

  .coord span {
    color: var(--muted);
    font-size: 12px;
  }

  .coord input {
    flex: 1;
    width: 0;
  }

  .buttons {
    display: flex;
    gap: 6px;
  }

  .hint {
    margin: 12px 0 8px;
    line-height: 1.5;
    font-size: 12px;
  }

  .note {
    margin: 0 0 8px;
    line-height: 1.45;
    font-size: 12px;
  }

  .wide {
    width: 100%;
    margin-top: 4px;
  }
</style>
