import type { AddonKind, ModCheck, ModFile, ReleaseType, SearchSort } from './api'

const stability: Record<ReleaseType, number> = { release: 1, beta: 2, alpha: 3 }

// We keep the last check for the session, so leaving the Mods view doesn't mean asking CurseForge again.
export const modUpdates = $state({
  pack: null as string | null,
  channel: 'release' as ReleaseType,
  // We replace the set on each change, since Svelte doesn't track changes inside a Set.
  locked: new Set<number>(),
  checks: null as Map<string, ModCheck> | null,
  checkedAt: 0,
})

export function newestOnChannel(check: ModCheck | undefined): ModFile | undefined {
  return check?.newer.find((file) => stability[file.releaseType] <= stability[modUpdates.channel])
}

export function availableUpdate(check: ModCheck | undefined): ModFile | undefined {
  return check && modUpdates.locked.has(check.projectId) ? undefined : newestOnChannel(check)
}

export function updateCount(pack: string | null): number {
  if (!modUpdates.checks || modUpdates.pack !== pack) return 0
  let count = 0
  for (const check of modUpdates.checks.values()) if (availableUpdate(check)) count++
  return count
}

export const browse = $state({
  tab: 'installed' as 'installed' | 'browse',
  text: '',
  kind: 'mods' as AddonKind,
  categoryId: null as number | null,
  sort: 'relevance' as SearchSort,
})
