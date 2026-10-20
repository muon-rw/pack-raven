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
  const freshAnimations = page.getByRole('switch', { name: 'Enable Fresh Animations' })
  await freshAnimations.click()
  await expect(freshAnimations).toHaveAttribute('aria-checked', 'false')
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

  await page.getByRole('button', { name: 'Update Apotheosis', exact: true }).click()
  await expect(page.getByRole('status').filter({ hasText: 'Updated Apotheosis.' })).toBeVisible()

  // Update all opens a review, where disabled mods start unticked.
  await page.getByRole('button', { name: 'Update all (4)' }).click()
  const review = page.getByRole('dialog', { name: 'Review 4 updates' })
  await expect(review.getByRole('checkbox', { name: /Better Clouds/ })).not.toBeChecked()
  await expect(review.getByRole('checkbox', { name: /Sodium/ })).toBeChecked()
  await review.getByRole('button', { name: 'Changelog' }).first().click()
  await expect(review).toContainText('Fixed a crash with Iris')
  await review.getByRole('checkbox', { name: /Sodium/ }).uncheck()
  await review.getByRole('button', { name: 'Update 2 files' }).click()
  await expect(page.getByRole('status').filter({ hasText: /Updated .*Iron's Spells/ })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Update Sodium', exact: true })).toBeVisible()

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
    expect(dialog.message()).toContain("Apotheosis and Iron's Spells 'n Spellbooks require Placebo, directly or through another mod")
    dialog.accept()
  })
  await page.getByRole('button', { name: 'Remove Placebo' }).click()
  await expect(page.getByRole('button', { name: 'Remove Placebo' })).toHaveCount(0)
})

test('search counts matches on every type tab', async ({ page }) => {
  await openApp(page, 'mods', 'ready')
  await page.getByRole('searchbox', { name: 'Find a mod or author' }).fill('complementary')
  const region = page.getByRole('region', { name: 'Installed mods' })
  await expect(region).toContainText('No mods match "complementary"')
  await expect(page.getByRole('group', { name: 'Show' }).getByRole('button', { name: /^Shaders/ })).toContainText('1')
  await region.getByRole('button', { name: 'Show 1 in Shaders' }).click()
  await expect(region.locator('tbody > tr.row')).toHaveCount(1)
})

test('selected mods can be disabled together and brought back', async ({ page }) => {
  await openApp(page, 'mods', 'ready')
  await page.getByRole('checkbox', { name: 'Select Just Enough Items (JEI)' }).check()
  await page.getByRole('checkbox', { name: 'Select Shoulder Surfing Reloaded' }).check()
  const bar = page.getByRole('toolbar', { name: 'Selected files' })
  await expect(bar).toContainText('2 selected')
  await bar.getByRole('button', { name: 'Disable' }).click()
  for (const name of ['Just Enough Items (JEI)', 'Shoulder Surfing Reloaded']) {
    await expect(page.getByRole('switch', { name: `Enable ${name}` })).toHaveAttribute('aria-checked', 'false')
  }
  await page.getByRole('status').getByRole('button', { name: 'Undo' }).click()
  await expect(page.getByRole('switch', { name: 'Enable Just Enough Items (JEI)' })).toHaveAttribute('aria-checked', 'true')

  // An older file is a rollback, and its button says so.
  await page.getByRole('button', { name: 'Placebo', exact: true }).click()
  await expect(page.getByRole('list', { name: 'Versions of Placebo' }).getByRole('button', { name: /^Roll back to/ })).toHaveCount(1)
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
  await expect(page.getByRole('button', { name: /installing Create: Steam 'n' Rails/i })).toContainText('Installing')

  await expect(page.getByRole('status').filter({ hasText: "Installed Create: Steam 'n' Rails. Added Create and Flywheel as required." })).toBeVisible()
  await expect(page.getByRole('button', { name: /^Install / })).toHaveCount(0)
  await expect(page.getByRole('tab', { name: 'Installed' })).toContainText('13')
})

