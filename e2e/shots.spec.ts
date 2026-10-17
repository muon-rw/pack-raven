import { test, type Page } from '@playwright/test'
import { openApp, packViews, scenarios, type Scenario, type View } from './app.ts'

// Captures every view in every scenario at the window sizes we design for, in both themes.
// Narrow the grid with comma-separated lists: SHOTS_VIEWS=publish,git SHOTS_SCENARIOS=dirty npm run shots
const pick = <T extends string>(env: string | undefined, all: readonly T[]) => (env ? all.filter((item) => env.split(',').includes(item)) : [...all])
const sizes = [
  { width: 960, height: 640 },
  { width: 1280, height: 820 },
]
const themes = ['light', 'dark'] as const

// The zoom hotkeys shrink the layout, so a minimum-size window at 125% lays out at 768x512. That's
// the most zoom we design for. Zoom doesn't change the theme's layout, so we capture it in light only.
const zooms = [125]

// Tabs the URL can't open, captured as <view>-<tab>.png after the view's own shot.
const tabs: Partial<Record<View, string[]>> = { server: ['Properties', 'Startup'] }

async function shootTabs(page: Page, view: View, path: (name: string) => string) {
  for (const tab of tabs[view] ?? []) {
    await page.getByRole('tab', { name: tab }).click()
    await page.screenshot({ path: path(`${view}-${tab.toLowerCase()}`), animations: 'disabled' })
  }
}

function views(scenario: Scenario) {
  return pick(process.env.SHOTS_VIEWS, scenario === 'nopack' ? (['add'] as View[]) : packViews)
}

for (const scenario of pick(process.env.SHOTS_SCENARIOS, scenarios)) {
  for (const view of views(scenario)) {
    for (const { width, height } of sizes) {
      for (const theme of themes) {
        test(`${scenario}-${width}-${theme}-${view}`, async ({ page }) => {
          await page.setViewportSize({ width, height })
          await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' })
          await openApp(page, view, scenario)
          await page.screenshot({ path: `shots/${scenario}-${width}-${theme}-${view}.png`, animations: 'disabled' })
          await shootTabs(page, view, (name) => `shots/${scenario}-${width}-${theme}-${name}.png`)
        })
      }
    }
  }
}

for (const zoom of zooms) {
  test.describe(`${zoom}% zoom`, () => {
    test.use({ deviceScaleFactor: zoom / 100, viewport: { width: Math.round(96000 / zoom), height: Math.round(64000 / zoom) } })
    for (const scenario of pick(process.env.SHOTS_SCENARIOS, scenarios)) {
      for (const view of views(scenario)) {
        test(`${scenario}-960@${zoom}-light-${view}`, async ({ page }) => {
          await page.emulateMedia({ colorScheme: 'light', reducedMotion: 'reduce' })
          await openApp(page, view, scenario)
          await page.screenshot({ path: `shots/${scenario}-960@${zoom}-light-${view}.png`, animations: 'disabled' })
          await shootTabs(page, view, (name) => `shots/${scenario}-960@${zoom}-light-${name}.png`)
        })
      }
    }
  })
}
