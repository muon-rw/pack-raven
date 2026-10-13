<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import { onMount } from 'svelte'
  import { open } from '@tauri-apps/plugin-dialog'
  import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart'
  import { api, errorText, type DefenderView, type Settings } from '../lib/api'

  let { onchange }: { onchange: () => void } = $props()

  let settings: Settings | null = $state(null)
  let autostart = $state(false)
  let tokens = $state<{ core: boolean; upload: boolean } | null>(null)
  let message: string | null = $state(null)
  let messageFailed = $state(false)
  let settingsError: string | null = $state(null)
  let autostartError: string | null = $state(null)

  async function load() {
    try {
      settings = await api.getSettings()
      settingsError = null
    } catch (error) {
      settingsError = errorText(error)
      return
    }
    // The backend only reports token presence in a pack's overview, so with no pack open we can't show it.
    const overview = await api.overview().catch(() => null)
    tokens = overview && { core: overview.hasCoreKey, upload: overview.hasUploadToken }
  }

  onMount(async () => {
    await load()
    autostart = await isEnabled().catch(() => false)
  })

  async function pickInstall() {
    const chosen = await open({ directory: true, defaultPath: settings?.installDir })
    if (typeof chosen === 'string' && settings) settings.installDir = chosen
  }

  async function save() {
    if (!settings) return
    try {
      await api.saveSettings({ ...settings, java: settings.java?.trim() || null })
      await load()
      onchange()
      messageFailed = false
      message = 'Saved'
    } catch (error) {
      messageFailed = true
      message = `Couldn't save: ${errorText(error)}`
    }
  }

  async function toggleAutostart() {
    const wanted = !autostart
    autostart = wanted
    try {
      autostartError = null
      if (wanted) await enable()
      else await disable()
    } catch (error) {
      autostart = !wanted
      autostartError = `Couldn't change the startup setting: ${errorText(error)}`
    }
  }

  let defender = $state<DefenderView | null>(null)
  let defenderBusy = $state(false)
  let defenderMessage = $state<string | null>(null)

  onMount(async () => {
    defender = await api.defenderStatus().catch(() => null)
  })

  async function excludeFolders() {
    if (!defender) return
    defenderBusy = true
    try {
      await api.defenderExclude(defender.missing)
      defender = await api.defenderStatus()
      defenderMessage = 'Folders excluded.'
    } catch (error) {
      defenderMessage = errorText(error)
    } finally {
      defenderBusy = false
    }
  }

  async function setDefenderPrompt(enabled: boolean) {
    await api.defenderDismiss(!enabled)
    if (settings) settings.defenderDismissed = !enabled
  }
</script>

