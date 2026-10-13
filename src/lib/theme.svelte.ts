import { getCurrentWindow } from '@tauri-apps/api/window'

type Theme = 'light' | 'dark'

const KEY = 'packraven-theme'

export const theme = $state<{ current: Theme }>({
  current: (localStorage.getItem(KEY) as Theme | null) ?? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'),
})

export function applyTheme() {
  document.documentElement.dataset.theme = theme.current
  getCurrentWindow().setTheme(theme.current).catch(() => {})
}

export function toggleTheme() {
  theme.current = theme.current === 'dark' ? 'light' : 'dark'
  localStorage.setItem(KEY, theme.current)
  applyTheme()
}
