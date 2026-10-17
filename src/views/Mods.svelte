<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import JobLog from '../components/JobLog.svelte'
  import ModBrowser from '../components/ModBrowser.svelte'
  import ContextMenu from '../components/ContextMenu.svelte'
  import { onMount, untrack } from 'svelte'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { ask } from '@tauri-apps/plugin-dialog'
  import { api, errorText, type InstalledMod, type ModCheck, type ModFile, type ReleaseType, type ModRequest } from '../lib/api'
  import { job, onJobEnd, serverMods } from '../lib/jobs.svelte'
  import { availableUpdate, browse, modUpdates, newestOnChannel } from '../lib/modUpdates.svelte'
  import { sanitizeRemoteHtml } from '../lib/markdown'
  import type { MenuItem } from '../lib/menu'
  import { commandKey, shortcut, trashName } from '../lib/platform'

  let { pack, game, clientRunning }: { pack: string | null; game: string; clientRunning: boolean } = $props()

  type Kind = 'mods' | 'resourcepacks' | 'shaderpacks' | 'datapacks'
  type SortKey = 'enabled' | 'name' | 'author' | 'update' | 'locked'
  type Detail = 'versions' | 'description'

  // We ask CurseForge again when you open the view after this long.
  const CHECK_MAX_AGE_MS = 15 * 60 * 1000

  let mods: InstalledMod[] = $state([])
  let loading = $state(true)
  let checking = $state(false)
  let loadError: string | null = $state(null)
  let checkError: string | null = $state(null)
  let actionError: string | null = $state(null)
  let kindFilter: Kind = $state('mods')
  let search = $state('')
  let sort: { key: SortKey; reversed: boolean } = $state({ key: 'update', reversed: false })
  let expanded: string | null = $state(null)
  let detail: Detail = $state('versions')
  let versions: ModFile[] | null = $state(null)
  let versionsError: string | null = $state(null)
  let descriptions: Record<number, string> = $state({})
  let descriptionError: string | null = $state(null)
  let removing: string | null = $state(null)
  let toggling: string | null = $state(null)
  let menu: { x: number; y: number; label: string; items: MenuItem[] } | null = $state(null)

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
      const installed = await api.installedMods()
      mods = installed.mods
      modUpdates.locked = new Set(installed.locked)
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
      .then((config) => (modUpdates.channel = config.updates.channel))
      .catch(() => {})
    await load()
    const unchecked = mods.some((m) => !modUpdates.checks?.has(m.path))
    if (mods.length && (unchecked || Date.now() - modUpdates.checkedAt > CHECK_MAX_AGE_MS)) await check()
  })

  onJobEnd('mods', async (phase) => {
    finishedHere = true
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

  async function setEnabled(mod: InstalledMod, enabled: boolean) {
    toggling = mod.path
    actionError = null
    try {
      const path = await api.setModEnabled(mod.path, enabled)
      // The check is keyed by path, so it follows the file to its new name.
      const status = modUpdates.checks?.get(mod.path)
      if (modUpdates.checks && status) modUpdates.checks = new Map([...modUpdates.checks].map(([key, value]) => (key === mod.path ? [path, { ...status, path }] : [key, value])))
      if (expanded === mod.path) expanded = path
      mods = mods.map((m) => (m.path === mod.path ? { ...m, path } : m))
      serverMods.changedSinceBuild = true
      serverMods.changedSinceTest = true
    } catch (error) {
      actionError = `Couldn't ${enabled ? 'enable' : 'disable'} ${mod.name}: ${errorText(error)}`
    } finally {
      toggling = null
    }
  }

  // The button you pressed is disabled or gone once a change starts, so we keep keyboard focus
  // on the list instead of letting it fall back to the page.
  function keepFocus(region: HTMLElement | undefined) {
    if (region && !region.contains(document.activeElement) && document.activeElement !== document.body) return
    region?.focus()
  }

  async function update(requests: ModRequest[]) {
    actionError = null
    finishedHere = false
    try {
      await api.changeMods(requests)
      if (browse.tab === 'installed') tableRegion?.focus()
    } catch (error) {
      actionError = `Couldn't start the update: ${errorText(error)}`
    }
  }

  async function updateAll() {
    const changes = mods.filter((m) => updates.has(m.path)).map((m) => `${m.name}: ${updates.get(m.path)!.fileName}`)
    const shown = changes.slice(0, 12)
    const more = changes.length - shown.length
    const message = [
      `packraven downloads ${changes.length === 1 ? 'this update' : `these ${changes.length} updates`}, replaces the old files, and installs any mods they newly require.`,
      shown.join('\n') + (more ? `\nand ${more} more` : ''),
    ].join('\n\n')
    const confirmed = await ask(message, { title: 'Update all', kind: 'info', okLabel: changes.length === 1 ? 'Update 1 mod' : `Update ${changes.length} mods`, cancelLabel: 'Cancel' })
    if (confirmed) await update([...updates].map(([path, file]) => ({ fileId: file.fileId, replaces: path })))
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
      keepFocus(tableRegion)
    } catch (error) {
      actionError = `Couldn't remove ${mod.name}: ${errorText(error)}`
    } finally {
      removing = null
    }
  }

  async function expand(mod: InstalledMod, show: Detail = detail) {
    if (expanded === mod.path && show === detail) {
      expanded = null
      return
    }
    const opening = expanded !== mod.path
    expanded = mod.path
    detail = show
    if (opening) {
      versions = null
      versionsError = null
      descriptionError = null
      api.modVersions(mod.path, mod.projectId).then(
        (files) => expanded === mod.path && (versions = files),
        (error) => expanded === mod.path && (versionsError = `Couldn't list the versions on CurseForge: ${errorText(error)}`),
      )
    }
    if (show === 'description' && descriptions[mod.projectId] === undefined) {
      try {
        descriptions[mod.projectId] = sanitizeRemoteHtml(await api.modDescription(mod.projectId))
      } catch (error) {
        if (expanded === mod.path) descriptionError = `Couldn't load the description from CurseForge: ${errorText(error)}`
      }
    }
  }

  // The whole row opens its details, except the controls that have a job of their own.
  function onRowClick(event: MouseEvent, mod: InstalledMod) {
    if ((event.target as HTMLElement).closest('button, a, input, select')) return
    expand(mod)
  }

  function onDescriptionClick(event: MouseEvent) {
    const link = (event.target as HTMLElement).closest('a')
    if (!link) return
    event.preventDefault()
    const external = link.getAttribute('data-external')
    if (external) openUrl(external)
  }

  function openMenu(event: MouseEvent, mod: InstalledMod) {
    event.preventDefault()
    const row = event.currentTarget as HTMLElement
    const rect = row.getBoundingClientRect()
    const keyboard = !event.clientX && !event.clientY
    const lockedHere = modUpdates.locked.has(mod.projectId)
    const disabled = mod.path.endsWith('.disabled')
    menu = {
      x: keyboard ? rect.left + 24 : event.clientX,
      y: keyboard ? rect.bottom : event.clientY,
      label: `${mod.name} actions`,
      items: [
        { label: 'Show versions', run: () => expand(mod, 'versions') },
        { label: 'Show description', run: () => expand(mod, 'description') },
        'separator',
        { label: 'Open on CurseForge', disabled: !mod.url, run: () => mod.url && openUrl(mod.url) },
        { label: 'Copy link', disabled: !mod.url, run: () => mod.url && navigator.clipboard.writeText(mod.url) },
        'separator',
        { label: lockedHere ? 'Unlock' : 'Lock to this version', run: () => setLocked(mod, !lockedHere) },
        ...(isMod(mod) ? [{ label: disabled ? 'Enable' : 'Disable', disabled: blocked, run: () => setEnabled(mod, disabled) }] : []),
        'separator',
        { label: `Move to ${trashName}`, danger: true, disabled: blocked, run: () => remove(mod) },
      ],
    }
  }

  function list(names: string[]) {
    return names.length < 3 ? names.join(' and ') : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`
  }

  function fileName(path: string) {
    return path.slice(path.lastIndexOf('/') + 1)
  }

  function folder(path: string) {
    return path.slice(0, path.indexOf('/')) as Kind
  }

  function isMod(mod: InstalledMod) {
    return folder(mod.path) === 'mods'
  }

  function date(iso: string | null) {
    return iso ? new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' }) : ''
  }

  const typeLabels: Record<ReleaseType, string> = { release: 'Release', beta: 'Beta', alpha: 'Alpha' }
  const kindLabels: Record<Kind, string> = { mods: 'Mods', resourcepacks: 'Resource packs', shaderpacks: 'Shaders', datapacks: 'Data packs' }

  let updates = $derived(new Map(mods.map((m) => [m.path, availableUpdate(modUpdates.checks?.get(m.path))] as const).filter(([, file]) => file)) as Map<string, ModFile>)
  let duplicates = $derived.by(() => {
    const byProject = new Map<number, InstalledMod[]>()
    for (const mod of mods.filter((m) => isMod(m) && !m.path.endsWith('.disabled'))) byProject.set(mod.projectId, [...(byProject.get(mod.projectId) ?? []), mod])
    return [...byProject.values()].filter((group) => group.length > 1)
  })
  let duplicatePaths = $derived(new Set(duplicates.flat().map((m) => m.path)))
  let kindsWithUpdates = $derived(new Set([...updates.keys()].map(folder)))
  let kindCounts = $derived(mods.reduce((counts, m) => counts.set(folder(m.path), (counts.get(folder(m.path)) ?? 0) + 1), new Map<Kind, number>()))

  // Each column sorts the interesting rows first: updates, locked or disabled mods, and A to Z.
  function compare(a: InstalledMod, b: InstalledMod): number {
    const byName = a.name.localeCompare(b.name)
    const flag = (value: (m: InstalledMod) => boolean) => Number(value(b)) - Number(value(a)) || byName
    switch (sort.key) {
      case 'update':
        return flag((m) => updates.has(m.path))
      case 'locked':
        return flag((m) => modUpdates.locked.has(m.projectId))
      case 'enabled':
        return flag((m) => m.path.endsWith('.disabled'))
      case 'author':
        return (a.authors[0] ?? '￿').localeCompare(b.authors[0] ?? '￿') || byName
      default:
        return byName
    }
  }

  let visible = $derived(
    mods
      .filter((m) => {
        if (folder(m.path) !== kindFilter) return false
        const query = search.toLowerCase()
        return !query || m.name.toLowerCase().includes(query) || m.path.toLowerCase().includes(query) || m.authors.some((a) => a.toLowerCase().includes(query))
      })
      .sort((a, b) => (sort.reversed ? -compare(a, b) : compare(a, b))),
  )

  function sortBy(key: SortKey) {
    sort = { key, reversed: sort.key === key ? !sort.reversed : false }
  }

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
  let tableRegion: HTMLElement | undefined = $state()
  let browser: ModBrowser | undefined = $state()
  // A run that finished before you opened the view already shows in the list, so we only
  // report the ones that end while you're here.
  let finishedHere = $state(false)

  const tabs = ['installed', 'browse'] as const

  function onTabKey(event: KeyboardEvent) {
    const index = tabs.indexOf(browse.tab)
    const next = { ArrowRight: index + 1, ArrowLeft: index - 1, Home: 0, End: tabs.length - 1 }[event.key]
    if (next === undefined) return
    event.preventDefault()
    browse.tab = tabs[(next + tabs.length) % tabs.length]
    document.getElementById(`mods-tab-${browse.tab}`)?.focus()
  }

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

{#snippet sortHeader(key: SortKey, label: string, hidden = false)}
  <th aria-sort={sort.key === key ? (sort.reversed ? 'descending' : 'ascending') : undefined} class={key}>
    <button class="sort" class:sorted={sort.key === key} onclick={() => sortBy(key)}>
      {#if hidden}<span class="visually-hidden">{label}</span>{:else}{label}{/if}
      <svg viewBox="0 0 10 10" aria-hidden="true"><path d={sort.key === key && sort.reversed ? 'M2 6.5 5 3.5 8 6.5' : 'M2 3.5 5 6.5 8 3.5'} /></svg>
    </button>
  </th>
{/snippet}

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
        <button class="primary" class:busy={updating} disabled={!updates.size || blocked} onclick={updateAll}>
          {updating ? 'Updating' : updates.size ? `Update all (${updates.size})` : 'Update all'}
        </button>
      </div>
    {/if}
  </header>

  <div class="tabs" role="tablist" aria-label="Mods" tabindex="-1" onkeydown={onTabKey}>
    {#each [['installed', 'Installed'], ['browse', 'Browse']] as const as [tab, label] (tab)}
      <button role="tab" id="mods-tab-{tab}" aria-selected={browse.tab === tab} tabindex={browse.tab === tab ? 0 : -1} aria-controls="mods-panel" class:on={browse.tab === tab} onclick={() => (browse.tab = tab)}>
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
  {#if finishedHere && job('mods').phase === 'done' && job('mods').message}
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
          {#each Object.keys(kindLabels) as Kind[] as kind (kind)}
            <button aria-pressed={kindFilter === kind} class:on={kindFilter === kind} onclick={() => (kindFilter = kind)}>
              {kindLabels[kind]}<span class="muted filter-count">{kindCounts.get(kind) ?? 0}</span>
              {#if kindsWithUpdates.has(kind)}<span class="has-updates" title="Has updates"><span class="visually-hidden">, has updates</span></span>{/if}
            </button>
          {/each}
        </div>
        <label class="channel">
          Update to
          <select value={modUpdates.channel} onchange={(e) => setChannel(e.currentTarget)}>
            <option value="alpha">Any release type</option>
            <option value="beta">Releases and betas</option>
            <option value="release">Releases only</option>
          </select>
        </label>
        <input type="search" bind:this={searchField} bind:value={search} placeholder="Find a mod or author" aria-label="Find a mod or author" title="Find a mod or author ({shortcut('Ctrl+F')})" />
      </div>

      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div class="table" role="region" aria-label="Installed mods" tabindex="0" bind:this={tableRegion}>
        <table aria-busy={loading && !mods.length}>
          <thead>
            <tr>
              {@render sortHeader('enabled', 'Enabled', true)}
              {@render sortHeader('name', 'Mod')}
              {@render sortHeader('author', 'Author')}
              {@render sortHeader('update', 'Update')}
              {@render sortHeader('locked', 'Locked', true)}
              <th class="trash"><span class="visually-hidden">Remove</span></th>
            </tr>
          </thead>
          <tbody>
            {#if loading && !mods.length}
              {#each { length: 8 } as _, row (row)}
                <tr class="skeleton" aria-hidden="true">
                  <td></td>
                  <td><span style="width: {55 + ((row * 37) % 35)}%"></span><span class="short"></span></td>
                  <td><span style="width: {40 + ((row * 23) % 30)}%"></span></td>
                  <td></td>
                  <td></td>
                  <td></td>
                </tr>
              {/each}
            {:else}
              {#each visible as mod (mod.path)}
                {@const status = checkFor(mod)}
                {@const next = updates.get(mod.path)}
                {@const lockedHere = modUpdates.locked.has(mod.projectId)}
                {@const held = newestOnChannel(status)}
                {@const disabled = mod.path.endsWith('.disabled')}
                {@const logo = mod.logo ?? status?.logo}
                <tr class="row" class:open={expanded === mod.path} class:off={disabled} onclick={(e) => onRowClick(e, mod)} oncontextmenu={(e) => openMenu(e, mod)}>
                  <td class="enabled">
                    {#if isMod(mod)}
                      <button
                        class="switch"
                        role="switch"
                        aria-checked={!disabled}
                        aria-label="Enable {mod.name}"
                        title={disabled ? 'Disabled. packraven leaves it out of exports.' : 'Enabled'}
                        disabled={blocked || toggling === mod.path}
                        onclick={() => setEnabled(mod, disabled)}
                      ></button>
                    {/if}
                  </td>
                  <td class="name">
                    <div class="identity">
                      {#if logo}<img src={logo} alt="" width="32" height="32" loading="lazy" />{:else}<span class="logo" aria-hidden="true"></span>{/if}
                      <div class="about">
                        <div class="title-line">
                          <button class="expand" aria-expanded={expanded === mod.path} aria-controls="details-{mod.projectId}" onclick={() => expand(mod)}>{mod.name}</button>
                          {#if duplicatePaths.has(mod.path)}<span class="chip fail">Two versions</span>{/if}
                        </div>
                        <div class="version">
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
                      </div>
                    </div>
                  </td>
                  <td class="author muted">{mod.authors.join(', ')}</td>
                  <td class="update">
                    {#if next}
                      <button onclick={() => update([{ fileId: next.fileId, replaces: mod.path }])} disabled={blocked} aria-label="Update {mod.name}">Update</button>
                    {/if}
                  </td>
                  <td class="locked">
                    <button
                      class="ghost icon lock"
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
                  </td>
                  <td class="trash">
                    <button class="ghost icon remove" onclick={() => remove(mod)} disabled={blocked || removing === mod.path} aria-label="Remove {mod.name}" title="Move to the {trashName}">
                      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.5h5.8l.6-8.5M7 7v4M9 7v4" /></svg>
                    </button>
                  </td>
                </tr>
                {#if expanded === mod.path}
                  <tr class="details" id="details-{mod.projectId}">
                    <td colspan="6">
                      <div class="detail-head">
                        <div class="modes" role="group" aria-label="Show">
                          <button class="mode" aria-pressed={detail === 'versions'} onclick={() => expand(mod, 'versions')}>Versions</button>
                          <button class="mode" aria-pressed={detail === 'description'} onclick={() => expand(mod, 'description')}>Description</button>
                        </div>
                        {#if mod.url}<button class="link" onclick={() => openUrl(mod.url!)}>Open on CurseForge</button>{/if}
                      </div>
                      {#if detail === 'versions'}
                        {#if versionsError}
                          <p class="failed">{versionsError}</p>
                        {:else if !versions}
                          <p class="muted">Listing files on CurseForge.</p>
                        {:else if !versions.length}
                          <p class="muted">CurseForge has no files of {mod.name} for {game}.</p>
                        {:else}
                          <ul class="files" aria-label="Versions of {mod.name}">
                            {#each versions as file (file.fileId)}
                              <li>
                                {@render fileText(file.fileName)}
                                <span class="chip {file.releaseType === 'release' ? 'ok' : 'pending'}">{typeLabels[file.releaseType]}</span>
                                <span class="muted">{date(file.fileDate)}</span>
                                {#if file.fileId === mod.fileId}
                                  <span class="muted current">Installed</span>
                                {:else}
                                  <button onclick={() => update([{ fileId: file.fileId, replaces: mod.path }])} disabled={blocked} aria-label="Use {file.fileName}">Use this</button>
                                {/if}
                              </li>
                            {/each}
                          </ul>
                        {/if}
                      {:else if descriptionError}
                        <p class="failed">{descriptionError}</p>
                      {:else if descriptions[mod.projectId] === undefined}
                        <p class="muted">Loading the description from CurseForge.</p>
                      {:else}
                        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (links inside are real links; this only routes their clicks) -->
                        <div class="description" onclick={onDescriptionClick}>
                          {@html descriptions[mod.projectId]}
                        </div>
                      {/if}
                    </td>
                  </tr>
                {/if}
              {:else}
                <tr>
                  <td colspan="6" class="muted">
                    {#if search}
                      No mods match "{search}". <button class="link" onclick={() => (search = '')}>Clear the search</button>
                    {:else}
                      This pack has no {kindLabels[kindFilter].toLowerCase()} from CurseForge yet.
                      <button class="link" onclick={() => { browse.kind = kindFilter; browse.tab = 'browse' }}>Browse CurseForge</button>
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

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} label={menu.label} items={menu.items} onclose={() => (menu = null)} />
{/if}

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

  .tab-count,
  .filter-count {
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
    display: flex;
    flex-wrap: wrap;
    max-width: 100%;
    padding: 3px;
    gap: 3px;
    background: var(--paper);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
  }

  .filters button {
    font-weight: 400;
    padding: 5px clamp(8px, 1.5vw, 14px);
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

  .has-updates {
    display: inline-block;
    width: 7px;
    height: 7px;
    margin-left: 6px;
    vertical-align: 0.15em;
    border-radius: 50%;
    background: var(--ochre-ink);
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
    flex: 0 1 240px;
    min-width: 0;
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
    z-index: 1;
    text-align: left;
    font-size: var(--step--1);
    color: var(--muted);
    font-weight: 400;
    background: var(--surface);
    padding: 4px 8px;
    white-space: nowrap;
  }

  th.enabled,
  th.locked,
  th.trash {
    width: 1%;
  }

  th.update {
    text-align: right;
  }

  .sort {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px;
    font: inherit;
    color: inherit;
    background: transparent;
    border: 0;
  }

  .sort:hover:not(:disabled) {
    color: var(--text);
    background: transparent;
  }

  .sort.sorted {
    color: var(--text);
    font-weight: 700;
  }

  .sort svg {
    width: 10px;
    height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
    opacity: 0;
  }

  .sort.sorted svg,
  .sort:hover svg,
  .sort:focus-visible svg {
    opacity: 1;
  }

  /* The switch and padlock columns are too narrow for words, so their headers draw the control. */
  th.enabled .sort::before {
    content: '';
    width: 20px;
    height: 11px;
    border: 1.5px solid currentColor;
    border-radius: 999px;
  }

  th.locked .sort::before {
    content: '';
    width: 13px;
    height: 13px;
    background: currentColor;
    mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16' fill='none' stroke='black' stroke-width='1.5'%3E%3Crect x='3' y='7' width='10' height='7' rx='1.5'/%3E%3Cpath d='M5.5 7V5a2.5 2.5 0 0 1 5 0v2'/%3E%3C/svg%3E") center / contain no-repeat;
  }

  td {
    padding: 8px;
    border-top: 1px solid var(--line);
    vertical-align: middle;
  }

  .row {
    cursor: pointer;
  }

  .row:hover td,
  tr.open td {
    background: var(--paper);
  }

  .row.off .identity img,
  .row.off .logo {
    opacity: 0.5;
  }

  .row.off .expand {
    color: var(--muted);
    font-weight: 400;
  }

  .row.off .next {
    color: var(--muted);
  }

  .identity {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .identity img,
  .logo {
    flex: none;
    width: 32px;
    height: 32px;
    border-radius: var(--radius-small);
    object-fit: cover;
  }

  .logo {
    background: var(--line);
  }

  .about {
    display: grid;
    gap: 1px;
    min-width: 0;
  }

  .title-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px 8px;
  }

  .expand {
    padding: 0;
    font: inherit;
    font-weight: 700;
    color: var(--text);
    text-align: left;
    background: transparent;
    border: 0;
  }

  .expand:hover:not(:disabled) {
    background: transparent;
    text-decoration: underline;
    text-underline-offset: 0.18em;
  }

  .version {
    color: var(--muted);
  }

  td .file {
    margin-right: 6px;
  }

  .held,
  .next {
    font-size: var(--step--1);
  }

  .held .file {
    margin-right: 0;
  }

  .next {
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

  .author {
    font-size: var(--step--1);
    min-width: 9ch;
  }

  .update {
    text-align: right;
    white-space: nowrap;
  }

  button.icon {
    display: grid;
    place-items: center;
    padding: 6px;
  }

  button.icon svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.25;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .lock[aria-pressed='false'],
  .remove {
    color: var(--muted);
  }

  .row:hover .lock[aria-pressed='false'],
  .lock[aria-pressed='false']:focus-visible {
    color: var(--text);
  }

  .remove:hover:not(:disabled),
  .remove:focus-visible {
    color: var(--fail);
  }

  .lock[aria-pressed='true'] {
    color: var(--ochre-ink);
  }

  .lock[aria-pressed='true'] rect {
    fill: currentColor;
  }

  .switch {
    position: relative;
    display: block;
    width: 30px;
    height: 18px;
    padding: 0;
    border-radius: 999px;
    border: 1px solid var(--control-edge);
    background: var(--line-strong);
  }

  .switch::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--field);
    transition: transform 120ms ease-out;
  }

  .switch[aria-checked='true'] {
    border-color: var(--forest);
    background: var(--forest);
  }

  .switch[aria-checked='true']::after {
    transform: translateX(12px);
  }

  .switch:hover:not(:disabled) {
    border-color: var(--forest-hi);
  }

  .switch[aria-checked='true']:hover:not(:disabled) {
    background: var(--forest-hi);
  }

  .details td {
    border-top: 0;
    padding: 4px 12px 14px 58px;
    background: var(--paper);
    cursor: auto;
  }

  .detail-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 16px;
    margin-bottom: 8px;
  }

  .modes {
    width: fit-content;
  }

  .files {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 2px;
    max-height: 280px;
    overflow: auto;
  }

  .files li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 10px;
    padding: 4px 0;
  }

  .files li button,
  .files .current {
    margin-left: auto;
  }

  .details p {
    margin: 4px 0;
  }

  .description {
    max-height: 360px;
    max-width: 80ch;
    overflow: auto;
    overflow-wrap: anywhere;
  }

  .description :global(img) {
    max-width: 100%;
    height: auto;
  }

  .description :global(h1),
  .description :global(h2),
  .description :global(h3) {
    font-size: var(--step-1);
    margin: 14px 0 6px;
  }

  .description :global(p) {
    margin: 0 0 8px;
  }

  .description :global(a) {
    color: var(--forest);
  }

  .description :global(.center),
  .description :global(.aligncenter) {
    text-align: center;
  }

  .description :global(.alignleft) {
    float: left;
    margin-right: 12px;
  }

  .description :global(.alignright) {
    float: right;
    margin-left: 12px;
  }
</style>
