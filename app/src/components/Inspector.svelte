<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { isCircle, isLine, kindInfo } from '../lib/edits';
  import type { ShapeEdit } from '../lib/types';
  import DragNumber from './DragNumber.svelte';

  let { index }: { index: number } = $props();

  const edit = $derived(app.settings.edits[index] as ShapeEdit | undefined);
  const biomes = $derived(app.info?.biomes.filter((b) => !b.feature) ?? []);

  /**
   * The numeric fields each landform has, beyond its position: key, label, range, and whether the
   * field steps in ratio (sizes) rather than in blocks. Heights go higher in tall worlds.
   */
  type Field = [key: string, label: string, min: number, max: number, log?: boolean];
  const FIELDS: Record<string, Field[]> = $derived({
    hill: [['radius', 'Radius', 4, 2048, true], ['height', 'Height', -120, app.limits.hill]],
    mountain: [['radius', 'Radius', 8, 2048, true], ['height', 'Height', 10, app.limits.mountain]],
    plateau: [['radius', 'Radius', 4, 2048, true], ['level', 'Level (Y)', -60, app.limits.plateau]],
    lake: [['radius', 'Radius', 4, 2048, true], ['depth', 'Depth', 1, 60]],
    river: [['width', 'Width', 2, 200, true], ['depth', 'Depth', 1, 30]],
    ridge: [['width', 'Width', 8, 1024, true], ['height', 'Height', 10, app.limits.ridge]],
    biome_zone: [['radius', 'Radius', 8, 2048, true]],
  });

  function set(key: string, value: number, min: number, max: number) {
    if (!Number.isFinite(value) || !edit) return;
    (edit as unknown as Record<string, number>)[key] = Math.max(min, Math.min(max, Math.round(value)));
  }

  function num(key: string): number {
    return (edit as unknown as Record<string, number>)[key];
  }
</script>

{#if edit}
  {@const k = kindInfo(edit.kind)}
  <div class="inspector" data-testid="inspector">
    <div class="head">
      <span class="dot" style:background={k.color}></span>
      <strong>{k.label}</strong>
      <span class="spacer"></span>
      <button class="btn small ghost" data-testid="delete-edit" onclick={() => app.removeEdit(index)}>Delete</button>
    </div>
    <div class="fields">
      {#if isCircle(edit)}
        {@const size = app.settings.size}
        <DragNumber label="X" field="x" value={Math.round(edit.x)} min={0} max={size} oninput={(v) => set('x', v, 0, size)} onchange={() => app.commit()} />
        <DragNumber label="Z" field="z" value={Math.round(edit.z)} min={0} max={size} oninput={(v) => set('z', v, 0, size)} onchange={() => app.commit()} />
      {/if}
      {#each FIELDS[edit.kind] ?? [] as [key, label, min, max, log] (key)}
        <DragNumber
          {label}
          field={key}
          {min}
          {max}
          {log}
          value={num(key)}
          oninput={(v) => set(key, v, min, max)}
          onchange={() => app.commit()}
        />
      {/each}
      {#if edit.kind === 'biome_zone'}
        <label class="wide">
          Biome
          <select
            value={edit.biome}
            onchange={(e) => {
              if (edit.kind === 'biome_zone') edit.biome = e.currentTarget.value;
              app.commit();
            }}
          >
            {#each biomes as b (b.id)}
              <option value={b.id}>{b.name}</option>
            {/each}
          </select>
        </label>
      {/if}
    </div>
    {#if isLine(edit)}
      <div class="faint note">
        {edit.points.length} points. Drag a point to move it; Alt+click a point to remove it.
        {edit.kind === 'river' ? 'The river flows from the first point to the last.' : ''}
      </div>
    {/if}
  </div>
{/if}

<style>
  .inspector {
    width: 270px;
    padding: 10px 12px;
    border-radius: 10px;
    background: rgba(26, 30, 34, 0.94);
    border: 1px solid var(--line);
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.35);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
  }

  .spacer {
    flex: 1;
  }

  .fields {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px 8px;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 12px;
    color: var(--muted);
  }

  label.wide {
    grid-column: 1 / -1;
  }

  select {
    width: 100%;
    color: var(--text);
  }

  .note {
    margin-top: 8px;
    font-size: 11px;
    line-height: 1.45;
  }
</style>
