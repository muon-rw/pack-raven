export const nav = $state({ view: null as 'changes' | 'test' | 'git' | 'files' | 'settings' | 'pack' | null, diffPath: null as string | null, filePath: null as string | null, editVersion: false })

export const serverView = $state({ tab: 'mods' as 'mods' | 'properties' | 'startup' })

export function showDiff(path: string) {
  nav.diffPath = path
  nav.view = 'git'
}

export function showInFiles(path: string) {
  nav.filePath = path
  nav.view = 'files'
}
