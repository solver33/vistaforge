<script lang="ts">
  import { tick } from 'svelte';

  let {
    label,
    value,
    min,
    max,
    log = false,
    field,
    oninput,
    onchange,
  }: {
    label: string;
    value: number;
    min: number;
    max: number;
    /** Even steps in ratio rather than in blocks, for sizes that span from a few blocks to thousands. */
    log?: boolean;
    field?: string;
    oninput: (v: number) => void;
    onchange?: () => void;
  } = $props();

  let box: HTMLDivElement;
  let entry: HTMLInputElement | undefined = $state();
  let editing = $state(false);
  let text = $state('');
  let dragging = $state(false);
  /** The pointer being dragged; `t` is the unrounded position along the range, 0 to 1. */
  let drag: { id: number; x: number; t: number; moved: boolean } | null = null;

  const ratio = $derived(log && min > 0);
  // Pixels the pointer travels to sweep the whole range. Short ranges get a few pixels per step.
  const span = $derived(ratio ? 400 : Math.max(200, Math.min(400, (max - min) * 4)));
  const fill = $derived(`${toT(value) * 100}%`);

  const clamp01 = (t: number) => Math.max(0, Math.min(1, t));

  function toT(v: number): number {
    if (max <= min) return 0;
    return clamp01(ratio ? Math.log(v / min) / Math.log(max / min) : (v - min) / (max - min));
  }

  function fromT(t: number): number {
    return ratio ? min * Math.pow(max / min, clamp01(t)) : min + clamp01(t) * (max - min);
  }

  function emit(v: number) {
    const r = Math.max(min, Math.min(max, Math.round(v)));
    if (r !== value) oninput(r);
  }

  function down(e: PointerEvent) {
    if (e.button !== 0 || editing) return;
    e.preventDefault();
    box.focus();
    box.setPointerCapture(e.pointerId);
    drag = { id: e.pointerId, x: e.clientX, t: toT(value), moved: false };
  }

  function move(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    const dx = e.clientX - drag.x;
    if (!drag.moved && Math.abs(dx) < 3) return;
    drag.moved = true;
    dragging = true;
    drag.t = clamp01(drag.t + (dx / span) * (e.shiftKey ? 0.1 : 1));
    drag.x = e.clientX;
    emit(fromT(drag.t));
  }

  function up(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    const moved = drag.moved;
    drag = null;
    dragging = false;
    if (moved) onchange?.();
    else if (e.type === 'pointerup') void startEdit();
  }

  async function startEdit() {
    text = String(value);
    editing = true;
    await tick();
    entry?.focus();
    entry?.select();
  }

  /** Leaves typing; `refocus` after Enter or Escape, not when the user clicked elsewhere. */
  function finishEdit(apply: boolean, refocus = false) {
    if (!editing) return;
    editing = false;
    const v = Number(text.trim().replace(',', '.'));
    if (apply && text.trim() !== '' && Number.isFinite(v)) {
      emit(v);
      onchange?.();
    }
    if (refocus) box.focus();
  }

  function key(e: KeyboardEvent) {
    if (editing || e.ctrlKey || e.metaKey || e.altKey) return;
    // Keep the map's shortcuts (Delete removes the landform) from acting on a key meant for the field.
    if (e.key === 'Delete' || e.key === 'Backspace') {
      e.stopPropagation();
      return;
    }
    const step = e.shiftKey ? 10 : 1;
    let next: number | null = null;
    if (e.key === 'ArrowRight' || e.key === 'ArrowUp') next = value + step;
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') next = value - step;
    else if (e.key === 'Home') next = min;
    else if (e.key === 'End') next = max;
    else if (e.key === 'Enter' || e.key === 'F2') {
      e.preventDefault();
      e.stopPropagation();
      void startEdit();
      return;
    }
    if (next === null) return;
    e.preventDefault();
    e.stopPropagation();
    emit(next);
    onchange?.();
  }
</script>

<div class="field">
  <span class="label">{label}</span>
  <div
    bind:this={box}
    class="drag"
    class:dragging
    class:editing
    role="slider"
    tabindex="0"
    aria-label={label}
    aria-valuemin={min}
    aria-valuemax={max}
    aria-valuenow={value}
    data-field={field}
    title="Drag left or right to change it; hold Shift for fine steps. Click to type a number."
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    onkeydown={key}
  >
    <span class="fill" style:width={fill}></span>
    {#if editing}
      <input
        bind:this={entry}
        bind:value={text}
        type="text"
        inputmode="numeric"
        aria-label={label}
        onkeydown={(e) => {
          if (e.key === 'Enter') finishEdit(true, true);
          else if (e.key === 'Escape') {
            e.stopPropagation();
            finishEdit(false, true);
          }
        }}
        onblur={() => finishEdit(true)}
      />
    {:else}
      <span class="arrow">‹</span>
      <span class="value mono">{value}</span>
      <span class="arrow">›</span>
    {/if}
  </div>
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .label {
    font-size: 12px;
    color: var(--muted);
  }

  .drag {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 28px;
    padding: 0 7px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--bg);
    overflow: hidden;
    cursor: ew-resize;
    touch-action: none;
  }

  .drag:hover {
    border-color: #3d454e;
  }

  .drag:focus-visible,
  .drag.dragging,
  .drag.editing {
    outline: none;
    border-color: var(--accent);
  }

  .drag.editing {
    cursor: text;
  }

  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    background: var(--accent-soft);
    pointer-events: none;
  }

  .value,
  .arrow {
    position: relative;
  }

  .value {
    color: var(--text);
  }

  .arrow {
    color: var(--faint);
    font-size: 14px;
    line-height: 1;
  }

  .drag:hover .arrow,
  .drag.dragging .arrow {
    color: var(--muted);
  }

  .drag input {
    position: relative;
    width: 100%;
    height: 100%;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--text);
    text-align: center;
    font-family: var(--mono);
    outline: none;
  }
</style>
