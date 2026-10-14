<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import { api, errorText, type Overview } from '../lib/api'
  import { job, serverMods, testState } from '../lib/jobs.svelte'
  import JobLog from '../components/JobLog.svelte'
  import Tip from '../components/Tip.svelte'

  let { overview, onservermods }: { overview: Overview | null; onservermods: () => void } = $props()

  let rebuild = $state(serverMods.changedSinceBuild)
  let join = $state(true)
  let clientJoins = $state(false)
  let command = $state('')
  let error: string | null = $state(null)

  function startTest(connect: boolean) {
    return run(async () => {
      await api.startTest(rebuild, connect)
      if (rebuild) serverMods.changedSinceBuild = false
    })
  }

  async function run(action: () => Promise<void>) {
    try {
      error = null
      await action()
    } catch (e) {
      error = errorText(e)
    }
  }

  async function sendCommand() {
    const line = command.trim()
    if (!line) return
    await run(() => api.serverCommand(line))
    command = ''
  }

  let serverRunning = $derived(overview?.serverRunning ?? false)
  let clientRunning = $derived(overview?.clientRunning ?? false)
  let preparing = $derived(job('test').phase === 'running')
  let serverLines = $derived(job('test').lines.concat(job('server').lines))
  let status = $derived(
    testState.players.length
      ? `${testState.players.join(', ')} joined the server.`
      : clientRunning
        ? clientJoins
          ? 'The client is launching and will join when it reaches the title screen.'
          : 'The client is running.'
        : testState.serverReady
          ? 'The server is up.'
          : serverRunning
            ? 'The server is starting.'
            : preparing
              ? 'Preparing the test server.'
              : 'Start a test server or client locally in offline mode.',
  )

  let serverStarting = $derived(preparing || (serverRunning && !testState.serverReady))
  let whyNoStart = $derived(serverRunning ? 'The test server is already running' : preparing ? 'packraven is preparing the test server' : null)
  let whyNoStop = $derived(serverRunning ? null : preparing ? "packraven is preparing the test server; you can stop it once it's running" : "The test server isn't running")
  let whyNoQuick = $derived(
    serverRunning ? 'The test server is already running. Use Launch client to join it' : clientRunning ? 'The client is already running' : preparing ? 'packraven is preparing the test server' : null,
  )
  let whyNoLaunch = $derived(
    clientRunning
      ? 'The client is already running'
      : join && !testState.serverReady
        ? serverStarting
          ? 'Waiting for the test server to finish starting, or untick auto-join'
          : 'Start the test server first, or untick auto-join'
        : null,
  )
  let whyNoSend = $derived(!serverRunning ? "The test server isn't running" : !command.trim() ? 'Type a command first' : null)
</script>

