<script lang="ts">
  import Notice from './components/Notice.svelte'
  import { onMount } from 'svelte'
  import { listen } from '@tauri-apps/api/event'
  import { api, errorText, type Overview, type PackList } from './lib/api'
  import { job, jobs, serverMods, startListening, testState } from './lib/jobs.svelte'
  import Changes from './views/Changes.svelte'
  import ServerMods from './views/ServerMods.svelte'
  import TestRun from './views/TestRun.svelte'
  import Publish from './views/Publish.svelte'
  import SettingsView from './views/Settings.svelte'
  import AddPack from './views/AddPack.svelte'
  import Git from './views/Git.svelte'
  import Releases from './views/Releases.svelte'
  import PackSettings from './views/PackSettings.svelte'
  const loadFiles = () => import('./views/Files.svelte')
  import { dirtyCount, startWatching } from './lib/workspace.svelte'
  import DefenderNotice from './components/DefenderNotice.svelte'
  import GitNotice from './components/GitNotice.svelte'
  import { ask } from '@tauri-apps/plugin-dialog'
  import PackSwitcher from './components/PackSwitcher.svelte'
  import RavenMark from './components/RavenMark.svelte'
  import ThemeToggle from './components/ThemeToggle.svelte'
  import { modSyncSummary } from './lib/modsync'

  type View = 'changes' | 'test' | 'mods' | 'files' | 'git' | 'publish' | 'releases' | 'pack' | 'settings' | 'add'
  type Tone = 'ok' | 'warn' | 'bad' | 'busy' | 'idle'
  type NavItem = { id: View; title: string; key?: string; status?: string; tone?: Tone; count?: number }

  let view: View = $state('changes')
  let overview = $state<Overview | null>(null)
  let loadError: string | null = $state(null)
  let packs = $state<PackList | null>(null)
  let hookMessage: string | null = $state(null)
  let hookFailed = $state(false)
  let hooksDismissed = $state(false)

  async function refresh() {
    packs = await api.listPacks().catch(() => packs)
    if (packs && !packs.active) {
      overview = null
      loadError = null
      if (view !== 'settings') view = 'add'
      return
    }
    try {
      overview = await api.overview()
      loadError = null
    } catch (error) {
      overview = null
      loadError = errorText(error)
    }
  }

  onMount(() => {
    startListening()
    startWatching()
    refresh()
    const timer = setInterval(refresh, 5000)
    const unlisten = listen<{ kind: string }>('test-event', ({ payload }) => {
      if (payload.kind.endsWith('-started') || payload.kind.endsWith('-stopped')) refresh()
    })
    return () => {
      clearInterval(timer)
      unlisten.then((stop) => stop())
    }
  })

  $effect(() => {
    for (const name in jobs) void jobs[name].phase
    refresh()
  })

  async function switchPack(dir: string) {
    const unsaved = dirtyCount()
    if (unsaved && dir !== packs?.active) {
      const confirmed = await ask(`${unsaved === 1 ? 'A file has' : `${unsaved} files have`} unsaved changes. If you switch packs, packraven closes ${unsaved === 1 ? 'it' : 'them'} without saving.`, {
        title: 'Unsaved changes',
        kind: 'warning',
        okLabel: 'Switch anyway',
        cancelLabel: 'Keep editing',
      })
      if (!confirmed) return
    }
    try {
      await api.switchPack(dir)
      view = 'changes'
      hookMessage = null
      hooksDismissed = false
      await refresh()
    } catch (error) {
      loadError = errorText(error)
    }
  }

  async function updateHooks() {
    try {
      await api.updateHooks()
      hookFailed = false
      hookMessage = 'Hooks updated. Commit the .githooks changes so other machines get them.'
      await refresh()
    } catch (error) {
      hookFailed = true
      hookMessage = `Couldn't update the hooks: ${errorText(error)}`
    }
  }

  let addonsOpen = $state(false)
  let addonRows = $derived(
    overview
      ? ([
          ['Mods', overview.addons.mods],
          ['Resource Packs', overview.addons.resourcePacks],
          ['Shader Packs', overview.addons.shaderPacks],
          ['Data Packs', overview.addons.dataPacks],
          ['Other', overview.addons.other],
        ] as const).filter(([label, count]) => count > 0 || label !== 'Other')
      : [],
  )

  let outOfSync = $derived(overview ? overview.missing.length + overview.unlocked.length : 0)

  let testStatus = $derived(
    testState.players.length
      ? `${testState.players.join(', ')} joined`
      : [
          testState.serverReady ? 'Server up' : overview?.serverRunning ? 'Server starting' : null,
          overview?.clientRunning ? 'Client running' : null,
        ]
          .filter(Boolean)
          .join(', ') || undefined,
  )

  let publishPhase = $derived(job('publish').phase)

  let groups = $derived<{ label?: string; items: NavItem[] }[]>([
    {
      label: 'Work',
      items: [
        {
          id: 'changes',
          title: 'Changes',
          key: '1',
          status: !overview ? 'Instance not found' : outOfSync ? modSyncSummary(overview) : undefined,
          tone: !overview ? 'bad' : outOfSync ? 'warn' : 'idle',
        },
        {
          id: 'test',
          title: 'Test',
          key: '2',
          status: testStatus,
          tone: overview?.serverRunning && !testState.serverReady ? 'busy' : testStatus ? 'ok' : 'idle',
        },
        {
          id: 'mods',
          title: 'Server mods',
          key: '3',
          status: serverMods.changedSinceBuild ? 'Changed since you last built the server pack' : undefined,
          tone: serverMods.changedSinceBuild ? 'warn' : 'idle',
        },
        { id: 'files', title: 'Files', key: 'E' },
        { id: 'git', title: 'Git', key: 'G', count: overview?.changed.length },
      ],
    },
    {
      label: 'Release',
      items: [
        {
          id: 'publish',
          title: 'Publish',
          key: '4',
          status: { running: 'Uploading', failed: 'Publish failed', done: 'Published', idle: undefined }[publishPhase],
          tone: ({ running: 'busy', failed: 'bad', done: 'ok', idle: 'idle' } as const)[publishPhase],
        },
        { id: 'releases', title: 'Releases' },
      ],
    },
    { items: [{ id: 'pack', title: 'Pack settings' }] },
  ])

  $effect(() => {
    if (job('server-pack').phase === 'done' || publishPhase === 'done') serverMods.changedSinceBuild = false
  })

  function onKey(event: KeyboardEvent) {
    if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey) return
    const key = event.key.toUpperCase()
    const item = packs?.active ? groups.flatMap((group) => group.items).find((item) => item.key === key) : undefined
    if (item) view = item.id
    else if (event.key === ',') view = 'settings'
    else return
    event.preventDefault()
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="shell">
  <aside class="rail">
    <div class="brand"><RavenMark /><span>packraven</span><ThemeToggle /></div>

    <section class="pack" aria-label="Current pack">
      <div class="identity">
        <PackSwitcher list={packs} name={overview?.name ?? (packs?.active ? 'Pack unavailable' : 'No pack')} onswitch={switchPack} onadd={() => (view = 'add')} />
        {#if overview}
          <div class="version" title="Pack version, from the Better Compatibility Checker config">{overview.version ?? 'No version'}</div>
          <div class="game">Minecraft {overview.minecraft} with {overview.loader}</div>
        {/if}
      </div>

      {#if overview}
        <dl class="facts">
          <div>
            <dt>Branch</dt>
            <dd>
              <span class="file">{overview.branch}</span>
              {#if overview.ahead}<span class="pending">{overview.ahead} to push</span>{/if}
              {#if overview.behind}<span class="pending">{overview.behind} to pull</span>{/if}
            </dd>
          </div>
          <div class="clickable" class:open={addonsOpen}>
            <dt>
              <button class="expand" aria-expanded={addonsOpen} aria-controls="addon-breakdown" onclick={() => (addonsOpen = !addonsOpen)}>
                Addons
                <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M4 2.5 7.5 6 4 9.5" /></svg>
              </button>
            </dt>
            <dd>{overview.lockCount}</dd>
            {#if addonsOpen}
              <dl class="breakdown" id="addon-breakdown">
                {#each addonRows as [label, count] (label)}
                  <div><dt>{label}</dt><dd>{count}</dd></div>
                {/each}
              </dl>
            {/if}
          </div>
        </dl>
      {/if}

      {#if packs?.active}
        <nav class="pack-nav" aria-label="Pack">
          {#each groups as group, index (index)}
            <ul aria-label={group.label}>
              {#if group.label}<li class="group" aria-hidden="true">{group.label}</li>{/if}
              {#each group.items as item (item.id)}
                <li>
                  <button
                    class="item {item.tone ?? 'idle'}"
                    class:current={view === item.id}
                    aria-current={view === item.id ? 'page' : undefined}
                    aria-keyshortcuts={item.key ? `Control+${item.key}` : undefined}
                    title={item.status}
                    onclick={() => (view = item.id)}
                  >
                    <span class="dot" aria-hidden="true"></span>
                    <span class="label">{item.title}{#if item.status}<span class="visually-hidden">, {item.status}</span>{/if}</span>
                    {#if item.key}<kbd aria-hidden="true">Ctrl {item.key}</kbd>{/if}
                    {#if item.count}
                      <span class="pending count" title="{item.count} uncommitted {item.count === 1 ? 'file' : 'files'}">{item.count}</span>
                    {/if}
                  </button>
                </li>
              {/each}
            </ul>
          {/each}
        </nav>
      {/if}
    </section>

    <button class="item app-settings" class:current={view === 'settings'} aria-current={view === 'settings' ? 'page' : undefined} aria-keyshortcuts="Control+," onclick={() => (view = 'settings')}>
      <span class="label">App settings</span>
      <kbd>Ctrl ,</kbd>
    </button>
  </aside>

  <main>
    <section class="view">
      <GitNotice oninstalled={refresh} />
      {#if view !== 'add'}<DefenderNotice />{/if}
      {#if overview && overview.hooks !== 'current' && view !== 'add' && !hooksDismissed}
        <div class="banner">
          <Notice tone="warn">
            <div class="hook-row">
              <p>{overview.hooks === 'missing' ? "This pack has no sync hooks, so pulling won't download mods." : 'Sync hooks are older than this version of packraven.'}</p>
              <span class="hook-actions">
                <button onclick={updateHooks}>Update hooks</button>
                <button class="quiet" onclick={() => (hooksDismissed = true)}>Not now</button>
              </span>
            </div>
            {#if hookMessage}<p class:muted={!hookFailed} class:failed={hookFailed}>{hookMessage}</p>{/if}
          </Notice>
        </div>
      {/if}
      {#key packs?.active}
        {#if view === 'add'}
          <AddPack first={!packs?.packs.length} onadded={() => { view = 'changes'; refresh() }} />
        {:else if loadError && view !== 'settings'}
          <Notice tone="error">
            <p>{loadError}</p>
            <p><button onclick={() => (view = 'settings')}>Open App settings</button></p>
          </Notice>
        {:else if view === 'changes'}
          <Changes {overview} onchange={refresh} />
        {:else if view === 'mods'}
          <ServerMods />
        {:else if view === 'test'}
          <TestRun {overview} onservermods={() => (view = 'mods')} />
        {:else if view === 'files'}
          {#await loadFiles() then { default: Files }}
            <Files pack={packs?.active ?? null} />
          {/await}
        {:else if view === 'git'}
          <Git onchange={refresh} />
        {:else if view === 'publish'}
          <Publish {overview} onchange={refresh} onchanges={() => (view = 'changes')} onsettings={() => (view = 'pack')} onreleases={() => (view = 'releases')} />
        {:else if view === 'releases'}
          <Releases onsettings={() => (view = 'pack')} />
        {:else if view === 'pack' && packs?.active}
          <PackSettings dir={packs.active} name={overview?.name ?? 'this pack'} onchange={refresh} onremoved={() => { view = 'changes'; refresh() }} />
        {:else}
          <SettingsView onchange={refresh} />
        {/if}
      {/key}
    </section>
  </main>
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 264px minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    height: 100vh;
  }

  .rail {
    display: flex;
    flex-direction: column;
    gap: 18px;
    min-height: 0;
    overflow-y: auto;
    padding: 20px 22px;
    color: var(--rail-text);
    background: var(--rail);
    border-right: 1px solid var(--rail-edge);
    scrollbar-color: var(--rail-edge) transparent;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 800;
    font-size: var(--step--1);
    letter-spacing: 0.02em;
    color: var(--rail-muted);
  }

  .pack {
    display: grid;
    gap: 16px;
    margin-inline: -10px;
    padding: 14px 10px 8px;
    background: var(--rail-pack);
    border: 1px solid var(--rail-edge);
    border-radius: var(--radius-panel);
  }

  .identity {
    display: grid;
    gap: 6px;
    padding-inline: 6px;
  }

  .version {
    font-family: var(--font-log);
    font-variant-numeric: tabular-nums;
    font-size: var(--step-2);
    line-height: 1.1;
    color: var(--rail-strong);
    overflow-wrap: anywhere;
  }

  .game {
    color: var(--rail-muted);
    font-size: var(--step--1);
  }

  .facts {
    display: grid;
    margin: 0 6px;
    border-top: 1px solid var(--rail-edge);
  }

  .facts div {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: baseline;
    gap: 4px 12px;
    padding: 7px 0;
    border-bottom: 1px solid var(--rail-edge);
  }

  .facts dt {
    color: var(--rail-muted);
    font-size: var(--step--1);
  }

  .facts dd {
    margin: 0;
    display: flex;
    flex-wrap: wrap;
    justify-content: end;
    gap: 2px 8px;
    font-variant-numeric: tabular-nums;
  }

  .facts .clickable {
    position: relative;
    isolation: isolate;
  }

  .facts .clickable::before {
    content: '';
    position: absolute;
    inset: 2px -6px;
    z-index: -1;
    border-radius: var(--radius-control);
  }

  .facts .clickable:hover::before {
    background: var(--rail-hi);
  }

  .expand {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0;
    font: inherit;
    color: inherit;
    background: transparent;
    border: 0;
  }

  .expand::after {
    content: '';
    position: absolute;
    inset: 0;
  }

  .expand:hover:not(:disabled) {
    background: transparent;
  }

  .expand:focus-visible {
    outline: none;
  }

  .clickable:has(.expand:focus-visible) {
    outline: 2px solid var(--rail-strong);
    outline-offset: -2px;
  }

  .expand svg {
    width: 12px;
    height: 12px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
    transition: transform 0.15s ease;
  }

  .open .expand svg {
    transform: rotate(90deg);
  }

  .facts .breakdown {
    flex-basis: 100%;
    display: grid;
    gap: 2px;
    margin: 4px 0 0;
    padding-left: 18px;
  }

  .facts .breakdown div {
    padding: 0;
    border: 0;
  }

  .pending {
    color: var(--rail-warn);
  }

  .pack-nav {
    display: grid;
    gap: 10px;
  }

  .pack-nav ul {
    display: grid;
    gap: 1px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .pack-nav ul + ul {
    padding-top: 10px;
    border-top: 1px solid var(--rail-edge);
  }

  .group {
    padding: 0 6px 4px;
    font-size: var(--step--2);
    font-weight: 700;
    color: var(--rail-muted);
  }

  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 4px 6px;
    font-weight: 400;
    text-align: left;
    color: var(--rail-text);
    background: transparent;
    border-color: transparent;
  }

  .item:hover:not(:disabled) {
    background: var(--rail-hi);
  }

  .item.current,
  .item.current:hover:not(:disabled) {
    font-weight: 700;
    background: var(--rail-current);
  }

  .item:focus-visible {
    outline-offset: -2px;
  }

  .item .label {
    flex: 1;
    min-width: 0;
  }

  .item kbd {
    border-color: var(--rail-edge);
    color: var(--rail-muted);
    opacity: 0;
  }

  .item:hover kbd,
  .item:focus-visible kbd {
    opacity: 1;
    transition: opacity 0.12s ease-out;
  }

  .item .count {
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .ok .dot {
    background: var(--rail-ok);
  }

  .warn .dot {
    background: var(--rail-warn);
  }

  .bad .dot {
    background: var(--rail-fail);
  }

  .busy .dot {
    box-shadow: inset 0 0 0 2px var(--rail-muted);
    animation: rail-pulse 1.2s ease-in-out infinite;
  }

  @keyframes rail-pulse {
    50% {
      background: var(--rail-muted);
    }
  }


  .app-settings {
    margin-top: auto;
    color: var(--rail-muted);
  }

  main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .banner {
    margin-bottom: 18px;
  }

  .hook-row {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: 6px 16px;
  }

  .hook-actions {
    display: flex;
    gap: 4px;
  }

  .hook-actions button {
    padding-block: 4px;
  }

  .rail :global(:focus-visible) {
    outline-color: var(--rail-text);
  }

  .quiet {
    background: transparent;
    border-color: transparent;
    color: var(--muted);
  }

  main {
    --view-inset: 32px;
  }

  .view {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 28px var(--view-inset);
  }

  @media (max-height: 720px) {
    .game,
    .group {
      display: none;
    }

    .rail {
      gap: 12px;
      padding-block: 14px;
    }

    .pack {
      gap: 10px;
    }

    .item {
      padding-block: 3px;
    }
  }

  @media (max-width: 1100px) {
    .shell {
      grid-template-columns: 220px minmax(0, 1fr);
    }

    .rail {
      padding-inline: 16px;
    }

    .pack {
      margin-inline: -6px;
    }

    main {
      --view-inset: 24px;
    }
  }
</style>

