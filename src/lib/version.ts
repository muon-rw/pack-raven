export type Bump = 'patch' | 'minor' | 'major'

// We accept versions with fewer parts (1.2), since some packs use them.
export function parseVersion(version: string): number[] | null {
  const match = /^(\d+)(?:\.(\d+))?(?:\.(\d+))?$/.exec(version.trim())
  return match ? [Number(match[1]), Number(match[2] ?? 0), Number(match[3] ?? 0)] : null
}

export function bumpVersion(version: string, kind: Bump): string {
  const parts = parseVersion(version)
  if (!parts) return version
  const [major, minor, patch] = parts
  if (kind === 'major') return `${major + 1}.0.0`
  if (kind === 'minor') return `${major}.${minor + 1}.0`
  return `${major}.${minor}.${patch + 1}`
}
