<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import { api, errorText, type Overview } from '../lib/api'
  import { job, testState } from '../lib/jobs.svelte'
  import LogView from '../components/LogView.svelte'

  let { overview }: { overview: Overview | null } = $props()

  let rebuild = $state(false)
  let connect = $state(true)
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
        ? 'The client is launching and will join when it reaches the title screen.'
        : testState.serverReady
          ? 'The server is up.'
          : serverRunning
            ? 'The server is starting.'
            : preparing
              ? 'Preparing the test server.'
              : 'Starts the server pack locally in offline mode, then launches this instance offline and joins it.',
  )
</script>

<div class="test">
  <div class="head">
    <div>
      <h2>Test run</h2>
      <p class="muted">{status}</p>
    </div>
    <div class="controls">
      <label class="check"><input type="checkbox" bind:checked={rebuild} disabled={serverRunning} /> Rebuild server pack first</label>
      <label class="check"><input type="checkbox" bind:checked={connect} disabled={serverRunning} /> Launch the client and join</label>
      <button class="primary" class:busy={preparing} disabled={serverRunning || preparing} onclick={() => run(() => api.startTest(rebuild, connect))}>
        {preparing ? 'Starting' : 'Start test'}
      </button>
    </div>
  </div>

  {#if error}<Notice tone="error"><p>{error}</p></Notice>{/if}
  {#if job('test').phase === 'failed'}<Notice tone="error"><p>{job('test').message}</p></Notice>{/if}

  <div class="logs">
    <div class="pane">
      <div class="pane-head">
        <h3>Server</h3>
        <div class="buttons">
          <button disabled={!serverRunning} title="Saves the world and shuts the server down" onclick={() => run(() => api.serverCommand('stop'))}>Stop</button>
          <button class="danger" disabled={!serverRunning} title="Ends the process at once without saving" onclick={() => run(() => api.killProcess('server'))}>Kill</button>
        </div>
      </div>
      <LogView lines={serverLines} label="Server log" empty="Start a test to see the server log." />
      <form class="console" onsubmit={(e) => { e.preventDefault(); sendCommand() }}>
        <input bind:value={command} placeholder="op Dev" disabled={!serverRunning} aria-label="Server command" aria-describedby="console-hint" />
        <button type="submit" disabled={!serverRunning || !command.trim()}>Send</button>
      </form>
      <p class="hint muted" id="console-hint">{serverRunning ? 'Sends a command to the server console. Stop saves and shuts down; Kill ends the process without saving.' : 'Commands can be sent once the server is running.'}</p>
    </div>

    <div class="pane">
      <div class="pane-head">
        <h3>Client</h3>
        <div class="buttons">
          <button disabled={clientRunning || !testState.serverReady} onclick={() => run(() => api.startClient())}>Launch client</button>
          <button class="danger" disabled={!clientRunning} title="Ends the client process at once" onclick={() => run(() => api.killProcess('client'))}>Kill</button>
        </div>
      </div>
      <LogView lines={job('client').lines} label="Client log" empty="The client log appears here once it launches." />
      <p class="hint muted">
        {clientRunning ? 'Running offline as the test username.' : testState.serverReady ? 'Launches this instance offline and joins the test server.' : 'The client can launch once the test server is up.'}
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

  .hint {
    font-size: var(--step--1);
  }

  .console input {
    flex: 1;
    font-family: var(--font-log);
    font-size: var(--step--1);
  }
</style>
