export const nav = $state({ view: null as 'git' | null, diffPath: null as string | null })

export function showDiff(path: string) {
  nav.diffPath = path
  nav.view = 'git'
}
