<script lang="ts">
  let { open }: { open: boolean | null } = $props()
</script>

<span class="twisty" aria-hidden="true">
  {#if open !== null}
    <svg viewBox="0 0 12 12" class:open><path d="M4.5 3l3 3-3 3" /></svg>
  {/if}
</span>

<style>
  .twisty {
    flex: none;
    width: 14px;
    height: 14px;
    display: grid;
    place-items: center;
  }

  svg {
    width: 12px;
    height: 12px;
    fill: none;
    stroke: var(--muted);
    stroke-width: 1.6;
    transition: transform 0.1s ease;
  }

  svg.open {
    transform: rotate(90deg);
  }

  @media (prefers-reduced-motion: reduce) {
    svg {
      transition: none;
    }
  }
</style>
