import { listen } from '@tauri-apps/api/event'
import { ask, message } from '@tauri-apps/plugin-dialog'
import { api, errorText, type UpdateView } from './api'
import { dirtyCount } from './workspace.svelte'

export type ChangelogRequest = {
  mode: 'updated' | 'available' | 'current'
  since: string
  through: string
}

export const selfUpdate = $state({
  view: null as UpdateView | null,
  changelog: null as ChangelogRequest | null,
  lastUpdatedFrom: null as string | null,
})

let autoShownPair: string | null = null

function applyView(view: UpdateView) {
  selfUpdate.view = view
  if (view.updatedFrom) {
    selfUpdate.lastUpdatedFrom = view.updatedFrom
    const pair = `${view.updatedFrom}->${view.current}`
    if (autoShownPair !== pair) {
      autoShownPair = pair
      selfUpdate.changelog = { mode: 'updated', since: view.updatedFrom, through: view.current }
    }
  }
}

export function updateWaiting(view: UpdateView | null): view is UpdateView & { status: { phase: 'available' | 'ready'; version: string } } {
  return view?.status.phase === 'available' || view?.status.phase === 'ready'
}

export function openUpdateChangelog(since: string, through: string, mode: ChangelogRequest['mode'] = 'available') {
  selfUpdate.changelog = { mode, since, through }
}

export function closeUpdateChangelog() {
  const needsDismiss = selfUpdate.changelog?.mode === 'updated' || !!selfUpdate.view?.updatedFrom
  selfUpdate.changelog = null
  if (needsDismiss) {
    if (selfUpdate.view) selfUpdate.view.updatedFrom = null
    api.dismissUpdateChangelog().catch(() => {})
  }
}

// The backend only reports phase changes, so we re-read the view to keep `blocked` current as
// tests and tasks start and stop.
export async function refreshSelfUpdate() {
  try {
    applyView(await api.updateStatus())
  } catch {
    // Keep the last view; a failed read doesn't change what we know about updates.
  }
}

export function startSelfUpdates(): () => void {
  refreshSelfUpdate()
  const unlisten = listen<UpdateView>('self-update-changed', ({ payload }) => {
    applyView(payload)
  })
  return () => {
    unlisten.then((stop) => stop())
  }
}

export async function installNow() {
  const unsaved = dirtyCount()
  if (unsaved) {
    const confirmed = await ask(`${unsaved === 1 ? 'A file has' : `${unsaved} files have`} unsaved changes. If you restart packraven to update, they're lost.`, {
      title: 'Unsaved changes',
      kind: 'warning',
      okLabel: 'Restart anyway',
      cancelLabel: 'Keep editing',
    })
    if (!confirmed) return
  }
  try {
    selfUpdate.changelog = null
    await api.installUpdate()
  } catch (error) {
    await message(errorText(error), { title: "Couldn't update packraven", kind: 'error' })
  }
}
