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
  import PackSwitcher from './components/PackSwitcher.svelte'
  import RavenMark from './components/RavenMark.svelte'

  type View = 'prepare' | 'server' | 'test' | 'publish' | 'settings' | 'add'
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
    refresh()
    const timer = setInterval(refresh, 5000)
    return () => clearInterval(timer)
  })

  $effect(() => {
    for (const name in jobs) void jobs[name].phase
    refresh()
  })

  async function switchPack(dir: string) {
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

  let outOfSync = $derived(overview ? overview.missing.length + overview.unlocked.length : 0)

  let stages = $derived<{ id: View; title: string; detail: string; tone: Tone }[]>([
    {
      id: 'prepare',
      title: 'Prepare',
      detail: !overview ? 'Instance not found' : outOfSync ? `${outOfSync} files out of sync` : 'Lockfile in sync',
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
    else return
    event.preventDefault()
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="shell">
  <aside class="rail">
    <div class="brand"><RavenMark /><span>packraven</span></div>

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
        <div>
          <dt>Locked mods</dt>
          <dd>{overview.lockCount}</dd>
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

    <button class="settings" class:current={view === 'settings'} onclick={() => (view = 'settings')} aria-current={view === 'settings' ? 'page' : undefined}>
      Settings <kbd>Ctrl ,</kbd>
    </button>
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
      {#if overview && overview.hooks !== 'current' && view !== 'add' && !hooksDismissed}
        <div class="banner">
          <Notice tone="warn">
            <p>{overview.hooks === 'missing' ? "This pack has no sync hooks, so mods won't download when you pull." : "This pack's sync hooks are older than this version of packraven."}</p>
            <p class="hook-row">
              <button onclick={updateHooks}>Update hooks</button>
              <button class="quiet" onclick={() => (hooksDismissed = true)}>Not now</button>
              {#if hookMessage}<span class:muted={!hookFailed} class:failed={hookFailed}>{hookMessage}</span>{/if}
            </p>
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
    color: var(--rail-ash);
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
    color: var(--teal);
    overflow-wrap: anywhere;
  }

  .game {
    color: var(--rail-ash);
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
    color: var(--rail-ash);
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

  .pending {
    color: var(--amber);
  }

  .settings {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-top: auto;
    background: transparent;
    border-color: var(--rail-edge);
  }

  .settings:hover:not(:disabled),
  .settings.current {
    background: var(--rail-hi);
  }

  .settings.current {
    border-color: var(--violet);
  }

  .settings kbd {
    border-color: var(--rail-edge);
    color: var(--rail-ash);
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
    border-bottom: 1px solid var(--quill);
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
    background: var(--quill-hi);
    pointer-events: none;
  }

  .route li.ok:not(:last-child)::after {
    background: var(--teal);
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
    background: color-mix(in srgb, var(--quill) 45%, transparent);
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
    background: var(--ink);
  }

  .ok .node {
    background: var(--teal);
    border-color: var(--teal);
  }

  .warn .node {
    border-color: var(--amber);
    background: color-mix(in srgb, var(--amber) 30%, var(--ink));
  }

  .bad .node {
    background: var(--rose);
    border-color: var(--rose);
  }

  .busy .node {
    border-color: var(--violet);
    animation: pulse 1.2s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      background: var(--violet);
    }
  }

  .current .node {
    box-shadow: 0 0 0 3px var(--ink), 0 0 0 5px var(--bone);
  }

  .title {
    color: var(--ash);
  }

  .current .title {
    color: var(--bone);
    font-weight: 800;
  }

  .detail {
    font-weight: 400;
    font-size: var(--step--1);
    color: var(--ash);
  }

  .banner {
    margin-bottom: 20px;
  }

  .hook-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 10px;
  }

  .quiet {
    background: transparent;
    border-color: transparent;
    color: var(--ash);
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
