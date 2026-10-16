import type { Page } from '@playwright/test'

// Keep these in sync with the `views` list in App.svelte and `scenarios` in src/dev/mock.ts.
export const headings = {
  changes: 'Changes',
  test: 'Test',
  mods: 'Server mods',
  files: 'Files',
  git: 'Git',
  publish: 'Publish',
  releases: 'Releases',
  pack: 'Pack settings',
  settings: 'App settings',
  add: 'Add your first pack',
}
export type View = keyof typeof headings
export const views = Object.keys(headings) as View[]
export const packViews = views.filter((view) => view !== 'add')

export const scenarios = ['dirty', 'ready', 'empty', 'nopack'] as const
export type Scenario = (typeof scenarios)[number]

export async function openApp(page: Page, view: View, scenario: Scenario) {
  await page.goto(`/?view=${view}&scenario=${scenario}`)
  // At high zoom a broken layout can squeeze the heading to nothing, and we still want that screenshot.
  await page.getByRole('heading', { level: 2, name: headings[view], exact: true }).waitFor({ state: 'attached' })
  await page.waitForLoadState('networkidle')
  await page.evaluate('document.fonts.ready.then(() => true)')
}
