<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import { onMount } from 'svelte'
  import { ask, open } from '@tauri-apps/plugin-dialog'
  import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart'
  import { api, errorText, type PackConfig, type Settings } from '../lib/api'

  let { onchange }: { onchange: () => void } = $props()

  let settings: Settings | null = $state(null)
  let config: PackConfig | null = $state(null)
  let configError: string | null = $state(null)
  let autostart = $state(false)
  let tokens = $state({ core: false, upload: false })
  let message: string | null = $state(null)
  let messageFailed = $state(false)
  let settingsError: string | null = $state(null)
  let autostartError: string | null = $state(null)
  let removeError: string | null = $state(null)

  async function load() {
    try {
      settings = await api.getSettings()
      settingsError = null
    } catch (error) {
      settingsError = errorText(error)
      return
    }
    try {
      config = await api.getPackConfig()
      configError = null
      const overview = await api.overview()
      tokens = { core: overview.hasCoreKey, upload: overview.hasUploadToken }
    } catch (error) {
      config = null
      configError = errorText(error)
    }
  }

  onMount(async () => {
    await load()
    autostart = await isEnabled().catch(() => false)
  })

  async function pickInstall() {
    const chosen = await open({ directory: true, defaultPath: settings?.installDir })
    if (typeof chosen === 'string' && settings) settings.installDir = chosen
  }

  async function removePack() {
    if (!settings) return
    const confirmed = await ask(`Remove this pack from packraven? The instance folder, its files and its git repository stay as they are.`, {
      title: 'Remove pack',
      kind: 'warning',
      okLabel: 'Remove pack',
      cancelLabel: 'Cancel',
    })
    if (!confirmed) return
    try {
      removeError = null
      await api.removePack(settings.instanceDir)
      onchange()
    } catch (error) {
      removeError = `Couldn't remove the pack: ${errorText(error)}`
    }
  }

  async function save() {
    if (!settings) return
    try {
      await api.saveSettings({ ...settings, java: settings.java?.trim() || null })
      if (config) await api.savePackConfig(config)
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
</script>

{#if settingsError}
  <Notice tone="error">
    <p>Couldn't read packraven's settings: {settingsError}</p>
    <p><button onclick={load}>Try again</button></p>
  </Notice>
{:else if settings}
  <div class="settings">
  <div class="switch-row">
    <div>
      <div id="autostart-label">Start in the tray when I sign in</div>
      <p class="muted">Keeps packraven running so builds and test servers are a click away.</p>
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
      <h2>This machine</h2>
      <div class="path">
        <label>CurseForge Install folder <input bind:value={settings.installDir} /></label>
        <button type="button" onclick={pickInstall}>Choose</button>
      </div>
      <label>
        Java 21 executable
        <input value={settings.java ?? ''} oninput={(e) => (settings!.java = e.currentTarget.value)} aria-describedby="java-hint" />
      </label>
      <p class="muted hint" id="java-hint">Leave empty to use the Java from the CurseForge profile.</p>
      <ul class="tokens">
        <li class:missing={!tokens.core}>CurseForge API key (CF_DOWNLOAD_TOKEN): {tokens.core ? 'found' : 'not set'}</li>
        <li class:missing={!tokens.upload}>CurseForge upload token (CURSEFORGE_TOKEN): {tokens.upload ? 'found' : 'not set'}</li>
      </ul>
      {#if !tokens.core || !tokens.upload}
        <p class="muted hint">
          Both are read from environment variables. Create an API key in the CurseForge for Studios console and an upload token in your CurseForge account's API tokens page, set them as user environment variables, then restart packraven.
        </p>
      {/if}
    </section>

    {#if configError}
      <Notice tone="error"><p>{configError}</p></Notice>
    {:else if config}
      <section>
        <h2>Pack</h2>
        <p class="muted">Stored in packraven.toml in the pack repo, so both machines share it.</p>
        <div class="grid">
          <label>CurseForge project ID <input type="number" bind:value={config.curseforge.project_id} /></label>
          <label>
            Release type
            <select bind:value={config.curseforge.release_type}>
              <option value="release">Release</option>
              <option value="beta">Beta</option>
              <option value="alpha">Alpha</option>
            </select>
          </label>
          <label>Author <input bind:value={config.author} /></label>
          <label>Server memory <input bind:value={config.server.memory} placeholder="4G" title="Java heap size, such as 4G or 6144M" /></label>
          <label>Test username <input bind:value={config.test.username} /></label>
          <label>Test server port <input type="number" bind:value={config.test.port} /></label>
        </div>
        <label class="check">
          <input type="checkbox" bind:checked={config.curseforge.manual_release} /> Hold approved files until I release them on CurseForge
        </label>
      </section>
    {/if}

    <div class="actions">
      <button type="submit" class="primary">Save settings</button>
      <span class:muted={!messageFailed} class:failed={messageFailed} role="status">{message ?? ''}</span>
    </div>
  </form>

  {#if settings.instanceDir}
    <section class="removal">
      <h2>Remove pack</h2>
      <p class="muted">Stops showing <span class="file">{settings.instanceDir}</span> in packraven. Nothing on disk is deleted.</p>
      <div><button type="button" class="danger" onclick={removePack}>Remove pack</button></div>
      {#if removeError}<p class="failed" role="alert">{removeError}</p>{/if}
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

  .removal {
    padding-top: 28px;
    border-top: 1px solid var(--quill);
  }

  .switch-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 24px;
    padding-bottom: 28px;
    border-bottom: 1px solid var(--quill);
  }

  .switch {
    position: relative;
    flex: none;
    width: 46px;
    height: 26px;
    padding: 0;
    border-radius: 13px;
    background: var(--quill);
  }

  .switch span {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--ash);
    transition: transform 0.15s ease;
  }

  .switch[aria-checked='true'] {
    background: var(--teal);
    border-color: var(--teal);
  }

  .switch[aria-checked='true'] span {
    background: var(--ink);
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

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 14px 18px;
  }

  .tokens {
    margin: 0;
    padding-left: 18px;
    color: var(--ash);
  }

  .tokens .missing {
    color: var(--amber);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }
</style>
