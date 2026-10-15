import { ask } from '@tauri-apps/plugin-dialog'
import { api, errorText, type FileChange, type GitStatus } from './api'
import { LOCKFILE } from './modsync'
import { showDiff } from './nav.svelte'
import type { MenuItem, Submenu } from './menu'

export async function confirmDiscard(changes: FileChange[]) {
  const names = changes.length === 1 ? changes[0].path : `${changes.length} files`
  const deletes = changes.some((c) => c.status === '?') ? ' Git deletes new files you discard.' : ''
  const mods = changes.some((c) => c.path === LOCKFILE) ? ' packraven also puts your mods back the way the last commit has them.' : ''
  return ask(`Discard your changes to ${names}? This can't be undone.${deletes}${mods}`, {
    title: 'Discard changes',
    kind: 'warning',
    okLabel: 'Discard',
    cancelLabel: 'Cancel',
  })
}

// packraven rewrites mods.lock.json from the mods folder, so discarding only the file would undo
// nothing. We revert the mods folder along with it instead.
export async function discardChanges(changes: FileChange[]) {
  const files = changes.filter((c) => c.path !== LOCKFILE)
  if (files.length) {
    await api.gitDiscard(
      files.filter((c) => c.status !== '?').map((c) => c.path),
      files.filter((c) => c.status === '?').map((c) => c.path),
    )
  }
  if (files.length < changes.length) await api.revertMods()
}

export type Feedback = { ondone: () => void; onerror: (message: string) => void; onnotice: (message: string) => void }

export function gitMenu(paths: string[], status: GitStatus | null, ignored: (path: string) => boolean, feedback: Feedback): Submenu | null {
  if (!status || !paths.length) return null
  const covered = (file: string) => paths.some((p) => file === p || file.startsWith(`${p}/`))
  const unstaged = status.unstaged.filter((c) => covered(c.path))
  const single = paths.length === 1 ? paths[0] : null
  const changed = single !== null && [...status.unstaged, ...status.staged].some((c) => c.path === single)
  const ignorable = paths.filter((p) => !ignored(p))
  const items: MenuItem[] = [
    ...(changed ? [{ label: 'Show diff', run: () => showDiff(single!) }] : []),
    ...(unstaged.length
      ? [{ label: unstaged.length === 1 ? 'Discard changes' : `Discard changes to ${unstaged.length} files`, danger: true, run: () => discard(unstaged, feedback) }]
      : []),
    'separator',
    ...(ignorable.length ? [{ label: 'Add to .gitignore', run: () => addToGitignore(ignorable, feedback) }] : []),
  ]
  return items.some((item) => item !== 'separator') ? { label: 'Git', items } : null
}

async function discard(changes: FileChange[], feedback: Feedback) {
  if (!(await confirmDiscard(changes))) return
  try {
    await discardChanges(changes)
    feedback.ondone()
  } catch (e) {
    feedback.onerror(errorText(e))
  }
}

export async function addToGitignore(paths: string[], feedback: Feedback) {
  try {
    const tracked = await api.gitIgnore(paths)
    feedback.ondone()
    if (tracked.length) {
      const names = tracked.length === 1 ? tracked[0] : `${tracked.length} of these paths`
      feedback.onnotice(`Git already tracks ${names}, so the new .gitignore line won't apply until you remove it from git with "git rm --cached".`)
    }
  } catch (e) {
    feedback.onerror(errorText(e))
  }
}
