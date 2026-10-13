<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import { api, errorText, type Overview } from '../lib/api'
  import { job, testState } from '../lib/jobs.svelte'
  import LogView from '../components/LogView.svelte'
  import Tip from '../components/Tip.svelte'

  let { overview }: { overview: Overview | null } = $props()

  let rebuild = $state(false)
  let join = $state(true)
  let clientJoins = $state(false)
  let command = $state('')
  let error: string | null = $state(null)

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
  let whyNoStart = $derived(serverRunning ? 'The test server is already running' : preparing ? 'The test server is being prepared' : null)
  let whyNoStop = $derived(serverRunning ? null : preparing ? "The test server is being prepared; it can be stopped once it's running" : "The test server isn't running")
  let whyNoQuick = $derived(
    serverRunning ? 'The test server is already running. Use Launch client to join it' : clientRunning ? 'The client is already running' : preparing ? 'The test server is being prepared' : null,
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
      <Tip tip={whyNoQuick ?? 'Starts the server, then launches the client and joins it'}>
        <button
          class="primary"
          class:busy={preparing}
          disabled={!!whyNoQuick}
          onclick={() => run(() => ((clientJoins = true), api.startTest(rebuild, true)))}
        >
          {preparing ? 'Starting' : 'Quick test'}
        </button>
      </Tip>
    </div>
  </div>

  {#if error}<Notice tone="error"><p>{error}</p></Notice>{/if}
  {#if job('test').phase === 'failed'}<Notice tone="error"><p>{job('test').message}</p></Notice>{/if}

  <div class="logs">
    <div class="pane">
      <div class="pane-head">
        <h3>Server</h3>
        <div class="buttons">
          <Tip tip={whyNoStart}>
            <button disabled={!!whyNoStart} onclick={() => run(() => api.startTest(rebuild, false))}>Start server</button>
          </Tip>
          <Tip tip={whyNoStop ?? 'Saves the world and shuts the server down'}>
            <button disabled={!!whyNoStop} onclick={() => run(() => api.serverCommand('stop'))}>Stop</button>
          </Tip>
          <Tip tip={whyNoStop ?? 'Ends the process at once without saving'}>
            <button class="danger" disabled={!!whyNoStop} onclick={() => run(() => api.killProcess('server'))}>Kill</button>
          </Tip>
        </div>
      </div>
      <LogView lines={serverLines} label="Server log" empty="Start the server to see its log." />
      <form class="console" onsubmit={(e) => { e.preventDefault(); sendCommand() }}>
        <input bind:value={command} placeholder="op Dev" disabled={!serverRunning} aria-label="Server command" aria-describedby="console-hint" />
        <Tip tip={whyNoSend}><button type="submit" disabled={!!whyNoSend}>Send</button></Tip>
      </form>
      <p class="hint muted" id="console-hint">{serverRunning ? 'Sends a command to the server console. Stop saves and shuts down; Kill ends the process without saving.' : 'Commands can be sent once the server is running.'}</p>
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
          <Tip tip={clientRunning ? 'Ends the client process at once' : "The client isn't running"}>
            <button class="danger" disabled={!clientRunning} onclick={() => run(() => api.killProcess('client'))}>Kill</button>
          </Tip>
        </div>
      </div>
      <LogView lines={job('client').lines} label="Client log" empty="The client log appears here once it launches." />
      <p class="hint muted">
        {clientRunning
          ? 'Running offline as the test username.'
          : !join
            ? 'Launches this instance offline to the title screen.'
            : testState.serverReady
              ? 'Launches this instance offline and joins the test server.'
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

  .console input {
    flex: 1;
    font-family: var(--font-log);
    font-size: var(--step--1);
  }
</style>