<div class="test">
  <div class="head">
    <div>
      <h2>Test run</h2>
      <p class="muted">{status}</p>
    </div>
    <div class="controls">
      <label class="check" title={serverRunning ? 'Stop the test server to change this' : undefined}>
        <input type="checkbox" bind:checked={rebuild} disabled={serverRunning} /> Rebuild server pack first
      </label>
      <Tip tip={whyNoQuick ?? 'Start the server, then launch the client and join it'}>
        <button
          class="primary"
          class:busy={preparing}
          disabled={!!whyNoQuick}
          onclick={() => ((clientJoins = true), startTest(true))}
        >
          {preparing ? 'Starting' : 'Quick test'}
        </button>
      </Tip>
    </div>
  </div>

  {#if serverMods.changedSinceBuild}
    <Notice tone="warn">
      <p>
        You changed server mod overrides since packraven last built the server pack.
        {serverRunning
          ? 'Stop the test server, then start it again with Rebuild server pack first ticked to use them.'
          : rebuild
            ? 'Rebuild server pack first is ticked, so the next start uses them.'
            : 'Tick Rebuild server pack first so the next start uses them.'}
      </p>
    </Notice>
  {/if}
  {#if error}<Notice tone="error"><p>{error}</p></Notice>{/if}
  {#if job('test').phase === 'failed'}<Notice tone="error"><p>{job('test').message}</p></Notice>{/if}

  <div class="logs">
    <div class="pane">
      <div class="pane-head">
        <h3>Server</h3>
        <div class="buttons">
          <Tip tip={whyNoStart}>
            <button disabled={!!whyNoStart} onclick={() => startTest(false)}>Start server</button>
          </Tip>
          <Tip tip={whyNoStop ?? 'Save the world and shut the server down'}>
            <button disabled={!!whyNoStop} onclick={() => run(() => api.serverCommand('stop'))}>Stop</button>
          </Tip>
          <Tip tip={whyNoStop ?? 'End the process at once without saving'}>
            <button class="danger" disabled={!!whyNoStop} onclick={() => run(() => api.killProcess('server'))}>Kill</button>
          </Tip>
        </div>
      </div>
      <JobLog lines={serverLines} label="Server log" empty="Start the server to see its log." />
      <form class="console" onsubmit={(e) => { e.preventDefault(); sendCommand() }}>
        <input bind:value={command} placeholder="op Dev" disabled={!serverRunning} aria-label="Server command" aria-describedby="console-hint" />
        <Tip tip={whyNoSend}><button type="submit" disabled={!!whyNoSend}>Send</button></Tip>
      </form>
      <p class="hint muted" id="console-hint">
        {serverRunning ? 'Send commands to the server console. Use Stop to save and shut down, or Kill to end the process without saving.' : 'You can send commands once the server is running.'}
        Server crashing on a mod? <button class="link" onclick={onservermods}>Change server mods</button>.
      </p>
    </div>

    <div class="pane">
      <div class="pane-head">
        <h3>Client</h3>
        <div class="buttons">
          <label class="check" title={clientRunning ? "Can't change this while the client is running" : undefined}>
            <input type="checkbox" bind:checked={join} disabled={clientRunning} /> Auto-join test server
          </label>
          <Tip tip={whyNoLaunch}>
            <button disabled={!!whyNoLaunch} onclick={() => run(() => ((clientJoins = join), api.startClient(join)))}>Launch client</button>
          </Tip>
          <Tip tip={clientRunning ? 'End the client process at once' : "The client isn't running"}>
            <button class="danger" disabled={!clientRunning} onclick={() => run(() => api.killProcess('client'))}>Kill</button>
          </Tip>
        </div>
      </div>
      <JobLog lines={job('client').lines} label="Client log" empty="The client log will appear here after you launch it." />
      <p class="hint muted">
        {clientRunning
          ? 'Running offline as the test username.'
          : !join
            ? 'Launch this instance offline to the title screen.'
            : testState.serverReady
              ? 'Launch this instance offline and join the test server.'
              : 'Start the server first, or untick auto-join to launch to the title screen.'}
      </p>
    </div>
  </div>
</div>

<style>
  .test {
    display: flex;
    flex-direction: column;
    gap: 18px;
    height: 100%;
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: start;
    gap: 24px;
  }

  .head p {
    margin-top: 4px;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 18px;
    flex-wrap: wrap;
    justify-content: end;
  }

  .logs {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 20px;
  }

  @media (max-width: 900px) {
    .logs {
      grid-template-columns: minmax(0, 1fr);
      grid-auto-rows: minmax(280px, 1fr);
    }
  }

  .pane {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
  }

  .pane-head,
  .buttons,
  .console {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .buttons button {
    white-space: nowrap;
  }

  .hint {
    font-size: var(--step--1);
  }

  .link {
    display: inline;
    padding: 0;
    font: inherit;
    color: var(--forest);
    text-decoration: underline;
    text-underline-offset: 0.18em;
    background: transparent;
    border: 0;
  }

  .link:hover:not(:disabled) {
    color: var(--forest-hi);
    background: transparent;
  }

  .console input {
    flex: 1;
    font-family: var(--font-log);
    font-size: var(--step--1);
  }
</style>
