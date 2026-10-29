<script lang="ts">
  import type { Snippet } from 'svelte'

  // A notice full of controls announces its own results, so it opts out of being a live region,
  // which would read out every row that moves.
  let { tone, live = true, children }: { tone: 'warn' | 'error' | 'ok'; live?: boolean; children: Snippet } = $props()

  const labels = { warn: 'Warning', error: 'Error', ok: 'Done' }
</script>

<div class="notice {tone}" role={!live ? undefined : tone === 'error' ? 'alert' : 'status'}>
  <svg viewBox="0 0 16 16" aria-label={labels[tone]} role="img">
    {#if tone === 'ok'}
      <path d="M3 8.5l3 3 7-7" />
    {:else if tone === 'error'}
      <path d="M4 4l8 8M12 4l-8 8" />
    {:else}
      <path d="M8 3v6M8 12v1" />
    {/if}
  </svg>
  <div class="body">{@render children()}</div>
</div>

<style>
  .notice {
    display: grid;
    grid-template-columns: 16px 1fr;
    gap: 10px;
    padding: 10px 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    background: var(--fill);
  }

  .warn {
    --fill: var(--warn-fill);
    --border: var(--warn-border);
    --tone: var(--warn-icon);
  }

  .error {
    --fill: var(--fail-fill);
    --border: var(--fail-border);
    --tone: var(--fail-icon);
  }

  .ok {
    --fill: var(--ok-fill);
    --border: var(--ok-border);
    --tone: var(--forest);
  }

  svg {
    width: 16px;
    height: 16px;
    margin-top: 3px;
    fill: none;
    stroke: var(--tone);
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .body {
    display: grid;
    gap: 6px;
    min-width: 0;
  }

  .body :global(ul) {
    margin: 0;
    padding-left: 18px;
    max-height: 160px;
    overflow: auto;
  }
</style>
