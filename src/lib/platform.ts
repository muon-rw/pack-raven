export const isMac = /Mac|iPhone|iPad/.test(navigator.userAgent)
export const isWindows = /Windows/.test(navigator.userAgent)

export const fileManager = isMac ? 'Finder' : 'Explorer'

export const trashName = isMac ? 'Trash' : 'Recycle Bin'

// On macOS, Ctrl+click opens the context menu, so Cmd is the only multi-select modifier.
export function commandKey(event: MouseEvent | KeyboardEvent) {
  return isMac ? event.metaKey : event.ctrlKey
}

export function shortcut(keys: string) {
  return isMac ? keys.replace('Ctrl+', '⌘').replace('Shift+', '⇧').replace('Delete', '⌘⌫') : keys
}
