import { expect, test } from '@playwright/test'
import { openApp, packViews, scenarios, type Scenario, type View } from './app.ts'

const cases: [View, Scenario][] = [...scenarios.filter((s) => s !== 'nopack').flatMap((s) => packViews.map((v): [View, Scenario] => [v, s])), ['add', 'nopack']]

for (const [view, scenario] of cases) {
  test(`${view} renders in ${scenario}`, async ({ page }) => {
    const problems: string[] = []
    page.on('pageerror', (error) => problems.push(`uncaught: ${error.message}`))
    page.on('console', (message) => {
      // Browsers request a favicon on their own; the Tauri webview doesn't.
      if (message.location().url.endsWith('/favicon.ico')) return
      // The mock logs commands it has no handler for. Views must not need one just to render.
      if (message.type() === 'error' || message.text().startsWith('[mock] unhandled command')) problems.push(message.text())
    })
    await openApp(page, view, scenario)
    expect(problems).toEqual([])
  })
}