test('updates you click during a download join it', async ({ page }) => {
  await openApp(page, 'mods', 'ready')
  await page.getByRole('button', { name: 'Update Apotheosis', exact: true }).click()
  await expect(page.getByRole('button', { name: 'Updating Apotheosis' })).toHaveAttribute('aria-disabled', 'true')
  await expect(page.getByRole('button', { name: 'Updating Apotheosis' })).toBeFocused()
  await expect(page.getByRole('status').filter({ hasText: 'Downloading 1 file.' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Update all (4)' })).toBeEnabled()

  await page.getByRole('button', { name: 'Update Sodium', exact: true }).click()
  await expect(page.getByRole('button', { name: 'Cancel updating Sodium' })).toBeVisible()

  await expect(page.getByRole('status').filter({ hasText: 'Updated Apotheosis. Updated Sodium.' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Update all (3)' })).toBeEnabled()
})

test('updated mods leave the top of the Update sort when the run ends', async ({ page }) => {
  await openApp(page, 'mods', 'ready')
  const firstRow = page.locator('tbody > tr.row .expand').first()
  await expect(firstRow).toHaveText('Apotheosis')
  await page.getByRole('button', { name: 'Update Apotheosis', exact: true }).click()
  await expect(page.getByRole('status').filter({ hasText: 'Updated Apotheosis.' })).toBeVisible()
  await expect(firstRow).not.toHaveText('Apotheosis')
})

test('a queued update can be cancelled before it starts', async ({ page }) => {
  await openApp(page, 'mods', 'ready')
  await page.getByRole('button', { name: 'Update Apotheosis', exact: true }).click()
  await page.getByRole('button', { name: 'Update Sodium', exact: true }).click()
  await page.getByRole('button', { name: 'Cancel updating Sodium' }).click()
  await expect(page.getByRole('button', { name: 'Update Sodium', exact: true })).toBeVisible()
  await expect(page.getByRole('status').filter({ hasText: 'Updated Apotheosis. Cancelled updating Sodium.' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Update Sodium', exact: true })).toBeVisible()
})

test("a change can't be cancelled once it starts", async ({ page }) => {
  await openApp(page, 'mods', 'ready')
  await page.getByRole('button', { name: 'Update Apotheosis', exact: true }).click()
  await page.getByRole('button', { name: 'Update Sodium', exact: true }).click()
  const running = page.getByRole('button', { name: 'Updating Apotheosis' })
  await expect(running).toHaveAttribute('aria-disabled', 'true')
  await running.click({ force: true })
  // We only reload the list when the run ends, so until then the row says what happened.
  const updated = page.getByRole('button', { name: 'Updated Apotheosis' })
  await expect(updated).toHaveAttribute('aria-disabled', 'true')
  await expect(page.getByRole('status').filter({ hasText: 'Updated Apotheosis. Updated Sodium.' })).toBeVisible()
})

test('cancelling one mod of a queued batch cancels the batch', async ({ page }) => {
  await openApp(page, 'mods', 'ready')
  await page.getByRole('button', { name: 'Update Apotheosis', exact: true }).click()
  await page.getByRole('button', { name: 'Update all (4)' }).click()
  await page.getByRole('dialog').getByRole('button', { name: /^Update \d+ files$/ }).click()
  const cancel = page.getByRole('button', { name: 'Cancel updating Sodium' })
  await expect(cancel).toHaveAttribute('title', /^Cancel this update\. It also cancels /)
  // Pointing at one row's Cancel marks every row it would cancel.
  await cancel.hover()
  await expect(page.getByRole('button', { name: "Cancel updating Iron's Spells 'n Spellbooks" })).toHaveClass(/pointed/)
  await cancel.click()
  await expect(page.getByRole('button', { name: 'Update Sodium', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: "Update Iron's Spells 'n Spellbooks", exact: true })).toBeVisible()
  await expect(page.getByRole('status').filter({ hasText: /Updated Apotheosis\. Cancelled updating Sodium and \d+ other mods?\./ })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Update all (4)' })).toBeEnabled()
})

test('a locked mod stays out of Update all', async ({ page }) => {
  await openApp(page, 'mods', 'ready')
  const lock = page.getByRole('button', { name: 'Lock Apotheosis to this version' })
  await expect(page.getByRole('button', { name: 'Update all (5)' })).toBeEnabled()

  await lock.click()
  await expect(lock).toHaveAttribute('aria-pressed', 'true')
  await expect(page.getByRole('button', { name: 'Update all (4)' })).toBeEnabled()
  await expect(page.getByRole('button', { name: 'Update Apotheosis', exact: true })).toHaveCount(0)
  await expect(page.getByText('Locked; 8.9.0 is available')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Update Apotheosis, locked' })).toHaveAttribute('aria-disabled', 'true')

  await page.getByRole('button', { name: 'Locked', exact: true }).click()
  await expect(page.getByRole('region', { name: 'Installed mods' }).locator('tbody > tr.row').first()).toContainText('Apotheosis')

  await lock.click()
  await expect(page.getByRole('button', { name: 'Update all (5)' })).toBeEnabled()
})

test('disabling a mod others need offers to disable them too', async ({ page }) => {
  await openApp(page, 'mods', 'ready')
  const placebo = page.getByRole('switch', { name: 'Enable Placebo' })
  await expect(page.getByRole('button', { name: 'Update all (5)' })).toBeEnabled()

  await placebo.click()
  const dialog = page.getByRole('dialog', { name: 'Disable Placebo' })
  await expect(dialog.getByRole('list', { name: 'Mods that require Placebo' }).getByRole('listitem')).toHaveText(['Apotheosis', "Iron's Spells 'n Spellbooks"])
  await expect(dialog.getByRole('checkbox')).toBeChecked()
  await dialog.getByRole('button', { name: 'Disable 3 files' }).click()
  for (const name of ['Placebo', 'Apotheosis', "Iron's Spells 'n Spellbooks"]) {
    await expect(page.getByRole('switch', { name: `Enable ${name}` })).toHaveAttribute('aria-checked', 'false')
  }

  // Turning dependents back on is a choice per mod, so enabling Placebo leaves the others off.
  await placebo.click()
  await expect(placebo).toHaveAttribute('aria-checked', 'true')
  await expect(page.getByRole('switch', { name: 'Enable Apotheosis' })).toHaveAttribute('aria-checked', 'false')

  await page.getByRole('switch', { name: 'Enable Apotheosis' }).click()
  await page.getByRole('switch', { name: "Enable Iron's Spells 'n Spellbooks" }).click()
  await placebo.click()
  await dialog.getByRole('checkbox').uncheck()
  await dialog.getByRole('button', { name: 'Disable Placebo only' }).click()
  await expect(placebo).toHaveAttribute('aria-checked', 'false')
  await expect(page.getByRole('switch', { name: 'Enable Apotheosis' })).toHaveAttribute('aria-checked', 'true')
})

test('clicking a version shows its changelog', async ({ page }) => {
  await openApp(page, 'mods', 'ready')
  await page.getByRole('button', { name: 'Apotheosis', exact: true }).click()
  const versions = page.getByRole('list', { name: 'Versions of Apotheosis' })
  const first = versions.getByRole('button', { name: /changelog$/ }).first()
  await first.click()
  await expect(first).toHaveAttribute('aria-expanded', 'true')
  await expect(versions.locator('.changelog')).toContainText('Fixed a crash with Iris')
  await first.click()
  await expect(versions.locator('.changelog')).toHaveCount(0)
})

test('test servers wait until you accept the Minecraft EULA', async ({ page }) => {
  await openApp(page, 'test', 'empty')
  const consent = page.getByRole('status').filter({ hasText: 'Accept the Minecraft EULA' })
  await expect(consent.getByRole('link', { name: 'Read the Minecraft EULA' })).toHaveAttribute('href', 'https://aka.ms/MinecraftEULA')
  for (const name of ['Quick test', 'Start server']) {
    await expect(page.getByRole('button', { name, exact: true })).toHaveAttribute('aria-disabled', 'true')
  }
  await expect(page.getByLabel('Join server')).not.toBeChecked()
  await expect(page.getByRole('button', { name: 'Launch client' })).toHaveAttribute('aria-disabled', 'false')

  await consent.getByRole('button', { name: 'I accept the EULA' }).click()
  await expect(consent).toHaveCount(0)
  for (const name of ['Quick test', 'Start server']) {
    await expect(page.getByRole('button', { name, exact: true })).toHaveAttribute('aria-disabled', 'false')
  }
})

test('Join server stays as you left it when you come back to Test', async ({ page }) => {
  await openApp(page, 'test', 'ready')
  await page.getByLabel('Join server').check()
  const nav = page.getByRole('navigation', { name: 'Pack' })
  await nav.getByRole('button', { name: /^Mods/ }).click()
  await nav.getByRole('button', { name: /^Test/ }).click()
  await expect(page.getByLabel('Join server')).toBeChecked()
})

test("starting a test clears the last run's logs", async ({ page }) => {
  await openApp(page, 'test', 'ready')
  await page.evaluate(() => {
    const emit = (globalThis as unknown as { __mockEmit: (event: string, payload: unknown) => void }).__mockEmit
    emit('job-log', { job: 'server', stream: 'info', text: 'Old server line' })
    emit('job-log', { job: 'client', stream: 'info', text: 'Old client line' })
  })
  await expect(page.getByText('Old server line')).toBeVisible()
  await expect(page.getByText('Old client line')).toBeVisible()
  await page.getByRole('button', { name: 'Quick test' }).click()
  await expect(page.getByText('Syncing changed files to the test server folder')).toBeVisible()
  await expect(page.getByText('Old server line')).toHaveCount(0)
  await expect(page.getByText('Old client line')).toHaveCount(0)
})

test('you can sign in to Minecraft and out again', async ({ page }) => {
  await openApp(page, 'settings', 'empty')
  const section = page.locator('#minecraft-account')
  await section.getByRole('button', { name: 'Sign in with Microsoft' }).click()
  await expect(section).toContainText('Finish signing in with Microsoft in your browser')
  await expect(section.getByRole('button', { name: 'Cancel' })).toBeFocused()
  await expect(section.getByRole('status')).toHaveText('Signed in as muonr.')
  await expect(page.locator('#rail-player')).toHaveText('Minecraft: muonr')
  await expect(section.getByRole('switch', { name: 'Run test servers in online mode' })).toHaveAttribute('aria-checked', 'true')
  await section.getByRole('button', { name: 'Sign out' }).click()
  await expect(section.getByRole('status')).toHaveText('Signed out of Minecraft.')
  await expect(section.getByRole('button', { name: 'Sign in with Microsoft' })).toBeFocused()
})

test('cancelling a Minecraft sign-in is not an error', async ({ page }) => {
  await openApp(page, 'settings', 'empty')
  const section = page.locator('#minecraft-account')
  await section.getByRole('button', { name: 'Sign in with Microsoft' }).click()
  await section.getByRole('button', { name: 'Cancel' }).click()
  await expect(section.getByRole('status')).toHaveText('Sign-in cancelled.')
  await expect(section.getByRole('alert')).toHaveCount(0)
})

test('server properties and startup save as you change them', async ({ page }) => {
  const problems: string[] = []
  page.on('console', (message) => message.type() === 'error' && !message.location().url.endsWith('/favicon.ico') && problems.push(message.text()))
  await openApp(page, 'server', 'dirty')
  await page.getByRole('tab', { name: 'Properties' }).click()
  const entry = page.getByRole('img', { name: /in a player's server list/ })
  await expect(entry).toContainText("Raven's High Fantasy 0.1.0")
  await expect(entry).toContainText('0/12')

  const motd = page.getByLabel('Message of the day')
  await motd.fill('Welcome ')
  await motd.press('End')
  await page.getByRole('button', { name: 'Green, §a' }).click()
  await expect(motd).toHaveValue('Welcome §a')
  await expect(page.getByRole('status').filter({ hasText: 'Saved' })).toBeVisible()

  await page.getByLabel('Difficulty', { exact: true }).selectOption('easy')
  await expect(page.getByRole('button', { name: /^Reset Difficulty/ })).toHaveCount(0)

  await page.getByRole('tab', { name: 'Startup' }).click()
  await page.getByPlaceholder('6G').fill('lots')
  await expect(page.getByText('Enter a size in gigabytes or megabytes')).toBeVisible()
  await page.getByRole('textbox', { name: 'JVM flags' }).fill('-Xmx4G')
  await expect(page.getByText('Set the heap size under Memory instead')).toBeVisible()
  await page.getByRole('textbox', { name: 'Server arguments' }).fill('--world\nMy World')
  await page.getByRole('textbox', { name: 'Server arguments' }).blur()
  await expect(page.getByRole('complementary', { name: 'What hosts run' })).toContainText('nogui --world "My World"')
  expect(problems).toEqual([])
})

test("installing a mod marks the dependencies it brings as installing", async ({ page }) => {
  await openApp(page, 'mods', 'ready')
  await page.getByRole('tab', { name: 'Browse' }).click()
  await page.getByRole('searchbox', { name: 'Search CurseForge' }).fill('create')
  await page.getByRole('button', { name: "Install Create: Steam 'n' Rails" }).click()
  await page.getByRole('button', { name: /^Install all/ }).click()
  await expect(page.getByRole('button', { name: /^(Cancel installing|Installing) Create$/ })).toContainText('Installing')
})
