<script lang="ts">
  import { onMount } from 'svelte'
  import { api, errorText, type DefenderView } from '../lib/api'
  import Notice from './Notice.svelte'

  let status = $state<DefenderView | null>(null)
  let hidden = $state(false)
  let busy = $state(false)
  let error = $state<string | null>(null)

  onMount(async () => {
    status = await api.defenderStatus().catch(() => null)
  })

  async function exclude() {
    if (!status) return
    busy = true
    try {
      await api.defenderExclude(status.missing)
      status = await api.defenderStatus()
      error = null
    } catch (e) {
      error = errorText(e)
    } finally {
      busy = false
    }
  }

  async function dismiss() {
    await api.defenderDismiss(true)
    hidden = true
  }
</script>

{#if status?.prompt && !hidden}
  <div class="banner">
    <Notice tone="warn">
      <p>Windows Defender can slow things down. Exclude these folders to speed them up, but note that Defender will stop scanning files in them.</p>
      <ul>{#each status.missing as path (path)}<li class="file">{path}</li>{/each}</ul>
      {#if error}<p class="failed">{error}</p>{/if}
      <div class="actions">
        <button onclick={exclude} class:busy disabled={busy}>{busy ? 'Waiting for Windows' : 'Exclude folders'}</button>
        <button class="quiet" onclick={() => (hidden = true)}>Not now</button>
        <button class="quiet" onclick={dismiss}>Don't ask again</button>
      </div>
      <p class="muted small">You'll get a pop-up from Windows.</p>
    </Notice>
  </div>
{/if}

<style>
  .banner {
    margin-bottom: 18px;
  }

  ul {
    margin: 0;
    padding-left: 18px;
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
