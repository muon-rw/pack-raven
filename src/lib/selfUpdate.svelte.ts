import { listen } from '@tauri-apps/api/event'
import { ask, message } from '@tauri-apps/plugin-dialog'
import { api, errorText, type UpdateView } from './api'
import { dirtyCount } from './workspace.svelte'

export const selfUpdate = $state({ view: null as UpdateView | null })

export function updateWaiting(view: UpdateView | null): view is UpdateView & { status: { phase: 'available' | 'ready'; version: string } } {
  return view?.status.phase === 'available' || view?.status.phase === 'ready'
}

// The backend only reports phase changes, so we re-read the view to keep `blocked` current as
// tests and tasks start and stop.
export async function refreshSelfUpdate() {
  try {
    selfUpdate.view = await api.updateStatus()
  } catch {
    // Keep the last view; a failed read doesn't change what we know about updates.
  }
}

export function startSelfUpdates(): () => void {
  refreshSelfUpdate()
  const unlisten = listen<UpdateView>('self-update-changed', ({ payload }) => {
    selfUpdate.view = payload
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
    await api.installUpdate()
  } catch (error) {
    await message(errorText(error), { title: "Couldn't update packraven", kind: 'error' })
  }
}
