<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import JobLog from '../components/JobLog.svelte'
  import ModBrowser from '../components/ModBrowser.svelte'
  import { onMount, untrack } from 'svelte'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { ask } from '@tauri-apps/plugin-dialog'
  import { api, errorText, type InstalledMod, type ModCheck, type ModFile, type ReleaseType, type ModRequest } from '../lib/api'
  import { job, onJobEnd, serverMods } from '../lib/jobs.svelte'
  import { availableUpdate, browse, modUpdates, newestOnChannel } from '../lib/modUpdates.svelte'
  import { commandKey, shortcut, trashName } from '../lib/platform'

  let { pack, game, clientRunning }: { pack: string | null; game: string; clientRunning: boolean } = $props()

  type Filter = 'updates' | 'locked' | 'all'

  // We ask CurseForge again when you open the view after this long.
  const CHECK_MAX_AGE_MS = 15 * 60 * 1000

  let mods: InstalledMod[] = $state([])
  let loading = $state(true)
  let checking = $state(false)
  let loadError: string | null = $state(null)
  let checkError: string | null = $state(null)
  let actionError: string | null = $state(null)
  let filter: Filter = $state('all')
  let search = $state('')
  let expanded: string | null = $state(null)
  let versions: ModFile[] | null = $state(null)
  let versionsError: string | null = $state(null)
  let removing: string | null = $state(null)

  let updating = $derived(job('mods').phase === 'running')
  let blocked = $derived(updating || clientRunning)

  // App remounts this view when you switch packs, so we only compare the pack once.
  untrack(() => {
    if (modUpdates.pack === pack) return
    modUpdates.pack = pack
    modUpdates.checks = null
    modUpdates.checkedAt = 0
  })

  async function load() {
    try {
      mods = await api.installedMods()
      loadError = null
    } catch (error) {
      loadError = `Couldn't read the mod list: ${errorText(error)}`
    } finally {
      loading = false
    }
  }

  async function check() {
    checking = true
    try {
      const checks = await api.checkModUpdates()
      modUpdates.checks = new Map(checks.map((c) => [c.path, c]))
      modUpdates.checkedAt = Date.now()
      checkError = null
    } catch (error) {
      checkError = `Couldn't check CurseForge for updates: ${errorText(error)}`
    } finally {
      checking = false
    }
  }

  onMount(async () => {
    api
      .getPackConfig()
      .then((config) => {
        modUpdates.channel = config.updates.channel
        modUpdates.locked = new Set(config.updates.locked)
      })
      .catch(() => {})
    await load()
    const unchecked = mods.some((m) => !modUpdates.checks?.has(m.path))
    if (mods.length && (unchecked || Date.now() - modUpdates.checkedAt > CHECK_MAX_AGE_MS)) await check()
  })

  onJobEnd('mods', async (phase) => {
    if (phase === 'done') {
      serverMods.changedSinceBuild = true
      serverMods.changedSinceTest = true
    }
    expanded = null
    await load()
    await check()
  })

  async function setChannel(select: HTMLSelectElement) {
    const previous = modUpdates.channel
    modUpdates.channel = select.value as ReleaseType
    try {
      await api.setUpdateChannel(modUpdates.channel)
    } catch (error) {
      modUpdates.channel = previous
      select.value = previous
      actionError = `Couldn't save the update channel: ${errorText(error)}`
    }
  }

  async function setLocked(mod: InstalledMod, lock: boolean) {
    actionError = null
    try {
      await api.setModLocked(mod.projectId, lock)
      const locked = new Set(modUpdates.locked)
      if (lock) locked.add(mod.projectId)
      else locked.delete(mod.projectId)
      modUpdates.locked = locked
    } catch (error) {
      actionError = `Couldn't ${lock ? 'lock' : 'unlock'} ${mod.name}: ${errorText(error)}`
    }
  }

  async function update(requests: ModRequest[]) {
    actionError = null
    try {
      await api.changeMods(requests)
    } catch (error) {
      actionError = `Couldn't start the update: ${errorText(error)}`
    }
  }

  function requiredBy(mod: InstalledMod): string[] {
    return mods.filter((m) => m.path !== mod.path && modUpdates.checks?.get(m.path)?.requires.includes(mod.projectId)).map((m) => m.name)
  }

  async function remove(mod: InstalledMod) {
    // We only know what depends on a mod from a check, so we finish one before warning you.
    if (mods.some((m) => !modUpdates.checks?.has(m.path))) await check()
    const dependents = requiredBy(mod)
    const lines = [`Move ${fileName(mod.path)} to the ${trashName}?`]
    if (dependents.length) lines.push(`${list(dependents)} ${dependents.length === 1 ? 'requires' : 'require'} ${mod.name}, so ${dependents.length === 1 ? 'it' : 'they'} may not load without it.`)
    const confirmed = await ask(lines.join('\n\n'), { title: `Remove ${mod.name}`, kind: 'warning', okLabel: `Move to ${trashName}`, cancelLabel: 'Cancel' })
    if (!confirmed) return
    removing = mod.path
    actionError = null
    try {
      await api.removeMods([mod.path])
      serverMods.changedSinceBuild = true
      serverMods.changedSinceTest = true
      if (modUpdates.checks) modUpdates.checks = new Map([...modUpdates.checks].filter(([path]) => path !== mod.path))
      if (expanded === mod.path) expanded = null
      await load()
    } catch (error) {
      actionError = `Couldn't remove ${mod.name}: ${errorText(error)}`
    } finally {
      removing = null
    }
  }

  async function toggleVersions(mod: InstalledMod) {
    if (expanded === mod.path) {
      expanded = null
      return
    }
    expanded = mod.path
    versions = null
    versionsError = null
    try {
      const files = await api.modVersions(mod.path, mod.projectId)
      if (expanded === mod.path) versions = files
    } catch (error) {
      if (expanded === mod.path) versionsError = `Couldn't list the versions on CurseForge: ${errorText(error)}`
    }
  }

  function list(names: string[]) {
    return names.length < 3 ? names.join(' and ') : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`
  }

  function fileName(path: string) {
    return path.slice(path.lastIndexOf('/') + 1)
  }

  const kinds: Record<string, string> = { resourcepacks: 'Resource pack', shaderpacks: 'Shader pack', datapacks: 'Data pack' }

  function kind(path: string) {
    return kinds[path.slice(0, path.indexOf('/'))]
  }

  function date(iso: string | null) {
    return iso ? new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' }) : ''
  }

  const typeLabels: Record<ReleaseType, string> = { release: 'Release', beta: 'Beta', alpha: 'Alpha' }

  let updates = $derived(new Map(mods.map((m) => [m.path, availableUpdate(modUpdates.checks?.get(m.path))] as const).filter(([, file]) => file)) as Map<string, ModFile>)
  let duplicates = $derived.by(() => {
    const byProject = new Map<number, InstalledMod[]>()
    for (const mod of mods.filter((m) => m.path.startsWith('mods/') && !m.path.endsWith('.disabled'))) byProject.set(mod.projectId, [...(byProject.get(mod.projectId) ?? []), mod])
    return [...byProject.values()].filter((group) => group.length > 1)
  })
  let duplicatePaths = $derived(new Set(duplicates.flat().map((m) => m.path)))
  let visible = $derived(
    mods.filter((m) => {
      if (filter === 'updates' && !updates.has(m.path)) return false
      if (filter === 'locked' && !modUpdates.locked.has(m.projectId)) return false
      return !search || m.name.toLowerCase().includes(search.toLowerCase()) || m.path.toLowerCase().includes(search.toLowerCase())
    }),
  )

  function checkFor(mod: InstalledMod): ModCheck | undefined {
    return modUpdates.checks?.get(mod.path)
  }

  let summary = $derived.by(() => {
    if (checking) return 'Checking CurseForge for updates.'
    if (!modUpdates.checks) return null
    if (!updates.size) return 'Everything is up to date.'
    return `${updates.size} ${updates.size === 1 ? 'has an update' : 'have updates'}.`
  })

  let installedIds = $derived(new Set(mods.map((m) => m.projectId)))

  let searchField: HTMLInputElement | undefined = $state()
  let browser: ModBrowser | undefined = $state()

  function onKey(event: KeyboardEvent) {
    if (event.key.toLowerCase() !== 'f' || event.shiftKey || !commandKey(event) || event.defaultPrevented) return
    event.preventDefault()
    if (browse.tab === 'browse') browser?.focusSearch()
    else {
      searchField?.focus()
      searchField?.select()
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- Browsers won't break a line between a hyphen and a digit, so long jar names would hold the
     table wider than the view without these break points. -->
{#snippet fileText(name: string)}<span class="file">{#each name.split(/(?<=[-_+])/) as part, index (index)}{#if index}<wbr />{/if}{part}{/each}</span>{/snippet}

<div class="mods">
  <header class="head">
    <div class="title">
    <h2>Mods</h2>
    <p class="muted">
      {#if browse.tab === 'browse'}
        Search CurseForge for mods, resource packs and shaders that run on {game}.
      {:else if loading && !mods.length}
        Reading the mod list.
      {:else if !mods.length}
        This pack has no mods from CurseForge yet.
      {:else}
        {mods.length} {mods.length === 1 ? 'file' : 'files'} from CurseForge. {summary ?? ''}
      {/if}
    </p>
    </div>
    {#if browse.tab === 'installed'}
    <div class="actions">
      <button onclick={check} disabled={checking || updating || !mods.length} class:busy={checking}>{checking ? 'Checking' : 'Check for updates'}</button>
      <button
        class="primary"
        class:busy={updating}
        disabled={!updates.size || blocked}
        onclick={() => update([...updates].map(([path, file]) => ({ fileId: file.fileId, replaces: path })))}
      >
        {updating ? 'Updating' : updates.size ? `Update all (${updates.size})` : 'Update all'}
      </button>
    </div>
    {/if}
  </header>

  <div class="tabs" role="tablist" aria-label="Mods">
    {#each [['installed', 'Installed'], ['browse', 'Browse']] as const as [tab, label] (tab)}
      <button role="tab" id="mods-tab-{tab}" aria-selected={browse.tab === tab} aria-controls="mods-panel" class:on={browse.tab === tab} onclick={() => (browse.tab = tab)}>
        {label}{#if tab === 'installed' && mods.length}<span class="muted tab-count">{mods.length}</span>{/if}
      </button>
    {/each}
  </div>

  {#if loadError}
    <Notice tone="error">
      <p>{loadError}</p>
      <p><button onclick={() => { loadError = null; load() }}>Try again</button></p>
    </Notice>
  {/if}
  {#if checkError}
    <Notice tone="error">
      <p>{checkError}</p>
      <p><button onclick={check} disabled={checking}>Try again</button></p>
    </Notice>
  {/if}
  {#if clientRunning}
    <Notice tone="warn"><p>The test client has this pack's mods open, so packraven won't change them until you close it.</p></Notice>
  {/if}
  {#if duplicates.length}
    <Notice tone="warn">
      <p>
        {duplicates.length === 1 ? `The mods folder has two versions of ${duplicates[0][0].name}` : `The mods folder has two versions of ${list(duplicates.map((group) => group[0].name))}`},
        and Minecraft won't load {duplicates.length === 1 ? 'it' : 'them'} that way. Remove the one you don't want. packraven won't publish until you do.
      </p>
    </Notice>
  {/if}
  {#if actionError}
    <Notice tone="error"><p>{actionError}</p></Notice>
  {/if}
  {#if job('mods').phase === 'done' && job('mods').message}
    <Notice tone="ok"><p>{job('mods').message}</p></Notice>
  {:else if job('mods').phase === 'failed'}
    <Notice tone="error">
      <p>Couldn't finish changing mods: {job('mods').message}</p>
    </Notice>
  {/if}
  {#if updating || job('mods').phase === 'failed'}
    <details class="log" open={job('mods').phase === 'failed'}>
      <summary>{updating ? 'Downloading mods' : 'Log'}</summary>
      <JobLog lines={job('mods').lines} label="Mod changes log" />
    </details>
  {/if}

  <div class="panel" id="mods-panel" role="tabpanel" aria-labelledby="mods-tab-{browse.tab}">
  {#if browse.tab === 'browse'}
    <ModBrowser bind:this={browser} {installedIds} {blocked} {game} install={update} />
  {:else}
  <div class="tools">
    <div class="filters" role="group" aria-label="Show">
      <button aria-pressed={filter === 'all'} class:on={filter === 'all'} onclick={() => (filter = 'all')}>All</button>
      <button aria-pressed={filter === 'updates'} class:on={filter === 'updates'} onclick={() => (filter = 'updates')}>
        Updates{#if updates.size}<span class="count">{updates.size}</span>{/if}
      </button>
      <button aria-pressed={filter === 'locked'} class:on={filter === 'locked'} onclick={() => (filter = 'locked')}>Locked</button>
    </div>
    <label class="channel">
      Update to
      <select value={modUpdates.channel} onchange={(e) => setChannel(e.currentTarget)}>
        <option value="release">Releases only</option>
        <option value="beta">Releases and betas</option>
        <option value="alpha">Any release type</option>
      </select>
    </label>
    <input type="search" bind:this={searchField} bind:value={search} placeholder="Find a mod" aria-label="Find a mod" title="Find a mod ({shortcut('Ctrl+F')})" />
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="table" role="region" aria-label="Mods" tabindex="0">
    <table aria-busy={loading && !mods.length}>
      <thead>
        <tr><th>Mod</th><th>Version</th><th><span class="visually-hidden">Actions</span></th></tr>
      </thead>
      <tbody>
        {#if loading && !mods.length}
          {#each { length: 8 } as _, row (row)}
            <tr class="skeleton" aria-hidden="true">
              <td><span style="width: {55 + ((row * 37) % 35)}%"></span></td>
              <td><span style="width: {60 + ((row * 23) % 30)}%"></span></td>
              <td></td>
            </tr>
          {/each}
        {:else}
          {#each visible as mod (mod.path)}
            {@const status = checkFor(mod)}
            {@const next = updates.get(mod.path)}
            {@const lockedHere = modUpdates.locked.has(mod.projectId)}
            {@const held = newestOnChannel(status)}
            <tr class:open={expanded === mod.path}>
              <td>
                <div class="identity">
                  {#if status?.logo}<img src={status.logo} alt="" width="28" height="28" loading="lazy" />{:else}<span class="logo" aria-hidden="true"></span>{/if}
                  <div>
                    {#if mod.url}<a href={mod.url} onclick={(e) => { e.preventDefault(); openUrl(mod.url!) }}>{mod.name}</a>{:else}{mod.name}{/if}
                    {#if kind(mod.path)}<span class="muted kind">{kind(mod.path)}</span>{/if}
                    {#if mod.path.endsWith('.disabled')}<span class="chip">Disabled</span>{/if}
                    {#if duplicatePaths.has(mod.path)}<span class="chip fail">Two versions</span>{/if}
                  </div>
                </div>
              </td>
              <td class="version">
                <div>
                  {@render fileText(fileName(mod.path).replace(/\.disabled$/, ''))}
                  {#if status?.releaseType && status.releaseType !== 'release'}<span class="chip pending">{typeLabels[status.releaseType]}</span>{/if}
                </div>
                {#if lockedHere && held}
                  <div class="muted held">Locked. {@render fileText(held.fileName)} is available.</div>
                {:else if next}
                  <div class="next">
                    <span class="visually-hidden">Update available:</span>
                    <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2 6h8M7 3l3 3-3 3" /></svg>
                    {@render fileText(next.fileName)}
                    {#if next.releaseType !== 'release'}<span class="chip pending">{typeLabels[next.releaseType]}</span>{/if}
                  </div>
                {/if}
              </td>
              <td>
                <div class="row-actions">
                {#if next}
                  <button onclick={() => update([{ fileId: next.fileId, replaces: mod.path }])} disabled={blocked} aria-label="Update {mod.name}">Update</button>
                {/if}
                <button
                  class="ghost lock"
                  aria-pressed={lockedHere}
                  onclick={() => setLocked(mod, !lockedHere)}
                  aria-label="Lock {mod.name} to this version"
                  title={lockedHere ? 'Locked to this version. Click to allow updates.' : 'Lock to this version'}
                >
                  <svg viewBox="0 0 16 16" aria-hidden="true">
                    <rect x="3" y="7" width="10" height="7" rx="1.5" />
                    <path d={lockedHere ? 'M5.5 7V5a2.5 2.5 0 0 1 5 0v2' : 'M5.5 7V5a2.5 2.5 0 0 1 4.9-.7'} />
                  </svg>
                </button>
                <button class="ghost" aria-expanded={expanded === mod.path} onclick={() => toggleVersions(mod)} aria-label="Versions of {mod.name}">Versions</button>
                <button class="ghost" onclick={() => remove(mod)} disabled={blocked || removing === mod.path} aria-label="Remove {mod.name}">Remove</button>
                </div>
              </td>
            </tr>
            {#if expanded === mod.path}
              <tr class="versions">
                <td colspan="3">
                  {#if versionsError}
                    <p class="failed">{versionsError}</p>
                  {:else if !versions}
                    <p class="muted">Listing files on CurseForge.</p>
                  {:else if !versions.length}
                    <p class="muted">CurseForge has no files of {mod.name} for this pack's Minecraft version and loader.</p>
                  {:else}
                    <ul aria-label="Versions of {mod.name}">
                      {#each versions as file (file.fileId)}
                        <li>
                          {@render fileText(file.fileName)}
                          <span class="chip {file.releaseType === 'release' ? 'ok' : 'pending'}">{typeLabels[file.releaseType]}</span>
                          <span class="muted">{date(file.fileDate)}</span>
                          {#if file.fileId === mod.fileId}
                            <span class="muted current">Installed</span>
                          {:else}
                            <button onclick={() => update([{ fileId: file.fileId, replaces: mod.path }])} disabled={blocked}>
                              Use this
                            </button>
                          {/if}
                        </li>
                      {/each}
                    </ul>
                  {/if}
                </td>
              </tr>
            {/if}
          {:else}
            <tr>
              <td colspan="3" class="muted">
                {#if search}
                  No mods match "{search}". <button class="link" onclick={() => (search = '')}>Clear the search</button>
                {:else if filter === 'updates'}
                  {modUpdates.checks ? 'Every mod is on its newest file for this update channel.' : "Check for updates to see which mods have one."}
                {:else if filter === 'locked'}
                  You haven't locked any mods. Lock a mod to keep it on its current version when you update the rest.
                {:else}
                  This pack has no mods from CurseForge yet. <button class="link" onclick={() => (browse.tab = 'browse')}>Browse CurseForge</button>
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
  .mods {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 18px;
    min-height: 0;
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    justify-content: space-between;
    gap: 12px 24px;
  }

  .title {
    display: grid;
    gap: 4px;
  }

  .head p {
    max-width: 64ch;
  }

  .tabs {
    display: flex;
    gap: 4px;
    border-bottom: 1px solid var(--line);
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

  .panel {
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
    gap: 12px 16px;
  }

  .actions {
    display: flex;
    gap: 8px;
  }

  .filters {
    display: grid;
    grid-template-columns: repeat(3, auto);
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

  .count {
    margin-left: 6px;
    padding: 0 6px;
    border-radius: 999px;
    font-size: var(--step--1);
    font-variant-numeric: tabular-nums;
    background: var(--here);
    color: var(--ochre-ink);
  }

  .channel {
    white-space: nowrap;
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 100%;
  }

  .channel select {
    min-width: 0;
  }

  .tools input {
    width: 220px;
  }

  .log summary {
    cursor: pointer;
    color: var(--muted);
    margin-bottom: 8px;
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
    z-index: 1;
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
    vertical-align: middle;
  }

  td a {
    color: var(--text);
    font-weight: 700;
  }

  td .file {
    margin-right: 6px;
  }

  .held {
    margin-top: 2px;
    font-size: var(--step--1);
  }

  .lock svg {
    width: 16px;
    height: 16px;
    vertical-align: -3px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
  }

  .lock[aria-pressed='false'] {
    opacity: 0.35;
  }

  tr:hover .lock,
  .lock:focus-visible {
    opacity: 1;
  }

  .lock[aria-pressed='true'] {
    color: var(--ochre-ink);
  }

  .lock[aria-pressed='true'] rect {
    fill: currentColor;
  }

  .held .file {
    margin-right: 0;
  }

  .next {
    margin-top: 2px;
    color: var(--ochre-ink);
  }

  .next svg {
    width: 12px;
    height: 12px;
    margin-right: 4px;
    vertical-align: -1px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .identity {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .identity img,
  .logo {
    flex: none;
    width: 28px;
    height: 28px;
    border-radius: var(--radius-small);
    object-fit: cover;
  }

  .logo {
    background: var(--line);
  }

  .kind {
    display: block;
    font-size: var(--step--1);
  }

  .row-actions {
    display: flex;
    justify-content: flex-end;
    gap: 2px;
  }

  .row-actions .ghost {
    padding-inline: 8px;
  }

  tr.open td {
    background: var(--paper);
  }

  .versions td {
    border-top: 0;
    padding: 0 12px 12px 50px;
    background: var(--paper);
  }

  .versions ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 2px;
    max-height: 280px;
    overflow: auto;
  }

  .versions li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 10px;
    padding: 4px 0;
  }

  .versions li button,
  .versions .current {
    margin-left: auto;
  }

  .versions p {
    margin: 4px 0;
  }
</style>
