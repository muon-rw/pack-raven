import type { ModCounts } from './api'

export const LOCKFILE = 'mods.lock.json'

export function modCountsSummary(counts: ModCounts): string | null {
  const parts = [
    counts.updated ? `${counts.updated} ${counts.updated === 1 ? 'mod update' : 'mod updates'}` : null,
    counts.added ? `${counts.added} new` : null,
    counts.removed ? `${counts.removed} removed` : null,
  ].filter(Boolean)
  return parts.length ? parts.join(', ') : null
}

export function notOnCurseforgeSummary(paths: string[]): string {
  return paths.length === 1 ? '1 file not on CurseForge' : `${paths.length} files not on CurseForge`
}
