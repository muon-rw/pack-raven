import type { Page } from '@playwright/test'

// Keep these in sync with the `views` list in src/lib/panes.svelte.ts and `scenarios` in src/dev/mock.ts.
export const headings = {
  changes: 'Changes',
  test: 'Run',
  mods: 'Mods',
  server: 'Server',
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
// Most views show only an error in newerlock, so it stays out of the every-view runs.
export type Scenario = (typeof scenarios)[number] | 'newerlock'

// Pass `beside` to open a second view in a split.
export async function openApp(page: Page, view: View, scenario: Scenario, beside?: View) {
  await page.goto(`/?view=${view}&scenario=${scenario}${beside ? `&beside=${beside}` : ''}`)
  // At high zoom a broken layout can squeeze the heading to nothing, and we still want that screenshot.
  // A split too narrow for two panes shows them as tabs, so one heading is in a hidden panel.
  for (const shown of beside ? [view, beside] : [view]) {
    await page.getByRole('heading', { level: 2, name: headings[shown], exact: true, includeHidden: true }).waitFor({ state: 'attached' })
  }
  await page.waitForLoadState('networkidle')
  await page.evaluate('document.fonts.ready.then(() => true)')
}
