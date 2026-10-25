export const isMac = /Mac|iPhone|iPad/.test(navigator.userAgent)
export const isWindows = /Windows/.test(navigator.userAgent)

export const fileManager = isMac ? 'Finder' : 'Explorer'

export const trashName = isMac ? 'Trash' : 'Recycle Bin'

// On macOS, Ctrl+click opens the context menu, so Cmd is the only multi-select modifier.
export function commandKey(event: MouseEvent | KeyboardEvent) {
  return isMac ? event.metaKey : event.ctrlKey
}

// macOS lists modifiers in a fixed order (Option, Shift, Command), whatever order we write them in.
const MAC_MODIFIERS: [string, string][] = [
  ['Alt', '⌥'],
  ['Shift', '⇧'],
  ['Ctrl', '⌘'],
]

export function shortcut(keys: string) {
  if (!isMac) return keys
  const parts = keys.split('+')
  const key = parts.pop()!
  const modifiers = MAC_MODIFIERS.filter(([name]) => parts.includes(name)).map(([, glyph]) => glyph)
  return modifiers.join('') + (key === 'Delete' ? '⌘⌫' : key)
}
