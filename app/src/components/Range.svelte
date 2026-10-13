<script lang="ts">
  let {
    label,
    help = '',
    value,
    normal = null,
    min = 0,
    max = 1,
    step = 0.01,
    format = (v: number) => `${Math.round(v * 100)}`,
    oninput,
    onchange,
    testid,
  }: {
    label: string;
    help?: string;
    value: number;
    /** A tick at the preset's value; double-click returns to it. */
    normal?: number | null;
    min?: number;
    max?: number;
    step?: number;
    format?: (v: number) => string;
    oninput: (v: number) => void;
    onchange?: () => void;
    testid?: string;
  } = $props();

  // The thumb (14 px) travels between the track's ends inset by half its width.
  const tick = $derived(normal === null ? null : `calc(7px + (100% - 14px) * ${(normal - min) / (max - min)})`);

  function reset() {
    if (normal !== null) {
      oninput(normal);
      onchange?.();
    }
  }
</script>

<label class="range" title={help ? `${help}${normal !== null ? ' Double-click to reset.' : ''}` : undefined}>
  <span class="head">
    <span class="label">{label}</span>
    <span class="value mono">{format(value)}</span>
  </span>
  <span class="track">
    {#if tick}
      <span class="tick" style:left={tick}></span>
    {/if}
    <input
      type="range"
      {min}
      {max}
      {step}
      {value}
      data-testid={testid}
      oninput={(e) => oninput(Number(e.currentTarget.value))}
      onchange={() => onchange?.()}
      ondblclick={reset}
    />
  </span>
</label>

<style>
  .range {
    display: block;
    padding: 3px 0;
  }

  .head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }

  .label {
    color: var(--text);
  }

  .value {
    color: var(--muted);
  }

  .track {
    position: relative;
    display: block;
  }

  .tick {
    position: absolute;
    top: 3px;
    width: 2px;
    height: 12px;
    margin-left: -1px;
    background: var(--faint);
    border-radius: 1px;
    pointer-events: none;
  }
</style>