{#if settingsError}
  <Notice tone="error">
    <p>Couldn't read packraven's settings: {settingsError}</p>
    <p><button onclick={load}>Try again</button></p>
  </Notice>
{:else if settings}
  <div class="settings">
  <header>
    <h2>App settings</h2>
    <p class="muted">For packraven on this machine, across every pack. Each pack's own settings are under Pack settings.</p>
  </header>

  <div class="switch-row">
    <div>
      <div id="autostart-label">Start in the tray when I sign in</div>
      <p class="muted">Keep packraven running so builds and test servers are a click away.</p>
      {#if autostartError}<p class="failed" role="alert">{autostartError}</p>{/if}
    </div>
    <button
      type="button"
      role="switch"
      class="switch"
      aria-checked={autostart}
      aria-labelledby="autostart-label"
      onclick={toggleAutostart}
    ><span></span></button>
  </div>

  <form class="form" onsubmit={(e) => { e.preventDefault(); save() }}>
    <section>
      <h3>This machine</h3>
      <div class="path">
        <label>CurseForge Install folder <input bind:value={settings.installDir} /></label>
        <button type="button" onclick={pickInstall}>Choose</button>
      </div>
      <label>
        Java 21 executable
        <input value={settings.java ?? ''} oninput={(e) => (settings!.java = e.currentTarget.value)} aria-describedby="java-hint" />
      </label>
      <p class="muted hint" id="java-hint">Leave empty to use the Java from the CurseForge profile.</p>
      {#if tokens}
      <ul class="tokens">
        <li class:missing={!tokens.core}>CurseForge API key (CF_DOWNLOAD_TOKEN): {tokens.core ? 'found' : 'not set'}</li>
        <li class:missing={!tokens.upload}>CurseForge upload token (CURSEFORGE_TOKEN): {tokens.upload ? 'found' : 'not set'}</li>
      </ul>
      {#if !tokens.core || !tokens.upload}
        <p class="muted hint">
          packraven reads both from environment variables. Create an API key in the CurseForge for Studios console and an upload token in your CurseForge account's API tokens page, set them as user environment variables, then restart packraven.
        </p>
      {/if}
      {/if}
    </section>

    <div class="actions">
      <button type="submit" class="primary">Save app settings</button>
      <span class:muted={!messageFailed} class:failed={messageFailed} role="status">{message ?? ''}</span>
    </div>
  </form>

  {#if defender?.active}
    <section class="removal">
      <h3>Windows Security</h3>
      {#if defender.missing.length}
        <p class="muted">
          Windows Defender can slow things down. Exclude {defender.missing.length === 1 ? 'this folder' : 'these folders'} to speed things up, but Defender will stop scanning files in {defender.missing.length === 1 ? 'it' : 'them'}.
          {defender.exclusionsReadable ? '' : "Windows only shows exclusions to administrators, so packraven may still list folders you excluded elsewhere."}
        </p>
        <ul class="folders">{#each defender.missing as path (path)}<li class="file">{path}</li>{/each}</ul>
        <div><button type="button" class:busy={defenderBusy} disabled={defenderBusy} onclick={excludeFolders}>{defenderBusy ? 'Waiting for Windows' : 'Exclude folders'}</button></div>
        <p class="muted">You'll get a pop-up from Windows.</p>
      {:else}
        <p class="muted">Defender skips the pack, CurseForge and packraven folders.</p>
      {/if}
      {#if defenderMessage}<p class="muted">{defenderMessage}</p>{/if}
      <label class="check">
        <input type="checkbox" checked={!settings.defenderDismissed} onchange={(e) => setDefenderPrompt(e.currentTarget.checked)} />
        Remind me when these folders aren't excluded
      </label>
    </section>
  {/if}
  </div>
{/if}

<style>
  .settings {
    display: grid;
    gap: 36px;
    max-width: 760px;
  }

  .folders {
    margin: 0;
    padding-left: 18px;
  }

  .removal {
    padding-top: 28px;
    border-top: 1px solid var(--line);
  }

  header p {
    margin-top: 4px;
  }

  .switch-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 24px;
    padding-bottom: 28px;
    border-bottom: 1px solid var(--line);
  }

  .switch {
    position: relative;
    flex: none;
    width: 46px;
    height: 26px;
    padding: 0;
    border-radius: 13px;
    background: var(--line);
  }

  .switch span {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--muted);
    transition: transform 0.15s ease;
  }

  .switch[aria-checked='true'] {
    background: var(--forest);
    border-color: var(--forest);
  }

  .switch[aria-checked='true'] span {
    background: var(--paper);
    transform: translateX(20px);
  }

  .hint {
    margin-top: -8px;
    font-size: var(--step--1);
  }

  .form {
    display: grid;
    gap: 36px;
  }

  section {
    display: grid;
    gap: 14px;
  }

  .path {
    display: flex;
    align-items: end;
    gap: 10px;
  }

  .path label {
    flex: 1;
  }

  .tokens {
    margin: 0;
    padding-left: 18px;
    color: var(--muted);
  }

  .tokens .missing {
    color: var(--warn);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }
</style>
