import { api, errorText } from './api'
import { clearLog, onServerStopped, serverPack, testOptions, testState } from './jobs.svelte'

// stopRequested tells a stop we asked for apart from a crash, which the backend reports as a normal exit.
export const launch = $state({ restarting: false, restartError: null as string | null, stopRequested: false, exitedOnItsOwn: false })

let pendingSaves: Promise<unknown> = Promise.resolve()

// Settings save on blur, so clicking Start can fire alongside a save. We wait for it so the server
// starts with the new settings and the save can't mark the pack changed after the start cleared it.
export function trackSave<T>(save: Promise<T>): Promise<T> {
  pendingSaves = Promise.all([pendingSaves, save.catch(() => {})])
  return save
}

// The server log shows the test job's lines before the server's, and the backend only clears a
// job's lines once that job starts, so the last run's server log would sit under this one's.
export async function startServer(connect: boolean) {
  await pendingSaves
  clearLog('test', 'server', ...(connect ? (['client'] as const) : []))
  if (connect) testState.clientJoins = true
  launch.exitedOnItsOwn = false
  await api.startTest(testOptions.recopyAll, connect)
  serverPack.changedSinceTest = false
}

export async function launchClient(join: boolean) {
  await pendingSaves
  clearLog('client')
  testState.clientJoins = join
  await api.startClient(join)
}

export async function stopServer() {
  launch.restarting = false
  launch.stopRequested = true
  try {
    await api.serverCommand('stop')
  } catch (error) {
    launch.stopRequested = false
    throw error
  }
}

export async function killServer() {
  launch.restarting = false
  launch.stopRequested = true
  await api.killProcess('server')
}

// Stop saves the world before the server exits, so we start the next run only once it has.
export async function restartServer() {
  launch.restartError = null
  await stopServer()
  // The server can't have exited yet; vanilla saves every world before it does.
  launch.restarting = true
}

onServerStopped(() => {
  launch.exitedOnItsOwn = !launch.stopRequested
  launch.stopRequested = false
  if (!launch.restarting) return
  launch.restarting = false
  startServer(false).catch((error) => (launch.restartError = errorText(error)))
})
