<script lang="ts">
  let {
    active,
    state,
    tick,
  }: {
    active: boolean
    state: 'idle' | 'saving' | 'saved' | 'failed'
    tick: number
  } = $props()
</script>

{#if active && state !== 'idle'}
  <span class="save-badge" class:failed={state === 'failed'} role="status">
    {#if state === 'saved'}
      {#key tick}
        <span class="badge-text"><svg viewBox="0 0 12 12" width="11" height="11" aria-hidden="true"><path d="M2 6.3 4.8 9 10 3.3" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" /></svg>Saved</span>
      {/key}
    {:else if state === 'failed'}
      <span class="badge-text">Not saved</span>
    {/if}
  </span>
{/if}

<style>
  .save-badge {
    display: inline-flex;
    align-items: center;
    color: var(--forest);
    font-size: var(--step--2);
    font-weight: 700;
    line-height: 1;
    white-space: nowrap;
  }

  .save-badge.failed {
    color: var(--fail);
  }

  .badge-text {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    animation: saved-in 0.18s cubic-bezier(0.22, 1, 0.36, 1);
  }

  @keyframes saved-in {
    from {
      opacity: 0.25;
      transform: translateY(2px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .badge-text {
      animation: none;
    }
  }
</style>
