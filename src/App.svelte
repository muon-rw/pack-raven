<script lang="ts">
  import Notice from './components/Notice.svelte'
  import { onMount } from 'svelte'
  import { api, errorText, type Overview, type PackList } from './lib/api'
  import { job, jobs, startListening, testState } from './lib/jobs.svelte'
  import Prepare from './views/Prepare.svelte'
  import ServerPack from './views/ServerPack.svelte'
  import TestRun from './views/TestRun.svelte'
  import Publish from './views/Publish.svelte'
  import SettingsView from './views/Settings.svelte'
  import AddPack from './views/AddPack.svelte'
  import Git from './views/Git.svelte'
  const loadFiles = () => import('./views/Files.svelte')
  import { dirtyCount, startWatching } from './lib/workspace.svelte'
  import DefenderNotice from './components/DefenderNotice.svelte'
  import { ask } from '@tauri-apps/plugin-dialog'
  import PackSwitcher from './components/PackSwitcher.svelte'
  import RavenMark from './components/RavenMark.svelte'
  import ThemeToggle from './components/ThemeToggle.svelte'
  import { modSyncSummary } from './lib/modsync'

  type View = 'prepare' | 'server' | 'test' | 'publish' | 'settings' | 'add' | 'git' | 'files'
  type Tone = 'ok' | 'warn' | 'bad' | 'busy' | 'idle'

  let view: View = $state('prepare')
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
    return () => clearInterval(timer)
  })

  $effect(() => {
    for (const name in jobs) void jobs[name].phase
    refresh()
  })

  async function switchPack(dir: string) {
    const unsaved = dirtyCount()
    if (unsaved && dir !== packs?.active) {
      const confirmed = await ask(`${unsaved === 1 ? 'A file has' : `${unsaved} files have`} unsaved changes. Switching packs closes ${unsaved === 1 ? 'it' : 'them'} without saving.`, {
        title: 'Unsaved changes',
        kind: 'warning',
        okLabel: 'Switch anyway',
        cancelLabel: 'Keep editing',
      })
      if (!confirmed) return
    }
    try {
      await api.switchPack(dir)
      view = 'prepare'
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

  function jobTone(name: 'server-pack' | 'publish'): Tone {
    const phase = job(name).phase
    return phase === 'running' ? 'busy' : phase === 'failed' ? 'bad' : phase === 'done' ? 'ok' : 'idle'
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

  let stages = $derived<{ id: View; title: string; detail: string; tone: Tone }[]>([
    {
      id: 'prepare',
      title: 'Prepare',
      detail: !overview ? 'Instance not found' : modSyncSummary(overview),
      tone: !overview ? 'bad' : outOfSync ? 'warn' : 'ok',
    },
    {
      id: 'server',
      title: 'Server pack',
      detail: { busy: 'Building', bad: 'Build failed', ok: 'Built', warn: '', idle: 'Not built this session' }[jobTone('server-pack')],
      tone: jobTone('server-pack'),
    },
    {
      id: 'test',
      title: 'Test',
      detail: testState.players.length
        ? `${testState.players.join(', ')} joined`
        : testState.serverReady
          ? 'Server up'
          : overview?.serverRunning
            ? 'Server starting'
            : 'Not running',
      tone: testState.players.length || testState.serverReady ? 'ok' : overview?.serverRunning ? 'busy' : 'idle',
    },
    {
      id: 'publish',
      title: 'Publish',
      detail: job('publish').phase === 'running' ? 'Uploading' : overview?.lastTag ? `Last release ${overview.lastTag}` : 'Never released',
      tone: jobTone('publish'),
    },
  ])

  function onKey(event: KeyboardEvent) {
    if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey) return
    const stage = packs?.active ? stages[Number(event.key) - 1] : undefined
    if (stage) view = stage.id
    else if (event.key === ',') view = 'settings'
    else if (event.key.toLowerCase() === 'g' && packs?.active) view = 'git'
    else if (event.key.toLowerCase() === 'e' && packs?.active) view = 'files'
    else return
    event.preventDefault()
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="shell">
  <aside class="rail">
    <div class="brand"><RavenMark /><span>packraven</span><ThemeToggle /></div>

    <div class="identity">
      <PackSwitcher list={packs} name={overview?.name ?? 'No pack'} onswitch={switchPack} onadd={() => (view = 'add')} />
      {#if overview}
        <div class="version" title="Pack version, stored in the Better Compatibility Checker config">{overview.version ?? 'No version'}</div>
        <div class="game">Minecraft {overview.minecraft} with {overview.loader}</div>
      {/if}
    </div>

    {#if overview}
      <dl class="machine">
        <div>
          <dt>Branch</dt>
          <dd>
            <span class="file">{overview.branch}</span>
            {#if overview.ahead}<span class="pending">{overview.ahead} to push</span>{/if}
            {#if overview.behind}<span class="pending">{overview.behind} to pull</span>{/if}
          </dd>
        </div>
        <div>
          <dt>Uncommitted</dt>
          <dd class:pending={overview.changed.length}>{overview.changed.length ? `${overview.changed.length} files` : 'None'}</dd>
        </div>
        <div class="addons" class:open={addonsOpen}>
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
        <div class="optional">
          <dt>Test server</dt>
          <dd>{overview.serverRunning ? 'Running' : 'Stopped'}</dd>
        </div>
        <div class="optional">
          <dt>Test client</dt>
          <dd>{overview.clientRunning ? 'Running' : 'Stopped'}</dd>
        </div>
      </dl>
    {/if}

    <div class="rail-nav">
      {#if packs?.active}
        <button class="settings" class:current={view === 'files'} onclick={() => (view = 'files')} aria-current={view === 'files' ? 'page' : undefined}>
          Files <kbd>Ctrl E</kbd>
        </button>
        <button class="settings" class:current={view === 'git'} onclick={() => (view = 'git')} aria-current={view === 'git' ? 'page' : undefined}>
          <span>Git{#if overview?.changed.length}<span class="pending count">{overview.changed.length}</span>{/if}</span>
          <kbd>Ctrl G</kbd>
        </button>
      {/if}
      <button class="settings" class:current={view === 'settings'} onclick={() => (view = 'settings')} aria-current={view === 'settings' ? 'page' : undefined}>
        Settings <kbd>Ctrl ,</kbd>
      </button>
    </div>
  </aside>

  <main>
    {#if packs?.active}
      <nav aria-label="Release steps">
        <ol class="route">
          {#each stages as stage, index (stage.id)}
            <li class={stage.tone}>
              <button class:current={view === stage.id} onclick={() => (view = stage.id)} aria-current={view === stage.id ? 'step' : undefined} title="Ctrl+{index + 1}">
                <span class="node" aria-hidden="true"></span>
                <span class="title">{stage.title}</span>
                <span class="detail">{stage.detail}</span>
              </button>
            </li>
          {/each}
        </ol>
      </nav>
    {/if}

    <section class="view">
      {#if view !== 'add'}<DefenderNotice />{/if}
      {#if overview && overview.hooks !== 'current' && view !== 'add' && !hooksDismissed}
        <div class="banner">
          <Notice tone="warn">
            <div class="hook-row">
              <p>{overview.hooks === 'missing' ? "No sync hooks, so mods won't download when you pull." : 'Sync hooks are older than this version of packraven.'}</p>
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
          <AddPack first={!packs?.packs.length} onadded={() => { view = 'prepare'; refresh() }} />
        {:else if loadError && view !== 'settings'}
          <Notice tone="error">
            <p>{loadError}</p>
            <p><button onclick={() => (view = 'settings')}>Open Settings</button></p>
          </Notice>
        {:else if view === 'prepare'}
          <Prepare {overview} onchange={refresh} />
        {:else if view === 'server'}
          <ServerPack />
        {:else if view === 'test'}
          <TestRun {overview} />
        {:else if view === 'files'}
          {#await loadFiles() then { default: Files }}
            <Files pack={packs?.active ?? null} />
          {/await}
        {:else if view === 'git'}
          <Git onchange={refresh} />
        {:else if view === 'publish'}
          <Publish {overview} onchange={refresh} onprepare={() => (view = 'prepare')} />
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
    gap: 28px;
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

  .identity {
    display: grid;
    gap: 6px;
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

  .machine {
    display: grid;
    margin: 0;
    border-top: 1px solid var(--rail-edge);
  }

  .machine div {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: baseline;
    gap: 4px 12px;
    padding: 9px 0;
    border-bottom: 1px solid var(--rail-edge);
  }

  .machine dt {
    color: var(--rail-muted);
    font-size: var(--step--1);
  }

  .machine dd {
    margin: 0;
    display: flex;
    flex-wrap: wrap;
    justify-content: end;
    gap: 2px 8px;
    font-variant-numeric: tabular-nums;
  }

  .machine .addons {
    position: relative;
    isolation: isolate;
  }

  .machine .addons::before {
    content: '';
    position: absolute;
    inset: 3px -8px;
    z-index: -1;
    border-radius: var(--radius-control);
  }

  .machine .addons:hover::before {
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

  .addons:has(.expand:focus-visible) {
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

  .machine .breakdown {
    flex-basis: 100%;
    display: grid;
    gap: 2px;
    margin: 4px 0 0;
    padding-left: 18px;
  }

  .machine .breakdown div {
    padding: 0;
    border: 0;
  }

  .pending {
    color: var(--rail-warn);
  }

  .count {
    margin-left: 8px;
    font-variant-numeric: tabular-nums;
  }

  .rail-nav {
    display: grid;
    gap: 6px;
    margin-top: auto;
  }

  .settings {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-top: auto;
    color: inherit;
    background: transparent;
    border-color: var(--rail-edge);
  }

  .settings:hover:not(:disabled),
  .settings.current {
    background: var(--rail-hi);
  }

  .settings.current {
    border-color: var(--rail-strong);
  }

  .settings kbd {
    border-color: var(--rail-edge);
    color: var(--rail-muted);
  }

  main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .route {
    --node: 14px;
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    margin: 0;
    padding: 0 0 0 calc(var(--view-inset) - 12px);
    list-style: none;
    border-bottom: 1px solid var(--line);
  }

  .route li {
    position: relative;
  }

  .route li:not(:last-child)::after {
    content: '';
    position: absolute;
    top: calc(18px + var(--node) / 2 - 1px);
    left: calc(12px + var(--node) + 6px);
    right: -6px;
    height: 2px;
    background: var(--line-strong);
    pointer-events: none;
  }

  .route li.ok:not(:last-child)::after {
    background: var(--sage);
  }

  .route button {
    display: grid;
    justify-items: start;
    align-content: start;
    gap: 2px;
    width: 100%;
    height: 100%;
    padding: 18px 12px 14px;
    text-align: left;
    background: transparent;
    border: 0;
    border-radius: 0;
  }

  .route button:hover {
    background: color-mix(in srgb, var(--surface) 70%, transparent);
  }

  .route button:focus-visible {
    outline-offset: -2px;
  }

  .node {
    width: var(--node);
    height: var(--node);
    margin-bottom: 8px;
    border-radius: 50%;
    border: 2px solid var(--edge);
    background: var(--paper);
  }

  .ok .node {
    background: var(--sage);
    border-color: var(--sage);
  }

  .warn .node {
    border-color: var(--warn);
    background: color-mix(in srgb, var(--warn) 25%, var(--paper));
  }

  .bad .node {
    background: var(--fail);
    border-color: var(--fail);
  }

  .busy .node {
    border-color: var(--busy);
    background: var(--highlight);
    animation: pulse 1.2s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      background: var(--busy);
    }
  }

  .route .current {
    background: var(--here);
    box-shadow: inset 0 -3px 0 var(--forest);
  }

  .route .current:hover {
    background: var(--here);
  }

  .current .node {
    box-shadow: 0 0 0 3px var(--here), 0 0 0 5px var(--text);
  }

  .title {
    color: var(--muted);
  }

  .current .title {
    color: var(--text);
    font-weight: 800;
  }

  .detail {
    font-weight: 400;
    font-size: var(--step--1);
    color: var(--muted);
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

  @media (max-width: 1100px) {
    .shell {
      grid-template-columns: 220px minmax(0, 1fr);
    }

    .rail {
      padding-inline: 16px;
    }

    .machine .optional {
      display: none;
    }

    main {
      --view-inset: 24px;
    }
  }
</style>
