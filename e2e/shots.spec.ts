import { test } from '@playwright/test'
import { openApp, packViews, scenarios, type Scenario, type View } from './app.ts'

// Captures every view in every scenario at both window widths we design for, in both themes.
// Narrow the grid with comma-separated lists: SHOTS_VIEWS=publish,git SHOTS_SCENARIOS=dirty npm run shots
const pick = <T extends string>(env: string | undefined, all: readonly T[]) => (env ? all.filter((item) => env.split(',').includes(item)) : [...all])
const widths = [960, 1280]
const themes = ['light', 'dark'] as const

for (const scenario of pick(process.env.SHOTS_SCENARIOS, scenarios)) {
  const views: View[] = scenario === 'nopack' ? ['add'] : packViews
  for (const view of pick(process.env.SHOTS_VIEWS, views)) {
    for (const width of widths) {
      for (const theme of themes) {
        test(`${scenario}-${width}-${theme}-${view}`, async ({ page }) => {
          await page.setViewportSize({ width, height: 820 })
          await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' })
          await openApp(page, view, scenario as Scenario)
          await page.screenshot({ path: `shots/${scenario}-${width}-${theme}-${view}.png`, animations: 'disabled' })
        })
      }
    }
  }
}
