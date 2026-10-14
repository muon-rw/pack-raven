import { getCurrentWindow } from '@tauri-apps/api/window'
import { api } from './api'

type Theme = 'light' | 'dark'
export type ThemeMode = Theme | 'auto'

const KEY = 'packraven-theme'
const MODES: ThemeMode[] = ['light', 'dark', 'auto']

// The webview answers this query from the window theme, so it only reports the OS theme once
// we hand the window back to the OS with setTheme(null). We read it in auto mode only.
const systemDark = matchMedia('(prefers-color-scheme: dark)')

function storedMode(): ThemeMode {
  const stored = localStorage.getItem(KEY)
  return stored === 'light' || stored === 'dark' ? stored : 'auto'
}

function resolve(mode: ThemeMode): Theme {
  if (mode !== 'auto') return mode
  return systemDark.matches ? 'dark' : 'light'
}

const initialMode = storedMode()

export const theme = $state<{ mode: ThemeMode; current: Theme }>({
  mode: initialMode,
  current: resolve(initialMode),
})

let appliedIcon: Theme | null = null

export function applyTheme() {
  theme.current = resolve(theme.mode)
  document.documentElement.dataset.theme = theme.current
  getCurrentWindow().setTheme(theme.mode === 'auto' ? null : theme.mode).catch(() => {})
  if (appliedIcon !== theme.current) {
    appliedIcon = theme.current
    api.setIconTheme(theme.current === 'dark').catch(() => {})
  }
}

export function cycleThemeMode() {
  theme.mode = MODES[(MODES.indexOf(theme.mode) + 1) % MODES.length]
  if (theme.mode === 'auto') localStorage.removeItem(KEY)
  else localStorage.setItem(KEY, theme.mode)
  applyTheme()
}

systemDark.addEventListener('change', () => {
  if (theme.mode === 'auto') applyTheme()
})
