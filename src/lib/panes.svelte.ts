import { getContext, setContext } from 'svelte'
import { StoredSplit } from './storedSplit.svelte'

export const views = ['changes', 'test', 'mods', 'server', 'files', 'git', 'publish', 'releases', 'pack', 'settings', 'add'] as const
export type View = (typeof views)[number]

export const titles: Record<View, string> = {
  changes: 'Changes',
  test: 'Run',
  mods: 'Mods',
  server: 'Server',
  files: 'Files',
  git: 'Git',
  publish: 'Publish',
  releases: 'Releases',
  pack: 'Pack settings',
  settings: 'App settings',
  add: 'Add pack',
}

// Each view shows in at most one pane, since views keep their state (open files, the Server tab,
// element ids) in modules shared by every instance.
export const layout = $state({ panes: ['mods'] as View[], focused: 0 })

// We store the left pane's share of the width, so the split keeps its proportions as the window resizes.
export const paneSplit = new StoredSplit('packraven.paneSplit')

let previous: View | null = null
// Add pack takes the whole work area, so we set the split aside and bring it back when you leave.
let parked: { panes: View[]; focused: number } | null = null

export function focusedView() {
  return layout.panes[layout.focused]
}

export function isSplit() {
  return layout.panes.length > 1
}

export function only(view: View) {
  if (view === 'add' && isSplit()) parked = { panes: [...layout.panes], focused: layout.focused }
  else if (view !== 'add') parked = null
  layout.panes = [view]
  layout.focused = 0
}

export function show(view: View, pane?: number) {
  if (view === 'add') return only(view)
  if (focusedView() === 'add' && parked) {
    layout.panes = parked.panes
    layout.focused = parked.focused
    parked = null
    pane = undefined
  }
  const at = layout.panes.indexOf(view)
  if (at >= 0) {
    layout.focused = at
    return
  }
  const target = pane ?? layout.focused
  previous = layout.panes[target]
  layout.panes[target] = view
  layout.focused = target
}

// Links inside a view open in the other pane of a split, so you keep what you were reading.
export function showFrom(view: View, pane: number) {
  show(view, isSplit() ? 1 - pane : pane)
}

export function openBeside(view: View) {
  const current = focusedView()
  if (view === 'add' || current === 'add') return show(view)
  const at = layout.panes.indexOf(view)
  if (at >= 0) {
    layout.focused = at
    return
  }
  if (isSplit()) {
    const other = 1 - layout.focused
    layout.panes[other] = view
    layout.focused = other
  } else {
    layout.panes.push(view)
    layout.focused = 1
  }
}

export function closePane(pane: number) {
  if (!isSplit()) return
  previous = layout.panes[pane]
  layout.panes.splice(pane, 1)
  layout.focused = 0
}

export function swapPanes() {
  layout.panes.reverse()
  layout.focused = 1 - layout.focused
}

// We split with the view you were on before, or Files when there's none to go back to.
export function splitCompanion(): View {
  const current = focusedView()
  return previous && previous !== current && previous !== 'add' ? previous : current === 'files' ? 'git' : 'files'
}

export function toggleSplit() {
  if (isSplit()) return closePane(1 - layout.focused)
  if (focusedView() === 'add') return
  openBeside(splitCompanion())
  layout.focused = 0
}

const PANE = Symbol('pane')

export function providePane(focused: () => boolean) {
  setContext(PANE, focused)
}

// Views with window-wide shortcuts check this, so Ctrl+S or Ctrl+F only reaches the focused pane.
export function paneFocus(): () => boolean {
  return getContext<(() => boolean) | undefined>(PANE) ?? (() => true)
}
