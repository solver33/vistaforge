<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { formatDuration, formatInt } from '../lib/format';

  const st = $derived(app.exportState);
  let now = $state(performance.now());

  $effect(() => {
    if (st.phase !== 'running') return;
    const timer = setInterval(() => (now = performance.now()), 250);
    return () => clearInterval(timer);
  });

  const running = $derived.by(() => {
    if (st.phase !== 'running') return null;
    const elapsed = (now - st.started) / 1000;
    const share = st.total > 0 ? st.done / st.total : 0;
    // Wait for a little progress before guessing how long the rest takes.
    const left = share > 0.03 && elapsed > 1 ? (elapsed / share) * (1 - share) : NaN;
    return { elapsed, share, left };
  });
</script>

{#if st.phase !== 'idle'}
  <div class="backdrop">
    <div class="dialog" role="dialog" aria-modal="true" data-testid="export-dialog" data-phase={st.phase}>
      {#if st.phase === 'confirm'}
        <h2>Replace “{st.world}”?</h2>
        <p>
          A world with this name is already in the saves folder. Replacing it deletes the old world for good. To keep
          it, cancel and change the name.
        </p>
        <div class="actions">
          <button class="btn" onclick={() => app.closeExport()}>Cancel</button>
          <button class="btn danger" data-testid="confirm-replace" onclick={() => app.runExport(true)}>Replace world</button>
        </div>
      {:else if st.phase === 'running' && running}
        <h2>Generating “{app.settings.name}”</h2>
        <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(running.share * 100)}>
          <div class="fill" style:width={`${running.share * 100}%`}></div>
        </div>
        <div class="stats">
          <span data-testid="export-progress">{formatInt(st.done)} of {formatInt(st.total)} chunks · {Math.floor(running.share * 100)}%</span>
          <span class="muted">
            {formatDuration(running.elapsed)} so far{Number.isFinite(running.left) ? ` · about ${formatDuration(running.left)} left` : ''}
          </span>
        </div>
        <div class="actions">
          <button class="btn" data-testid="cancel-export" disabled={st.cancelling} onclick={() => app.cancelExport()}>
            {st.cancelling ? 'Cancelling…' : 'Cancel'}
          </button>
        </div>
      {:else if st.phase === 'done'}
        {@const r = st.report}
        <h2>World ready</h2>
        <p>Start Minecraft with your Conquest Reforged pack, open <strong>Singleplayer</strong> and pick <strong>{app.settings.name}</strong>.</p>
        {#if app.settings.world_height === 'tall'}
          <p class="muted" data-testid="tall-warning">
            This is a tall world, so Minecraft warns about experimental settings when you open it. That is expected: carry on into the world.
          </p>
        {/if}
        <dl>
          <dt>Chunks</dt>
          <dd>{formatInt(r.chunks)} in {r.regions} region {r.regions === 1 ? 'file' : 'files'}</dd>
          <dt>Trees</dt>
          <dd>{formatInt(r.trees)}</dd>
          <dt>Block types</dt>
          <dd>{formatInt(r.block_states)}</dd>
          <dt>Spawn</dt>
          <dd>{r.spawn.join(', ')}</dd>
          <dt>Time</dt>
          <dd>{formatDuration(r.seconds)}</dd>
        </dl>
        <div class="actions">
          <button class="btn" onclick={() => app.reveal(r.world_dir)}>Show in folder</button>
          <button class="btn primary" data-testid="close-export" onclick={() => app.closeExport()}>Done</button>
        </div>
      {:else if st.phase === 'cancelled'}
        <h2>Export cancelled</h2>
        <p>Nothing was left in the saves folder.</p>
        <div class="actions">
          <button class="btn primary" data-testid="close-export" onclick={() => app.closeExport()}>Close</button>
        </div>
      {:else if st.phase === 'error'}
        <h2>Export failed</h2>
        <p class="error-text" data-testid="export-error">{st.message}</p>
        <p class="muted">Nothing was left in the saves folder.</p>
        <div class="actions">
          <button class="btn primary" data-testid="close-export" onclick={() => app.closeExport()}>Close</button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgba(8, 10, 12, 0.6);
    z-index: 20;
  }

  .dialog {
    width: 440px;
    padding: 20px 22px 18px;
    border-radius: 12px;
    background: var(--panel);
    border: 1px solid var(--line);
    box-shadow: 0 18px 48px rgba(0, 0, 0, 0.5);
    user-select: text;
  }

  h2 {
    margin: 0 0 10px;
    font-size: 17px;
    font-weight: 600;
  }

  p {
    margin: 0 0 12px;
    line-height: 1.5;
  }

  .bar {
    height: 8px;
    border-radius: 4px;
    background: var(--panel-3);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.2s linear;
  }

  .stats {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 10px 0 4px;
  }

  dl {
    display: grid;
    grid-template-columns: 110px 1fr;
    gap: 4px 10px;
    margin: 0 0 4px;
  }

  dt {
    color: var(--muted);
  }

  dd {
    margin: 0;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }
</style>
