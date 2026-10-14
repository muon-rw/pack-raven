<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import CredentialField from '../components/CredentialField.svelte'
  import { onMount } from 'svelte'
  import { open } from '@tauri-apps/plugin-dialog'
  import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { api, errorText, type CredentialsView, type DefenderView, type Settings } from '../lib/api'

  let { onchange }: { onchange: () => void } = $props()

  let settings: Settings | null = $state(null)
  let autostart = $state(false)
  let credentials = $state<CredentialsView | null>(null)
  let envNames = $state<string[]>([])
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
    await loadCredentials()
  }

  async function loadCredentials() {
    ;[credentials, envNames] = await Promise.all([api.credentialsStatus(), api.envVarNames()]).catch(() => [null, []])
  }

  const UPLOAD_TOKEN_PAGE = 'https://legacy.curseforge.com/account/api-tokens'

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
      const editorAskMb = Math.max(1, Math.round(settings.editorAskMb || 50))
      const editorMaxMb = Math.max(editorAskMb, Math.round(settings.editorMaxMb || 100))
      await api.saveSettings({ ...settings, java: settings.java?.trim() || null, editorAskMb, editorMaxMb })
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
    </section>

    <section>
      <h3>Editor</h3>
      <label>
        Ask before opening files larger than (MB)
        <input type="number" min="1" step="1" bind:value={settings.editorAskMb} aria-describedby="editor-ask-hint" />
      </label>
      <p class="muted hint" id="editor-ask-hint">Past this size, you can open a file in a simplified editor, without checking or folding, so typing stays responsive.</p>
      <label>
        Never open files larger than (MB)
        <input type="number" min="1" step="1" bind:value={settings.editorMaxMb} aria-describedby="editor-max-hint" />
      </label>
      <p class="muted hint" id="editor-max-hint">Larger files can run the editor out of memory, so we offer your default app for them instead.</p>
    </section>

    <div class="actions">
      <button type="submit" class="primary">Save app settings</button>
      <span class:muted={!messageFailed} class:failed={messageFailed} role="status">{message ?? ''}</span>
    </div>
  </form>

  {#if credentials}
    <section class="removal">
      <h3>CurseForge access</h3>
      <p class="muted">Changes here take effect right away, including in the git hooks.</p>
      <CredentialField credential="download" label="CurseForge API key (a.k.a. Launcher Token)" status={credentials.download} keychain={credentials.keychain} {envNames} onchange={loadCredentials}>
        {#snippet help()}
          <p class="muted hint">packraven uses this to look up and download mods. For now, you'll need to provide your own.</p>
        {/snippet}
      </CredentialField>
      <CredentialField credential="upload" label="CurseForge upload token" status={credentials.upload} keychain={credentials.keychain} {envNames} onchange={loadCredentials}>
        {#snippet help()}
          <p class="muted hint">
            You only need this to publish. <a href={UPLOAD_TOKEN_PAGE} onclick={(e) => { e.preventDefault(); openUrl(UPLOAD_TOKEN_PAGE) }}>Generate a token on CurseForge</a>, then paste it here.
          </p>
        {/snippet}
      </CredentialField>
    </section>
  {/if}

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

  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }
</style>
