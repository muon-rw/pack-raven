// Pane sizes are a per-machine preference, so we keep them in the webview's storage rather than in
// packraven.toml. Without storage, a size lasts until you close packraven.
export class StoredSplit {
  value = $state<number | null>(null)

  constructor(private key: string) {
    try {
      const stored = Number(localStorage.getItem(key) ?? NaN)
      if (Number.isFinite(stored)) this.value = stored
    } catch {}
  }

  set(value: number) {
    this.value = value
    try {
      localStorage.setItem(this.key, String(value))
    } catch {}
  }

  reset() {
    this.value = null
    try {
      localStorage.removeItem(this.key)
    } catch {}
  }
}

export function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), Math.max(min, max))
}
