<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import { onMount } from 'svelte'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { api, errorText, type ModSide, type Override } from '../lib/api'
  import { serverMods } from '../lib/jobs.svelte'

  type Filter = 'excluded' | 'all' | 'overridden'

  let mods: ModSide[] = $state([])
  let loadError: string | null = $state(null)
  let loading = $state(true)
  let filter: Filter = $state('excluded')
  let search = $state('')

  async function load() {
    loading = true
    try {
      mods = await api.serverSides()
    } catch (error) {
      loadError = `Couldn't read mod sides: ${errorText(error)}`
    } finally {
      loading = false
    }
  }

  onMount(load)

  async function setOverride(mod: ModSide, select: HTMLSelectElement) {
    try {
      await api.setSideOverride(mod.projectId, select.value as Override)
      serverMods.changedSinceBuild = true
    } catch (error) {
      select.value = mod.override
      loadError = `Couldn't change the override for ${mod.name}: ${errorText(error)}`
    }
    await load()
  }

  let excludedCount = $derived(mods.filter((m) => m.side === 'client').length)
  let visible = $derived(
    mods.filter((m) => {
      if (filter === 'excluded' && m.side !== 'client') return false
      if (filter === 'overridden' && m.override === 'auto') return false
      return !search || m.name.toLowerCase().includes(search.toLowerCase())
    }),
  )
</script>

<div class="server">
  <header class="head">
    <h2>Server mods</h2>
    <p class="muted">
      {mods.length} mods, {excludedCount} left off the server as client-only based on CurseForge and Modrinth tags.
      Override them here. Choices are used by Test and Publish to build server packs.
    </p>
  </header>

  {#if loadError}
    <Notice tone="error">
      <p>{loadError}</p>
      <p><button onclick={() => { loadError = null; load() }} disabled={loading}>Try again</button></p>
    </Notice>
  {/if}

  <div class="tools">
    <div class="filters" role="group" aria-label="Show">
      {#each [['excluded', 'Excluded'], ['overridden', 'Overridden'], ['all', 'All mods']] as const as [id, label] (id)}
        <button aria-pressed={filter === id} class:on={filter === id} onclick={() => (filter = id)}>{label}</button>
      {/each}
    </div>
    <input type="search" bind:value={search} placeholder="Find a mod" aria-label="Find a mod" />
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="table" role="region" aria-label="Mods" tabindex="0">
    <table aria-busy={loading && !mods.length}>
      <thead>
        <tr><th>Mod</th><th>Server</th><th>Why</th><th>Override</th></tr>
      </thead>
      <tbody>
        {#if loading && !mods.length}
          {#each { length: 8 } as _, row (row)}
            <tr class="skeleton" aria-hidden="true">
              <td><span style="width: {55 + ((row * 37) % 35)}%"></span><span class="short"></span></td>
              <td><span></span></td>
              <td><span style="width: {60 + ((row * 23) % 30)}%"></span></td>
              <td><span></span></td>
            </tr>
          {/each}
        {:else}
          {#each visible as mod (mod.path)}
            <tr>
              <td>
                {#if mod.url}<a href={mod.url} onclick={(e) => { e.preventDefault(); openUrl(mod.url!) }}>{mod.name}</a>{:else}{mod.name}{/if}
                <div class="file muted">{mod.path.slice(5)}</div>
              </td>
              <td><span class="chip {mod.side === 'client' ? 'change' : 'ok'}">{mod.side === 'client' ? 'Excluded' : mod.side === 'server' ? 'Included (for Singleplayer)' : 'Included'}</span></td>
              <td class="muted">{mod.reason}</td>
              <td>
                <select value={mod.override} onchange={(e) => setOverride(mod, e.currentTarget)} aria-label="Override for {mod.name}">
                  <option value="auto">Automatic</option>
                  <option value="client">Exclude</option>
                  <option value="keep">Keep on server</option>
                </select>
              </td>
            </tr>
          {:else}
            <tr><td colspan="4" class="muted">No mods match. Try another filter or clear the search.</td></tr>
          {/each}
        {/if}
      </tbody>
    </table>
  </div>

</div>

<style>
  .server {
    display: grid;
    gap: 18px;
  }

  .head {
    display: grid;
    gap: 4px;
  }

  .tools {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  .filters {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    padding: 3px;
    gap: 3px;
    background: var(--paper);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
  }

  .filters button {
    font-weight: 400;
    padding: 5px 16px;
    color: var(--muted);
    background: transparent;
    border-color: transparent;
  }

  .filters button.on {
    color: var(--text);
    font-weight: 700;
    border-color: var(--control-edge);
    background: var(--control);
  }

  .skeleton span {
    display: block;
    height: 0.9em;
    width: 70%;
    margin: 4px 0;
    border-radius: 3px;
    background: var(--line);
    animation: pulse 1.4s ease-in-out infinite;
  }

  .skeleton span.short {
    width: 35%;
    opacity: 0.6;
  }

  @keyframes pulse {
    50% {
      opacity: 0.45;
    }
  }

  .tools input {
    width: 240px;
  }

  .table {
    max-height: calc(100vh - 260px);
    overflow: auto;
    border: 1px solid var(--line);
    border-radius: var(--radius-panel);
  }

  table {
    width: 100%;
    border-collapse: collapse;
  }

  th {
    position: sticky;
    top: 0;
    text-align: left;
    font-size: var(--step--1);
    color: var(--muted);
    font-weight: 400;
    background: var(--surface);
    padding: 8px 12px;
  }

  td {
    padding: 8px 12px;
    border-top: 1px solid var(--line);
    vertical-align: top;
  }

  td a {
    color: var(--text);
  }

</style>
