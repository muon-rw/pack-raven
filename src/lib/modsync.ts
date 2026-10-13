import type { Overview } from './api'

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`

export function modSyncSummary(overview: Overview): string {
  const parts = [
    overview.unlocked.length ? `${plural(overview.unlocked.length, 'mod change', 'mod changes')} to commit` : null,
    overview.missing.length ? `${plural(overview.missing.length, 'mod', 'mods')} missing, pull to restore` : null,
  ].filter(Boolean)
  return parts.length ? parts.join(', ') : 'Mods match the last commit'
}
