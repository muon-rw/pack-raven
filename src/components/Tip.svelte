<script lang="ts">
  import type { Snippet } from 'svelte'

  let { tip, children }: { tip: string | null | undefined; children: Snippet } = $props()
</script>

<span class="tip" title={tip ?? undefined}>{@render children()}</span>

<style>
  .tip {
    display: inline-flex;
  }
</style>
