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

test('mods can be filtered, updated and removed', async ({ page }) => {
  await openApp(page, 'mods', 'dirty')
  const rows = page.getByRole('region', { name: 'Mods' }).locator('tbody > tr')
  await expect(page.getByRole('button', { name: 'Update all (4)' })).toBeEnabled()
  await expect(page.getByRole('status').filter({ hasText: 'two versions of Iris Shaders' })).toBeVisible()

  await page.getByRole('button', { name: /^Updates/ }).click()
  await expect(rows).toHaveCount(4)

  await page.getByRole('button', { name: 'Versions of Apotheosis' }).click()
  await expect(page.getByRole('list', { name: 'Versions of Apotheosis' }).getByRole('listitem')).toHaveCount(3)

  await page.getByRole('button', { name: 'Update Apotheosis' }).click()
  await expect(page.getByRole('status').filter({ hasText: 'Updated 1 mod.' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Update all (3)' })).toBeEnabled()

  await page.getByRole('button', { name: 'All', exact: true }).click()
  page.once('dialog', (dialog) => {
    expect(dialog.message()).toContain('Apotheosis requires Placebo')
    dialog.accept()
  })
  await page.getByRole('button', { name: 'Remove Placebo' }).click()
  await expect(page.getByRole('button', { name: 'Remove Placebo' })).toHaveCount(0)
})
