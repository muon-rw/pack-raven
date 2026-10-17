<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import ServerProperties from '../components/ServerProperties.svelte'
  import ServerStartup from '../components/ServerStartup.svelte'
  import { onMount } from 'svelte'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { api, errorText, type ModSide, type Overview, type Override, type PackConfig } from '../lib/api'
  import { serverPack } from '../lib/jobs.svelte'
  import { serverView } from '../lib/nav.svelte'
  import { commandKey, shortcut } from '../lib/platform'

  let { overview }: { overview: Overview | null } = $props()

  type Filter = 'excluded' | 'all' | 'overridden'
  type Tab = typeof serverView.tab
  type Settings = Pick<PackConfig['server'], 'memory' | 'jvm_args' | 'args' | 'graalvm' | 'properties'>

  const tabs: [Tab, string][] = [
    ['mods', 'Mods'],
    ['properties', 'Properties'],
    ['startup', 'Startup'],
  ]

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

  let settings = $state<Settings | null>(null)
  let settingsError = $state<string | null>(null)
  let testPort = $state(25565)
  let icon = $state<string | null>(null)
  let launch = $state<{ windows: string; unix: string } | null>(null)
  let saveState = $state<'idle' | 'saving' | 'saved' | 'failed'>('idle')
  let saveError = $state('')

  async function loadSettings() {
    try {
      const config = await api.getPackConfig()
      const { memory, jvm_args, args, graalvm, properties } = config.server
      settings = { memory, jvm_args, args, graalvm, properties }
      testPort = config.test.port
      settingsError = null
    } catch (error) {
      settingsError = `Couldn't read packraven.toml: ${errorText(error)}`
    }
    icon = await api.getServerIcon().catch(() => null)
    launch = await api.serverLaunch().catch(() => null)
  }

  onMount(() => {
    load()
    loadSettings()
  })

  function markChanged() {
    serverPack.changedSinceBuild = true
    serverPack.changedSinceTest = true
  }

  // The Mods tab saves overrides on its own, so each save merges into a fresh read of packraven.toml
  // instead of writing back the config we loaded. Chaining keeps quick edits in order.
  let saving = Promise.resolve()
  function saveSettings(changes: Partial<Settings>) {
    if (!settings) return
    Object.assign(settings, changes)
    saveState = 'saving'
    saving = saving.then(async () => {
      try {
        const config = await api.getPackConfig()
        Object.assign(config.server, changes)
        await api.savePackConfig(config)
        markChanged()
        saveState = 'saved'
      } catch (error) {
        saveError = errorText(error)
        saveState = 'failed'
      }
    })
  }

  function onTabKey(event: KeyboardEvent) {
    const index = tabs.findIndex(([tab]) => tab === serverView.tab)
    const next = { ArrowRight: index + 1, ArrowLeft: index - 1, Home: 0, End: tabs.length - 1 }[event.key]
    if (next === undefined) return
    event.preventDefault()
    serverView.tab = tabs[(next + tabs.length) % tabs.length][0]
    document.getElementById(`server-tab-${serverView.tab}`)?.focus()
  }

  // Exclude and Keep fix the side outright, so we only reload the tags for Automatic.
  async function setOverride(mod: ModSide, select: HTMLSelectElement) {
    const mode = select.value as Override
    try {
      await api.setSideOverride(mod.projectId, mode)
      markChanged()
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

  let searchField = $state<HTMLInputElement>()

  function onKey(event: KeyboardEvent) {
    if (serverView.tab !== 'mods' || event.key.toLowerCase() !== 'f' || event.shiftKey || !commandKey(event) || event.defaultPrevented) return
    event.preventDefault()
    searchField?.focus()
    searchField?.select()
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="server">
  <header class="head">
    <div class="title">
      <h2>Server</h2>
      <p class="muted">
        {#if serverView.tab === 'properties'}
          Written to <span class="file">server.properties</span> in every server pack and test run. All other values falls back to vanilla or modded defaults.
        {:else if serverView.tab === 'startup'}
          Startup settings for the published server pack. We'll write these into the pack's <span class="file">user_jvm_args.txt</span> and run scripts.
        {:else if loading && !mods.length}
          Reading which mods the server needs.
        {:else if !mods.length}
          This pack has no mods yet.
        {:else}
          {mods.length} {mods.length === 1 ? 'mod' : 'mods'}. {excludedCount || 'none'} excluded from the server as client-only, based on their
          CurseForge and Modrinth tags. Override that here; these choices will be used in Test and Publish to make server packs.
        {/if}
      </p>
    </div>
    {#if serverView.tab !== 'mods'}
      <p class="save" class:failed={saveState === 'failed'} role="status">
        {#if saveState === 'saving'}Saving{:else if saveState === 'saved'}Saved to <span class="file">packraven.toml</span>{:else if saveState === 'failed'}Couldn't save: {saveError}{/if}
      </p>
    {/if}
  </header>

  <div class="tab-row">
    <div class="tabs" role="tablist" aria-label="Server" tabindex="-1" onkeydown={onTabKey}>
      {#each tabs as [tab, label] (tab)}
        <button role="tab" id="server-tab-{tab}" aria-selected={serverView.tab === tab} tabindex={serverView.tab === tab ? 0 : -1} aria-controls="server-panel" class:on={serverView.tab === tab} onclick={() => (serverView.tab = tab)}>
          {label}{#if tab === 'mods' && mods.length}<span class="muted tab-count">{mods.length}</span>{/if}
        </button>
      {/each}
    </div>
    {#if serverView.tab === 'mods'}
      <input type="search" class="find" bind:this={searchField} bind:value={search} placeholder="Find a mod" aria-label="Find a mod" title="Find a mod ({shortcut('Ctrl+F')})" />
    {/if}
  </div>

  <div class="panel" id="server-panel" role="tabpanel" aria-labelledby="server-tab-{serverView.tab}">
  {#if serverView.tab !== 'mods'}
    {#if settingsError}
      <Notice tone="error">
        <p>{settingsError}</p>
        <p><button onclick={loadSettings}>Try again</button></p>
      </Notice>
    {:else if settings && serverView.tab === 'properties'}
      <ServerProperties
        properties={settings.properties}
        packName={overview?.name ?? 'This pack'}
        version={overview?.version ?? null}
        minecraft={overview?.minecraft ?? null}
        bind:icon
        hasProject={!!overview?.projectId}
        {testPort}
        onchange={(properties) => saveSettings({ properties })}
        oniconchange={markChanged}
      />
    {:else if settings}
      <ServerStartup
        memory={settings.memory}
        jvmArgs={settings.jvm_args}
        args={settings.args}
        graalvm={settings.graalvm}
        minecraft={overview?.minecraft ?? null}
        {launch}
        {testPort}
        onchange={saveSettings}
      />
    {/if}
  {:else}
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
  {/if}
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
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    justify-content: space-between;
    gap: 8px 24px;
  }

  .title {
    display: grid;
    gap: 4px;
  }

  .title p {
    max-width: 68ch;
  }

  .save {
    color: var(--muted);
    font-size: var(--step--1);
  }

  .save.failed {
    color: var(--fail);
  }

  .tab-row {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    justify-content: space-between;
    gap: 8px 16px;
    border-bottom: 1px solid var(--line);
  }

  .tabs {
    display: flex;
    gap: 4px;
  }

  .tabs button {
    margin-bottom: -1px;
    padding: 8px 14px;
    font-weight: 400;
    color: var(--muted);
    background: transparent;
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: 0;
  }

  .tabs button:hover:not(.on) {
    color: var(--text);
    background: transparent;
  }

  .tabs button.on {
    color: var(--text);
    font-weight: 700;
    border-bottom-color: var(--forest);
  }

  .tab-count {
    margin-left: 6px;
    font-weight: 400;
    font-variant-numeric: tabular-nums;
  }

  .find {
    flex: 0 1 260px;
    min-width: min(100%, 15rem);
    margin-bottom: 6px;
  }

  .panel {
    container-type: inline-size;
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 18px;
    min-height: 0;
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
