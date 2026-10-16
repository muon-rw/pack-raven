import { test } from '@playwright/test'
import { openApp, packViews, scenarios, type Scenario, type View } from './app.ts'

// Captures every view in every scenario at both window widths we design for, in both themes.
// Narrow the grid with comma-separated lists: SHOTS_VIEWS=publish,git SHOTS_SCENARIOS=dirty npm run shots
const pick = <T extends string>(env: string | undefined, all: readonly T[]) => (env ? all.filter((item) => env.split(',').includes(item)) : [...all])
const widths = [960, 1280]
const themes = ['light', 'dark'] as const

// The zoom hotkeys shrink the layout width, so a minimum-size window at 200% lays out at 480 px.
// Zoom doesn't change the theme's layout, so we capture it in light only.
const zooms = [125, 150, 200]

function views(scenario: Scenario) {
  return pick(process.env.SHOTS_VIEWS, scenario === 'nopack' ? (['add'] as View[]) : packViews)
}

for (const scenario of pick(process.env.SHOTS_SCENARIOS, scenarios)) {
  for (const view of views(scenario)) {
    for (const width of widths) {
      for (const theme of themes) {
        test(`${scenario}-${width}-${theme}-${view}`, async ({ page }) => {
          await page.setViewportSize({ width, height: 820 })
          await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' })
          await openApp(page, view, scenario)
          await page.screenshot({ path: `shots/${scenario}-${width}-${theme}-${view}.png`, animations: 'disabled' })
        })
      }
    }
  }
}

for (const zoom of zooms) {
  test.describe(`${zoom}% zoom`, () => {
    test.use({ deviceScaleFactor: zoom / 100, viewport: { width: Math.round(96000 / zoom), height: Math.round(82000 / zoom) } })
    for (const scenario of pick(process.env.SHOTS_SCENARIOS, scenarios)) {
      for (const view of views(scenario)) {
        test(`${scenario}-960@${zoom}-light-${view}`, async ({ page }) => {
          await page.emulateMedia({ colorScheme: 'light', reducedMotion: 'reduce' })
          await openApp(page, view, scenario)
          await page.screenshot({ path: `shots/${scenario}-960@${zoom}-light-${view}.png`, animations: 'disabled' })
        })
      }
    }
  })
}
