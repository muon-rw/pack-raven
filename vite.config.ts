import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [svelte()],
  // The test server runs beside your own dev server, and two servers sharing one dependency cache
  // rebundle each other's dependencies and reload pages mid-test.
  cacheDir: process.env.PACKRAVEN_VITE_CACHE ?? 'node_modules/.vite',
})
