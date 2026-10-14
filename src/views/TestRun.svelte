<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import { api, errorText, type Overview } from '../lib/api'
  import { job, serverMods, testOptions, testState } from '../lib/jobs.svelte'
  import JobLog from '../components/JobLog.svelte'
  import Tip from '../components/Tip.svelte'
  import LinkedText from '../components/LinkedText.svelte'

  let { overview, onservermods }: { overview: Overview | null; onservermods: () => void } = $props()

  const RECOPY_TIP =
    "Before each start, packraven copies only the mods, configs and modloader files that changed since the last start, judging by each file's size and modified time. " +
    'If the test server seems to run outdated files, tick this to recopy everything. It adds 10 to 60 seconds to every server start.'

  let join = $state(true)
  let clientJoins = $state(false)
  let command = $state('')
  let error: string | null = $state(null)

  function startTest(connect: boolean) {
    return run(async () => {
      await api.startTest(testOptions.recopyAll, connect)
      serverMods.changedSinceTest = false
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
    if (!line || whyNoSend) return
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
      <h2>Test</h2>
      <p class="muted" role="status">{status}</p>
    </div>
    <div class="controls">
      <label class="check">
        <input type="checkbox" bind:checked={testOptions.recopyAll} disabled={serverRunning} aria-describedby="recopy-hint" /> Recopy all server files
      </label>
      <Tip tip={whyNoQuick}>
        {#snippet children(why)}
          <button
            class="primary"
            class:busy={preparing}
            aria-disabled={!!whyNoQuick}
            aria-describedby={why}
            title={whyNoQuick ? undefined : 'Start the server, then launch the client and join it'}
            onclick={() => !whyNoQuick && ((clientJoins = true), startTest(true))}
          >
            {preparing ? 'Starting' : 'Quick test'}
          </button>
        {/snippet}
      </Tip>
    </div>
  </div>

  <p class="hint muted recopy" id="recopy-hint">{serverRunning ? 'Stop the test server to change whether it recopies everything.' : RECOPY_TIP}</p>

  {#if serverRunning && serverMods.changedSinceTest}
    <Notice tone="warn">
      <p>You changed server mod overrides after the test server started. Stop it and start it again to use them.</p>
    </Notice>
  {/if}
  {#if error}<Notice tone="error"><p><LinkedText text={error} /></p></Notice>{/if}
  {#if job('test').phase === 'failed'}<Notice tone="error"><p><LinkedText text={job('test').message ?? ''} /></p></Notice>{/if}
  {#if job('client').phase === 'failed'}<Notice tone="error"><p>Couldn't launch the client: <LinkedText text={job('client').message ?? ''} /></p></Notice>{/if}

  <div class="logs">
    <div class="pane">
      <div class="pane-head">
        <h3>Server</h3>
        <div class="buttons">
          <Tip tip={whyNoStart}>
            {#snippet children(why)}
              <button aria-disabled={!!whyNoStart} aria-describedby={why} onclick={() => !whyNoStart && startTest(false)}>Start server</button>
            {/snippet}
          </Tip>
          <Tip tip={whyNoStop ?? 'Save the world and shut the server down'}>
            {#snippet children(why)}
              <button aria-disabled={!!whyNoStop} aria-describedby={why} onclick={() => !whyNoStop && run(() => api.serverCommand('stop'))}>Stop</button>
            {/snippet}
          </Tip>
          <Tip tip={whyNoStop ?? 'End the process at once without saving'}>
            {#snippet children(why)}
              <button class="danger" aria-disabled={!!whyNoStop} aria-describedby={why} onclick={() => !whyNoStop && run(() => api.killProcess('server'))}>Kill</button>
            {/snippet}
          </Tip>
        </div>
      </div>
      <JobLog lines={serverLines} label="Server log" empty="Start the server to see its log." />
      <form class="console" onsubmit={(e) => { e.preventDefault(); sendCommand() }}>
        <input bind:value={command} placeholder="op Dev" disabled={!serverRunning} aria-label="Server command" aria-describedby="console-hint" />
        <Tip tip={whyNoSend}>
          {#snippet children(why)}<button type="submit" aria-disabled={!!whyNoSend} aria-describedby={why}>Send</button>{/snippet}
        </Tip>
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
          <label class="check join">
            <input type="checkbox" bind:checked={join} disabled={clientRunning} aria-describedby="client-hint" /> Join server
          </label>
          <Tip tip={whyNoLaunch}>
            {#snippet children(why)}
              <button aria-disabled={!!whyNoLaunch} aria-describedby={why} onclick={() => !whyNoLaunch && run(() => ((clientJoins = join), api.startClient(join)))}>Launch client</button>
            {/snippet}
          </Tip>
          <Tip tip={clientRunning ? 'End the client process at once' : "The client isn't running"}>
            {#snippet children(why)}
              <button class="danger" aria-disabled={!clientRunning} aria-describedby={why} onclick={() => clientRunning && run(() => api.killProcess('client'))}>Kill</button>
            {/snippet}
          </Tip>
        </div>
      </div>
      <JobLog lines={job('client').lines} label="Client log" empty="The client log will appear here after you launch it." />
      <p class="hint muted" id="client-hint">
        {clientRunning
          ? "Running offline as the test username. You can't change Join server while the client runs."
          : !join
            ? 'Launch this instance offline to the title screen.'
            : testState.serverReady
              ? 'Launch this instance offline and join the test server.'
              : 'Start the server first, or untick Join server to launch to the title screen.'}
      </p>
    </div>
  </div>
</div>

<style>
  .test {
    display: flex;
    flex-direction: column;
    gap: 18px;
    flex: 1;
    min-height: 480px;
  }

  .recopy {
    margin-top: -10px;
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

  .pane-head,
  .buttons {
    flex-wrap: wrap;
  }

  .buttons {
    justify-content: end;
  }

  .join {
    white-space: nowrap;
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
