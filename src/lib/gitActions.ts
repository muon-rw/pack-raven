import { ask } from '@tauri-apps/plugin-dialog'
import { api, errorText, type FileChange, type GitStatus, type UntrackPreview } from './api'
import { LOCKFILE } from './modsync'
import { showDiff } from './nav.svelte'
import { openIgnoreDialog, type IgnoreRequest } from './ignoreDialog.svelte'
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

// Pass onuntracked to confirm untracking with an undo; without it we report it as a notice.
export type Feedback = { ondone: () => void; onerror: (message: string) => void; onnotice: (message: string) => void; onuntracked?: (paths: string[]) => void }

export async function gitMenu(paths: string[], status: GitStatus | null, ignored: (path: string) => boolean, feedback: Feedback): Promise<Submenu | null> {
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
    ...(await ignoreItems(ignorable, feedback)),
    ...(await untrackItems(paths, feedback)),
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

const MAX_HINT = 32

function patternHint(patterns: string[]) {
  if (patterns.length > 1) return `${patterns.length} lines`
  return patterns[0].length > MAX_HINT ? `${patterns[0].slice(0, MAX_HINT - 1)}…` : patterns[0]
}

export async function ignoreItems(paths: string[], feedback: Feedback): Promise<MenuItem[]> {
  if (!paths.length) return []
  const options = await api.gitIgnoreOptions(paths).catch(() => null)
  if (!options?.length) return []
  return [
    ...options.map((option, chosen) => ({
      label: `Ignore ${option.label[0].toLowerCase()}${option.label.slice(1)}`,
      shortcut: patternHint(option.patterns),
      run: () => ignoreWith({ paths, options, chosen, feedback }),
    })),
    { label: 'Ignore with a custom pattern…', run: () => openIgnoreDialog({ paths, options, chosen: 0, feedback }) },
  ]
}

// Git doesn't apply .gitignore to files it already tracks, so when the pattern matches some, we
// open the dialog instead, where you can choose to stop tracking them.
async function ignoreWith(request: IgnoreRequest) {
  const { patterns } = request.options[request.chosen]
  try {
    if ((await api.gitIgnorePreview(patterns)).tracked.length) return openIgnoreDialog(request)
    await api.gitIgnore(patterns)
    request.feedback.ondone()
  } catch (e) {
    request.feedback.onerror(errorText(e))
  }
}

export function untrackConsequence(count: number) {
  return count === 1
    ? 'The file stays on disk here, and we stage its removal for your next commit. When you or a collaborator pull that commit on another machine, git deletes the copy there.'
    : 'The files stay on disk here, and we stage their removal for your next commit. When you or a collaborator pull that commit on another machine, git deletes the copies there.'
}

export function untrackedNotice(paths: string[]) {
  const one = paths.length === 1
  return `Stopped tracking ${one ? paths[0] : `${paths.length} files`}. ${one ? 'It stays' : 'They stay'} on disk here. Once you commit, git deletes the ${one ? 'copy' : 'copies'} on other machines when you or a collaborator pull.`
}

export function reportUntracked(feedback: Feedback, paths: string[]) {
  if (feedback.onuntracked) feedback.onuntracked(paths)
  else feedback.onnotice(untrackedNotice(paths))
}

export function untrackLabel(preview: UntrackPreview) {
  const count = preview.ignored.length + preview.uncovered.length
  return `Stop tracking${count > 1 ? ` ${count} files` : ''}${preview.uncovered.length ? '…' : ''}`
}

export async function untrackItems(paths: string[], feedback: Feedback): Promise<MenuItem[]> {
  const preview = paths.length ? await api.gitUntrackPreview(paths).catch(() => null) : null
  if (!preview || !(preview.ignored.length || preview.uncovered.length)) return []
  return [{ label: untrackLabel(preview), run: () => stopTracking(paths, feedback, preview) }]
}

// A file .gitignore doesn't cover would come back as a new file, so for those we open the ignore
// dialog to add a pattern as we untrack them.
export async function stopTracking(paths: string[], feedback: Feedback, known?: UntrackPreview) {
  try {
    const { ignored, uncovered } = known ?? (await api.gitUntrackPreview(paths))
    if (uncovered.length) return openIgnoreDialog({ paths, options: await api.gitIgnoreOptions(paths), chosen: 0, feedback, untrack: true })
    if (!ignored.length) return feedback.onnotice("Git doesn't track any of these files.")
    await api.gitUntrack(ignored)
    feedback.ondone()
    reportUntracked(feedback, ignored)
  } catch (e) {
    feedback.onerror(errorText(e))
  }
}
