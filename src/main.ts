import { mount } from 'svelte'
import '@fontsource-variable/atkinson-hyperlegible-next/wght.css'
import '@fontsource-variable/atkinson-hyperlegible-next/wght-italic.css'
import '@fontsource/jetbrains-mono/400.css'
import './app.css'
import App from './App.svelte'
import { applyTheme } from './lib/theme.svelte'

if (import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)) {
  const { installMock } = await import('./dev/mock')
  installMock()
}

applyTheme()

export default mount(App, { target: document.getElementById('app')! })
