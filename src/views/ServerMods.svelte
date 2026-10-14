<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import { onMount } from 'svelte'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { api, errorText, type ModSide, type Override } from '../lib/api'
  import { serverMods } from '../lib/jobs.svelte'
  import { commandKey, shortcut } from '../lib/platform'

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

  // Exclude and Keep fix the side outright, so we only reload the tags for Automatic.
  async function setOverride(mod: ModSide, select: HTMLSelectElement) {
    const mode = select.value as Override
    try {
      await api.setSideOverride(mod.projectId, mode)
      serverMods.changedSinceBuild = true
      serverMods.changedSinceTest = true
    } catch (error) {
      select.value = mod.override
      loadError = `Couldn't change the override for ${mod.name}: ${errorText(error)}`
      return
    }
    if (mode === 'auto') return load()
    mods = mods.map((m) =>
      m.path === mod.path
        ? { ...m, override: mode, side: mode === 'client' ? 'client' : 'both', reason: mode === 'client' ? 'You excluded it' : 'You kept it on the server' }
        : m,
    )
  }

  let excludedCount = $derived(mods.filter((m) => m.side === 'client').length)
  let visible = $derived(
    mods.filter((m) => {
      if (filter === 'excluded' && m.side !== 'client') return false
      if (filter === 'overridden' && m.override === 'auto') return false
      return !search || m.name.toLowerCase().includes(search.toLowerCase())
    }),
  )

  let searchField: HTMLInputElement

  function onKey(event: KeyboardEvent) {
    if (event.key.toLowerCase() !== 'f' || event.shiftKey || !commandKey(event) || event.defaultPrevented) return
    event.preventDefault()
    searchField.focus()
    searchField.select()
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="server">
  <header class="head">
    <h2>Server mods</h2>
    <p class="muted">
      {#if loading && !mods.length}
        Reading which mods the server needs.
      {:else if !mods.length}
        This pack has no mods yet.
      {:else}
        {mods.length} {mods.length === 1 ? 'mod' : 'mods'}. packraven leaves {excludedCount || 'none'} off the server as client-only, based on their
        CurseForge and Modrinth tags. Override that here; Test and Publish build server packs with your choices.
      {/if}
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
    <input type="search" bind:this={searchField} bind:value={search} placeholder="Find a mod" aria-label="Find a mod" title="Find a mod ({shortcut('Ctrl+F')})" />
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
              <td><span class="chip {mod.side === 'client' ? 'change' : 'ok'}">{mod.side === 'client' ? 'Excluded' : mod.side === 'server' ? 'Included, server-side' : 'Included'}</span></td>
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
            <tr>
              <td colspan="4" class="muted">
                {#if search}
                  No mods match "{search}". <button class="link" onclick={() => (search = '')}>Clear the search</button>
                {:else if filter === 'excluded'}
                  packraven isn't leaving any mods off the server.
                {:else if filter === 'overridden'}
                  You haven't overridden any mods. Every mod uses its CurseForge and Modrinth tags.
                {:else}
                  This pack has no mods yet.
                {/if}
              </td>
            </tr>
          {/each}
        {/if}
      </tbody>
    </table>
  </div>

</div>

<style>
  .server {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 18px;
    min-height: 0;
  }

  .head {
    display: grid;
    gap: 4px;
  }

  .head p {
    max-width: 64ch;
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

  .filters button:hover:not(.on) {
    color: var(--text);
    background: var(--control-hi);
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
    border-radius: var(--radius-small);
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

  /* We let the table fill the rest of the view, so it scrolls without a page scroll around it. */
  .table {
    flex: 1;
    min-height: 240px;
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

  td .file {
    overflow-wrap: anywhere;
  }

</style>
