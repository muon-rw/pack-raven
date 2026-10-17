<script lang="ts">
  import { onMount } from 'svelte'
  import { api, errorText, type DefenderView } from '../lib/api'
  import Notice from './Notice.svelte'

  let status = $state<DefenderView | null>(null)
  let hidden = $state(false)
  let foldersOpen = $state(false)
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
      error = `Couldn't exclude the folders: ${errorText(e)}`
    } finally {
      busy = false
    }
  }

  async function dismiss() {
    hidden = true
    await api.defenderDismiss(true).catch((e) => ((hidden = false), (error = `Couldn't save that: ${errorText(e)}`)))
  }
</script>

{#if status?.prompt && !hidden}
  <div class="banner">
    <Notice tone="warn">
      <div class="row">
        <div class="text">
          Windows Defender scans every file packraven builds, which slows builds and test runs. Excluding
          <button class="link folders" aria-expanded={foldersOpen} aria-controls="defender-folders" onclick={() => (foldersOpen = !foldersOpen)}>
            {status.missing.length === 1 ? 'this folder' : `these ${status.missing.length} folders`}
          </button>
          speeds them up, but Defender stops scanning files in them. Windows asks you to confirm.
          {#if foldersOpen}
            <ul id="defender-folders">{#each status.missing as path (path)}<li class="file">{path}</li>{/each}</ul>
          {/if}
        </div>
        <span class="actions">
          <button onclick={exclude} class:busy disabled={busy}>{busy ? 'Waiting for Windows' : 'Exclude folders'}</button>
          <button class="quiet" onclick={() => (hidden = true)}>Not now</button>
          <button class="quiet" onclick={dismiss}>Don't ask again</button>
        </span>
      </div>
      {#if error}<p class="failed" role="alert">{error}</p>{/if}
    </Notice>
  </div>
{/if}

<style>
  .banner {
    margin-bottom: 18px;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: 6px 16px;
  }

  .text {
    max-width: 72ch;
  }

  /* Chromium lays out <details> as an inline-block even with display: inline, which broke the sentence after the folder link. */
  .folders {
    color: inherit;
  }

  ul {
    margin: 4px 0;
    padding-left: 18px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .actions button {
    padding-block: 4px;
  }
</style>
