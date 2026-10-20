<script lang="ts">
  import { onMount } from 'svelte'
  import Tip from './Tip.svelte'
  import { api, errorText } from '../lib/api'
  import { job, serverPack, testOptions, testState } from '../lib/jobs.svelte'
  import { killServer, launch, restartServer, startServer, stopServer } from '../lib/launch.svelte'
  import { nav } from '../lib/nav.svelte'

  let { running }: { running: boolean } = $props()

  const SLOW_STOP_MS = 10_000

  let error = $state<string | null>(null)
  // A failure from a start made in the Run view belongs to that view, so we only report our own.
  let startedHere = $state(false)
  // Until settings load we don't know, so we don't block; start_test still refuses.
  let eulaAccepted = $state<boolean | null>(null)
  onMount(() => {
    api.getSettings().then((settings) => (eulaAccepted = settings.minecraftEulaAccepted), () => {})
  })

  let preparing = $derived(job('test').phase === 'running')
  let starting = $derived(!launch.restarting && (preparing || (running && !testState.serverReady)))
  let stopping = $derived(running && launch.stopRequested && !launch.restarting)

  // A modded server can hang while it saves, so after a while we let Stop end the process instead.
  let slowStop = $state(false)
  $effect(() => {
    if (!stopping) return
    const timer = setTimeout(() => (slowStop = true), SLOW_STOP_MS)
    return () => {
      clearTimeout(timer)
      slowStop = false
    }
  })

  let failure = $derived(error ?? launch.restartError ?? (startedHere && job('test').phase === 'failed' ? job('test').message : null))
  let exitStatus = $derived(job('server').message?.replace(/^Exited/, 'exited'))
  let crashed = $derived(!failure && !running && !preparing && launch.exitedOnItsOwn)
  let needsEula = $derived(!running && eulaAccepted === false)

  let whyNoStart = $derived(
    launch.restarting || starting
      ? 'packraven is starting the test server'
      : stopping
        ? 'The test server is stopping'
        : job('mods').phase === 'running'
          ? 'Wait for packraven to finish changing mods'
          : needsEula
            ? 'Accept the Minecraft EULA in Run first'
            : null,
  )
  let whyNoStop = $derived(
    launch.restarting || (running && !stopping) || slowStop
      ? null
      : stopping
        ? 'The test server is saving its world and stopping'
        : preparing
          ? "packraven is preparing the test server; you can stop it once it's running"
          : "The test server isn't running",
  )

  let status = $derived(
    launch.restarting
      ? 'Saving the world, then starting the test server again.'
      : stopping
        ? slowStop
          ? 'The test server is taking a while to stop. Kill it to end the process without saving.'
          : 'Saving the world and stopping the test server.'
        : preparing && !running
          ? testOptions.recopyAll
            ? 'Recopying all server files for the test server.'
            : 'Preparing the test server.'
          : running && !testState.serverReady
            ? 'The test server is starting.'
            : running
              ? [
                  'The test server is up.',
                  testState.players.length ? `${testState.players.join(', ')} joined.` : null,
                  serverPack.changedSinceTest ? 'Restart it to use your changes.' : null,
                ]
                  .filter(Boolean)
                  .join(' ')
              : '',
  )

  async function act(action: () => Promise<void>) {
    error = null
    launch.restartError = null
    try {
      await action()
    } catch (e) {
      error = errorText(e)
    }
  }

  function startOrRestart() {
    if (whyNoStart) return
    startedHere = true
    act(running ? restartServer : () => startServer(false))
  }

  function stopOrKill() {
    if (whyNoStop) return
    // The server is already saving to restart, so stopping only means not starting it again.
    if (launch.restarting) launch.restarting = false
    else act(slowStop ? killServer : stopServer)
  }
</script>

<div class="launch">
  <div class="buttons">
    <!-- Each slot keeps one button through every state, so focus stays put when the label changes. -->
    <Tip tip={whyNoStart}>
      {#snippet children(why)}
        <button
          class:primary={running && testState.serverReady && serverPack.changedSinceTest && !stopping}
          class:busy={launch.restarting || starting}
          aria-busy={launch.restarting || starting}
          aria-disabled={!!whyNoStart}
          aria-describedby={why}
          title={whyNoStart ? undefined : running ? 'Save the world, then start the test server again' : testOptions.recopyAll ? 'Build the test server from this pack, recopying every file, and start it' : 'Build the test server from this pack and start it'}
          onclick={startOrRestart}
        >
          {launch.restarting ? 'Restarting' : starting ? 'Starting' : running ? 'Restart server' : 'Start server'}
        </button>
      {/snippet}
    </Tip>
    <Tip tip={whyNoStop}>
      {#snippet children(why)}
        <button
          class:danger={slowStop}
          aria-disabled={!!whyNoStop}
          aria-describedby={why}
          title={whyNoStop ? undefined : slowStop ? 'End the process at once without saving' : 'Save the world and shut down the test server'}
          onclick={stopOrKill}
        >
          {slowStop ? 'Kill server' : 'Stop server'}
        </button>
      {/snippet}
    </Tip>
  </div>
  <p class="status" class:failed={failure || crashed} role="status">
    {#if failure}
      <span class="message" title={failure}>Couldn't start the test server: {failure}</span>
      <button class="link" onclick={() => (nav.view = 'test')}>See the log in Run</button>
    {:else if crashed}
      <span class="message">The test server stopped on its own{exitStatus ? ` (${exitStatus})` : ''}.</span>
      <button class="link" onclick={() => (nav.view = 'test')}>See the log in Run</button>
    {:else if needsEula}
      <span class="message">Accept the Minecraft EULA to start test servers.</span>
      <button class="link" onclick={() => (nav.view = 'test')}>Open Run</button>
    {:else}
      <span class="message">{status}</span>
    {/if}
  </p>
</div>

<style>
  .launch {
    display: grid;
    justify-items: end;
    gap: 6px;
  }

  .buttons {
    display: flex;
    flex-wrap: wrap;
    justify-content: end;
    gap: 8px;
  }

  .buttons button {
    white-space: nowrap;
  }

  /* We keep the line's height when it's empty, so a status appearing doesn't move the tabs below. */
  .status {
    min-height: 1lh;
    max-width: 42ch;
    text-align: right;
    color: var(--muted);
    font-size: var(--step--1);
  }

  .status.failed {
    color: var(--fail);
  }

  /* The full message is in the Run view's server log, so we keep the header from growing past two lines. */
  .message {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
  }
</style>
