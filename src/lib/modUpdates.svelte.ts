import type { ModCheck, ModFile, ReleaseType } from './api'

const stability: Record<ReleaseType, number> = { release: 1, beta: 2, alpha: 3 }

// We keep the last check for the session, so leaving the Mods view doesn't mean asking CurseForge again.
export const modUpdates = $state({
  pack: null as string | null,
  channel: 'release' as ReleaseType,
  checks: null as Map<string, ModCheck> | null,
  checkedAt: 0,
})

export function availableUpdate(check: ModCheck | undefined, channel: ReleaseType): ModFile | undefined {
  return check?.newer.find((file) => stability[file.releaseType] <= stability[channel])
}

export function updateCount(pack: string | null): number {
  if (!modUpdates.checks || modUpdates.pack !== pack) return 0
  let count = 0
  for (const check of modUpdates.checks.values()) if (availableUpdate(check, modUpdates.channel)) count++
  return count
}
