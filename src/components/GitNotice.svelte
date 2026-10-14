<script lang="ts">
  import { onMount } from 'svelte'
  import { api, errorText } from '../lib/api'
  import Notice from './Notice.svelte'

  let { oninstalled }: { oninstalled: () => void } = $props()

  let missing = $state(false)
  let busy = $state(false)
  let message = $state<string | null>(null)
  let failed = $state(false)

  async function check() {
    const wasMissing = missing
    missing = !(await api.gitVersion().catch(() => null))
    if (wasMissing && !missing) oninstalled()
  }

  onMount(check)

  async function install() {
    busy = true
    try {
      message = await api.installGit()
      failed = false
      await check()
    } catch (e) {
      message = errorText(e)
      failed = true
    } finally {
      busy = false
    }
  }
</script>

{#if missing}
  <div class="banner">
    <Notice tone="warn">
      <p>packraven needs Git to track your pack and sync mods between machines, and this computer doesn't have it.</p>
      {#if message}<p class:muted={!failed} class:failed>{message}</p>{/if}
      <div class="actions">
        <button class="primary" class:busy disabled={busy} onclick={install}>{busy ? 'Installing Git' : 'Install Git'}</button>
        <button class="quiet" disabled={busy} onclick={check}>Check again</button>
      </div>
      <p class="muted small">packraven downloads the official installer. You'll get a permission pop-up from your system.</p>
    </Notice>
  </div>
{/if}

<style>
  .banner {
    margin-bottom: 18px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .small {
    font-size: var(--step--1);
  }
</style>
