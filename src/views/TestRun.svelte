<script lang="ts">
  import { onMount } from 'svelte'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import Notice from '../components/Notice.svelte'
  import { api, errorText, type MinecraftAccount, type Overview } from '../lib/api'
  import { job, serverPack, testOptions, testState } from '../lib/jobs.svelte'
  import { killServer, launch, launchClient, restartServer, startServer, stopServer } from '../lib/launch.svelte'
  import { serverView } from '../lib/nav.svelte'
  import JobLog from '../components/JobLog.svelte'
  import Tip from '../components/Tip.svelte'
  import LinkedText from '../components/LinkedText.svelte'
  import Splitter from '../components/Splitter.svelte'
  import { StoredSplit, clamp } from '../lib/storedSplit.svelte'

  let { overview, onserver, onappsettings }: { overview: Overview | null; onserver: () => void; onappsettings: () => void } = $props()

  // The logs are peers, so we keep their ratio rather than a width as the window resizes.
  const LOGS_GAP = 20
  const logSplit = new StoredSplit('packraven.testLogRatio')
  let logsWidth = $state(0)
  let splitTotal = $derived(logsWidth - LOGS_GAP)
  let logMin = $derived(Math.min(220, splitTotal * 0.4))
  let serverWidth = $derived(clamp(splitTotal * (logSplit.value ?? 0.5), logMin, splitTotal - logMin))

  let command = $state('')
  let error: string | null = $state(null)

  const WAIT_FOR_MODS = 'Wait for packraven to finish changing mods'
  const EULA_URL = 'https://aka.ms/MinecraftEULA'
  // Until settings load we don't know, so we neither ask nor block; start_test still refuses.
  let eulaAccepted: boolean | null = $state(null)
  let account = $state<MinecraftAccount | null>(null)
  let onlineMode = $state(true)
  let offlineName = $state('Dev')
  // Each load is separate, so a keychain read that fails can't stop us gating on the EULA.
  onMount(() => {
    run(async () => {
      const settings = await api.getSettings()
      eulaAccepted = settings.minecraftEulaAccepted
      onlineMode = settings.minecraftOnlineMode
    })
    api.minecraftAccount().then((signedIn) => (account = signedIn), () => {})
    api.getPackConfig().then((config) => (offlineName = config.test.username), () => {})
  })


  function acceptEula() {
    return run(async () => {
      await api.acceptMinecraftEula()
      eulaAccepted = true
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
    await run(() => (line === 'stop' ? stopServer() : api.serverCommand(line)))
    command = ''
  }

  let serverRunning = $derived(overview?.serverRunning ?? false)
  let clientRunning = $derived(overview?.clientRunning ?? false)
  let preparing = $derived(job('test').phase === 'running')
  let serverLines = $derived(job('test').lines.concat(job('server').lines))
  let running = $derived(serverRunning || clientRunning || preparing)
  // Once a run starts we show what the backend resolved; before that, what the next run intends.
  let confirmed = $derived(running ? testState.player : null)
  let playerName = $derived(confirmed?.name ?? account?.name ?? offlineName)
  let playing = $derived((confirmed ? confirmed.offline : !account) ? `offline as ${playerName}` : `as ${playerName}`)
  let modeLine = $derived(
    confirmed
      ? confirmed.offline
        ? `This run is offline, as ${confirmed.name}.`
        : confirmed.serverOnline === null
          ? `The client plays as ${confirmed.name}.`
          : `This run's server is ${confirmed.serverOnline ? 'online' : 'offline'}, and the client plays as ${confirmed.name}.`
      : account
        ? `Test servers run ${onlineMode ? 'online' : 'offline'}, and the client plays as ${account.name}.`
        : `Test runs are offline, as ${offlineName}.`,
  )
  let status = $derived(
    testState.players.length
      ? `${testState.players.join(', ')} joined the server.`
      : clientRunning
        ? testState.clientJoins
          ? 'The client is launching and will join when it reaches the title screen.'
          : 'The client is running.'
        : testState.serverReady
          ? 'The server is up.'
          : serverRunning
            ? 'The server is starting.'
            : preparing
              ? 'Preparing the test server.'
              : 'Start a test server or client on this computer.',
  )

  let modsChanging = $derived(job('mods').phase === 'running')
  let serverStarting = $derived(preparing || (serverRunning && !testState.serverReady))
  let whyNoEula = $derived(eulaAccepted === false ? 'Accept the Minecraft EULA first' : null)
  let whyNoStart = $derived(serverRunning ? 'The test server is already running' : preparing ? 'packraven is preparing the test server' : modsChanging ? WAIT_FOR_MODS : whyNoEula)
  let whyNoStop = $derived(serverRunning ? null : preparing ? "packraven is preparing the test server; you can stop it once it's running" : "The test server isn't running")
  let whyNoQuick = $derived(
    serverRunning ? 'The test server is already running. Use Launch client to join it' : clientRunning ? 'The client is already running' : preparing ? 'packraven is preparing the test server' : modsChanging ? WAIT_FOR_MODS : whyNoEula,
  )
  let whyNoLaunch = $derived(
    clientRunning
      ? 'The client is already running'
      : modsChanging
        ? WAIT_FOR_MODS
        : testOptions.joinServer && !testState.serverReady
          ? serverStarting
            ? 'Waiting for the test server to finish starting, or untick Join server'
            : 'Start the test server first, or untick Join server'
          : null,
  )
  let whyNoSend = $derived(!serverRunning ? "The test server isn't running" : !command.trim() ? 'Type a command first' : null)
</script>

<div class="test">
  <div class="head">
    <div>
      <h2>Run</h2>
      <p class="muted" role="status">{status}</p>
      <p class="muted mode">
        {modeLine}
        {#if !running}
          <button type="button" class="link" onclick={onappsettings}>{account ? 'Change in App settings' : 'Sign in to play as yourself'}</button>
        {/if}
      </p>
    </div>
    <div class="controls">
      <div class="recopy">
        <input id="recopy" type="checkbox" bind:checked={testOptions.recopyAll} disabled={serverRunning} aria-describedby="recopy-hint" />
        <label for="recopy">Recopy all server files, not just changes</label>
        <p class="hint muted" id="recopy-hint">
          {serverRunning ? 'Stop the test server to change this.' : 'Tick this to give each test server a clean slate. Server starts will take 10 to 60 seconds longer.'}
        </p>
      </div>
      <Tip tip={whyNoQuick}>
        {#snippet children(why)}
          <button
            class="primary"
            class:busy={preparing}
            aria-disabled={!!whyNoQuick}
            aria-describedby={why}
            title={whyNoQuick ? undefined : 'Start the server, then launch the client and join it'}
            onclick={() => !whyNoQuick && run(() => startServer(true))}
          >
            {preparing ? 'Starting' : 'Quick test'}
          </button>
        {/snippet}
      </Tip>
    </div>
  </div>

  {#if eulaAccepted === false}
    <Notice tone="warn">
      <p><strong>Accept the Minecraft EULA to start test servers.</strong> Vanilla's server won't run until you do. packraven remembers your answer on this computer and marks it accepted in each test server it starts. You can still launch the client on its own.</p>
      <div class="eula-actions">
        <a href={EULA_URL} onclick={(e) => { e.preventDefault(); openUrl(EULA_URL) }}>Read the Minecraft EULA</a>
        <button class="primary" onclick={acceptEula}>I accept the EULA</button>
      </div>
    </Notice>
  {/if}
  {#if confirmed?.renewalFailure}
    <Notice tone="warn">
      <p>
        Couldn't renew your Minecraft sign-in, so this run is offline as {confirmed.name}.
        <button type="button" class="link" onclick={onappsettings}>Sign in again under App settings</button>.
      </p>
      <p class="muted">{confirmed.renewalFailure}</p>
    </Notice>
  {/if}
  {#if serverRunning && serverPack.changedSinceTest && !launch.restarting}
    <Notice tone="warn">
      <p>You changed the server pack after the test server started. Restart it to use your changes.</p>
      <p><button onclick={() => run(restartServer)}>Restart server</button></p>
    </Notice>
  {/if}
  {#if launch.exitedOnItsOwn && !serverRunning && !preparing && job('test').phase !== 'failed'}
    <Notice tone="error">
      <p>
        The test server stopped on its own{job('server').message ? ` (${job('server').message?.replace(/^Exited/, 'exited')})` : ''}. Its log is below.
        If a mod crashed it, <button class="link" onclick={() => ((serverView.tab = 'mods'), onserver())}>leave it off the server</button>.
      </p>
    </Notice>
  {/if}
  {#if error}<Notice tone="error"><p><LinkedText text={error} /></p></Notice>{/if}
  {#if launch.restartError}<Notice tone="error"><p>Couldn't restart the test server: <LinkedText text={launch.restartError} /></p></Notice>{/if}
  {#if job('test').phase === 'failed'}<Notice tone="error"><p><LinkedText text={job('test').message ?? ''} /></p></Notice>{/if}
  {#if job('client').phase === 'failed'}<Notice tone="error"><p>Couldn't launch the client: <LinkedText text={job('client').message ?? ''} /></p></Notice>{/if}

  <div class="logs" bind:clientWidth={logsWidth} style:grid-template-columns={logsWidth ? `${serverWidth}px ${LOGS_GAP}px minmax(0, 1fr)` : null}>
    <div class="pane" id="server-pane">
      <div class="pane-head">
        <h3>Server</h3>
        <div class="buttons">
          <Tip tip={whyNoStart}>
            {#snippet children(why)}
              <button aria-disabled={!!whyNoStart} aria-describedby={why} onclick={() => !whyNoStart && run(() => startServer(false))}>Start server</button>
            {/snippet}
          </Tip>
          <Tip tip={whyNoStop ?? 'Save the world and shut down the server'}>
            {#snippet children(why)}
              <button aria-disabled={!!whyNoStop} aria-describedby={why} onclick={() => !whyNoStop && run(stopServer)}>Stop</button>
            {/snippet}
          </Tip>
          <Tip tip={whyNoStop ?? 'End the process at once without saving'}>
            {#snippet children(why)}
              <button class="danger" aria-disabled={!!whyNoStop} aria-describedby={why} onclick={() => !whyNoStop && run(killServer)}>Kill</button>
            {/snippet}
          </Tip>
        </div>
      </div>
      <JobLog lines={serverLines} label="Server log" empty="Start the server to see its log." />
      <form class="console" onsubmit={(e) => { e.preventDefault(); sendCommand() }}>
        <input bind:value={command} placeholder="op {playerName}" disabled={!serverRunning} aria-label="Server command" aria-describedby="console-hint" />
        <Tip tip={whyNoSend}>
          {#snippet children(why)}<button type="submit" aria-disabled={!!whyNoSend} aria-describedby={why}>Send</button>{/snippet}
        </Tip>
      </form>
      <p class="hint muted" id="console-hint">
        {serverRunning ? 'Send commands to the server console. Use Stop to save and shut down, or Kill to end the process without saving.' : 'You can send commands once the server is running.'}
        Server crashing due to a mod? <button class="link" onclick={() => ((serverView.tab = 'mods'), onserver())}>Leave it off the server</button>.
      </p>
    </div>

    <Splitter value={serverWidth} min={logMin} max={splitTotal - logMin} total={splitTotal} label="Resize the server and client logs" controls="server-pane" onresize={(px) => logSplit.set(px / splitTotal)} onreset={() => logSplit.reset()} />

    <div class="pane">
      <div class="pane-head">
        <h3>Client</h3>
        <div class="buttons">
          <label class="check join">
            <!-- Quick test always joins, so while the client runs we show what that run did. -->
            <input type="checkbox" checked={clientRunning ? testState.clientJoins : testOptions.joinServer} onchange={(e) => (testOptions.joinServer = e.currentTarget.checked)} disabled={clientRunning} aria-describedby="client-hint" /> Join server
          </label>
          <Tip tip={whyNoLaunch}>
            {#snippet children(why)}
              <button aria-disabled={!!whyNoLaunch} aria-describedby={why} onclick={() => !whyNoLaunch && run(() => launchClient(testOptions.joinServer))}>Launch client</button>
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
          ? `Running ${playing}${testState.clientJoins ? ' and joining the test server' : ', without joining the test server'}.`
          : !testOptions.joinServer
            ? `Launch this instance ${playing} to the title screen.`
            : testState.serverReady
              ? `Launch this instance ${playing} and join the test server.`
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

  /* The hint sits in the label's column, so it lines up with the label text rather than the checkbox. */
  .recopy {
    display: grid;
    grid-template-columns: auto minmax(0, 30ch);
    align-items: center;
    column-gap: 8px;
    row-gap: 2px;
  }

  .recopy input {
    margin: 0;
  }

  .recopy label {
    display: block;
    color: var(--text);
    font-size: var(--step-0);
  }

  .recopy .hint {
    grid-column: 2;
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

  .head .mode {
    margin-top: 2px;
    font-size: var(--step--1);
  }

  .eula-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 16px;
  }

  .controls {
    display: flex;
    align-items: start;
    gap: 18px;
    flex-wrap: wrap;
    justify-content: end;
  }

  /* We share rows across the panes, so both logs start level when the client's buttons wrap. */
  .logs {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 20px minmax(0, 1fr);
    /* The floor keeps each log's toolbar clear of the console when notices take the space; the view scrolls instead. */
    grid-template-rows: auto minmax(220px, 1fr) auto auto;
  }

  .pane {
    display: grid;
    grid-row: span 4;
    grid-template-rows: subgrid;
    grid-template-columns: minmax(0, 1fr);
    row-gap: 10px;
    min-height: 0;
  }

  .pane > .hint {
    grid-row: 4;
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
