import { mount } from 'svelte'
import '@fontsource-variable/atkinson-hyperlegible-next/wght.css'
import '@fontsource/jetbrains-mono/400.css'
import './app.css'
import App from './App.svelte'

export default mount(App, { target: document.getElementById('app')! })
