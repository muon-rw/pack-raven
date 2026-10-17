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

test('installed mods can be sorted, filtered, expanded and changed', async ({ page }) => {
  await openApp(page, 'mods', 'dirty')
  const rows = page.getByRole('region', { name: 'Installed mods' }).locator('tbody > tr.row')
  await expect(page.getByRole('button', { name: 'Update all (5)' })).toBeEnabled()
  await expect(page.getByRole('status').filter({ hasText: 'two versions of Iris Shaders' })).toBeVisible()

  // Mods with updates come first until you pick another sort.
  await expect(rows.first().getByRole('button', { name: /^Update / })).toBeVisible()
  await page.getByRole('button', { name: 'Author' }).click()
  await expect(rows.first()).toContainText('coderbot')
  await page.getByRole('button', { name: 'Update', exact: true }).click()

  await page.getByRole('button', { name: /^Resource packs/ }).click()
  await expect(rows).toHaveCount(1)
  await page.getByRole('button', { name: /^Data packs/ }).click()
  await expect(page.getByRole('region', { name: 'Installed mods' })).toContainText('no data packs from CurseForge yet')
  await page.getByRole('group', { name: 'Show' }).getByRole('button', { name: /^Mods/ }).click()

  // Clicking a row away from its controls opens its versions and description.
  await rows.filter({ hasText: 'Apotheosis' }).locator('td.author').click()
  await expect(page.getByRole('list', { name: 'Versions of Apotheosis' }).getByRole('listitem')).toHaveCount(3)
  await page.getByRole('button', { name: 'Description', exact: true }).click()
  const description = page.locator('.description')
  await expect(description).toContainText('content mod')
  await expect(description.locator('iframe, script')).toHaveCount(0)
  await expect(description.locator('span')).not.toHaveAttribute('style', /color/)

  await page.getByRole('button', { name: 'Update Apotheosis' }).click()
  await expect(page.getByRole('status').filter({ hasText: 'Updated 1 mod.' })).toBeVisible()

  page.once('dialog', (dialog) => {
    expect(dialog.message()).toContain('Sodium: sodium-neoforge-0.8.13+mc1.21.1.jar')
    dialog.accept()
  })
  await page.getByRole('button', { name: 'Update all (4)' }).click()
  await expect(page.getByRole('status').filter({ hasText: 'Updated 4 mods.' })).toBeVisible()

  const betterClouds = page.getByRole('switch', { name: 'Enable Better Clouds' })
  await expect(betterClouds).toHaveAttribute('aria-checked', 'false')
  await betterClouds.click()
  await expect(betterClouds).toHaveAttribute('aria-checked', 'true')

  // Scrolling closes the menu, so we bring the row into view before right-clicking it.
  await rows.filter({ hasText: 'Placebo' }).scrollIntoViewIfNeeded()
  await rows.filter({ hasText: 'Placebo' }).click({ button: 'right' })
  await expect(page.getByRole('menuitem', { name: 'Copy link' })).toBeVisible()
  await page.keyboard.press('Escape')

  page.once('dialog', (dialog) => {
    expect(dialog.message()).toContain('Apotheosis requires Placebo')
    dialog.accept()
  })
  await page.getByRole('button', { name: 'Remove Placebo' }).click()
  await expect(page.getByRole('button', { name: 'Remove Placebo' })).toHaveCount(0)
})

test('mods can be found on CurseForge and installed with their dependencies', async ({ page }) => {
  await openApp(page, 'mods', 'ready')
  await page.getByRole('tab', { name: 'Browse' }).click()
  await expect(page.getByRole('region', { name: 'Search results' }).getByText('Installed', { exact: true })).not.toHaveCount(0)

  await page.getByRole('searchbox', { name: 'Search CurseForge' }).fill('steam')
  await expect(page.getByRole('button', { name: /^Install / })).toHaveCount(1)
  await page.getByRole('button', { name: "Install Create: Steam 'n' Rails" }).click()
  await expect(page.getByRole('group', { name: "Install Create: Steam 'n' Rails" })).toContainText('requires Create and Flywheel')
  await page.getByRole('button', { name: 'Install all 3' }).click()

  await expect(page.getByRole('status').filter({ hasText: "Installed Create: Steam 'n' Rails. Added Create and Flywheel as required." })).toBeVisible()
  await expect(page.getByRole('button', { name: /^Install / })).toHaveCount(0)
  await expect(page.getByRole('tab', { name: 'Installed' })).toContainText('13')
})

test('a locked mod stays out of Update all', async ({ page }) => {
  await openApp(page, 'mods', 'ready')
  const lock = page.getByRole('button', { name: 'Lock Apotheosis to this version' })
  await expect(page.getByRole('button', { name: 'Update all (5)' })).toBeEnabled()

  await lock.click()
  await expect(lock).toHaveAttribute('aria-pressed', 'true')
  await expect(page.getByRole('button', { name: 'Update all (4)' })).toBeEnabled()
  await expect(page.getByRole('button', { name: 'Update Apotheosis' })).toHaveCount(0)
  await expect(page.getByText('Locked. Apotheosis-1.21.1-8.9.0.jar is available.')).toBeVisible()

  await page.getByRole('button', { name: 'Locked' }).click()
  await expect(page.getByRole('region', { name: 'Installed mods' }).locator('tbody > tr.row').first()).toContainText('Apotheosis')

  await lock.click()
  await expect(page.getByRole('button', { name: 'Update all (5)' })).toBeEnabled()
})
