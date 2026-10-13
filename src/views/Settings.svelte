<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import { onMount } from 'svelte'
  import { ask, open } from '@tauri-apps/plugin-dialog'
  import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart'
  import { api, errorText, type DefenderView, type PackConfig, type Settings } from '../lib/api'

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
      webhook = (await api.getWebhook()) ?? ''
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
      await api.setWebhook(webhook)
      await load()
      onchange()
      messageFailed = false
      message = 'Saved'
    } catch (error) {
      messageFailed = true
      message = `Couldn't save: ${errorText(error)}`
    }
  }

  let webhook = $state('')
  let discordMessage = $state<string | null>(null)
  let discordFailed = $state(false)
  let testing = $state(false)

  async function testWebhook() {
    testing = true
    try {
      await api.setWebhook(webhook)
      await api.discordTest()
      discordFailed = false
      discordMessage = 'Sent. Check the channel.'
    } catch (error) {
      discordFailed = true
      discordMessage = errorText(error)
    } finally {
      testing = false
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

      <section>
        <h2>Discord announcements</h2>
        <div class="field">
          <label for="webhook">Webhook URL</label>
          <div class="inline">
            <input id="webhook" type="password" autocomplete="off" bind:value={webhook} placeholder="https://discord.com/api/webhooks/..." aria-describedby="webhook-hint" />
            <button type="button" class:busy={testing} disabled={testing || !webhook.trim()} onclick={testWebhook}>{testing ? 'Sending' : 'Send test message'}</button>
          </div>
          <p class="hint" id="webhook-hint">From the channel's Settings, Integrations, Webhooks. Saved only on this machine, never in the pack repo.</p>
          {#if discordMessage}<p class:muted={!discordFailed} class:failed={discordFailed} role="status">{discordMessage}</p>{/if}
        </div>
        <div class="grid">
          <label>Role to ping <input bind:value={config.discord.role_id} placeholder="Role ID" title="Right-click the role with Developer Mode on, then Copy Role ID. Leave empty for no ping." /></label>
          <label>Embed color <input type="color" bind:value={config.discord.color} /></label>
        </div>
        <label class="block">
          Title
          <input bind:value={config.discord.title} placeholder={'{project_name} - v{version}'} aria-describedby="discord-placeholders" />
        </label>
        <label class="block">
          Text above the changelog
          <textarea rows="3" bind:value={config.discord.header} aria-describedby="discord-placeholders"></textarea>
        </label>
        <label class="block">
          Text below the changelog
          <textarea rows="4" bind:value={config.discord.footer} placeholder={'[Install]({file_url})\n**Back up your world before updating**'} aria-describedby="discord-placeholders"></textarea>
        </label>
        <p class="hint" id="discord-placeholders">
          Discord markdown, used in every announcement. <code>{'{file_url}'}</code> links to the released file, <code>{'{project_url}'}</code> to the project page. <code>{'{project_name}'}</code> is the CurseForge project's name, <code>{'{name}'}</code> the instance name, and <code>{'{version}'}</code> the version. The project logo is used as the thumbnail.
        </p>
        <label class="check">
          <input type="checkbox" bind:checked={config.discord.show_author} /> Show the project author, with their CurseForge avatar and a link to their profile
        </label>
        <label class="check">
          <input type="checkbox" bind:checked={config.discord.announce_on_approval} /> Announce each release once CurseForge approves it
        </label>
        <p class="hint">packraven checks every few minutes while it's running in the tray. You can also announce a release by hand from Publish.</p>
      </section>
    {/if}

    <div class="actions">
      <button type="submit" class="primary">Save settings</button>
      <span class:muted={!messageFailed} class:failed={messageFailed} role="status">{message ?? ''}</span>
    </div>
  </form>

  {#if defender?.active}
    <section class="removal">
      <h2>Windows Security</h2>
      {#if defender.missing.length}
        <p class="muted">
          Windows Defender can slow things down. Excluding {defender.missing.length === 1 ? 'this folder makes it' : 'these folders makes them'} faster, but Defender will stop scanning files in {defender.missing.length === 1 ? 'it' : 'them'}.
          {defender.exclusionsReadable ? '' : "Windows only shows exclusions to administrators, so folders you excluded outside packraven may still be listed."}
        </p>
        <ul class="folders">{#each defender.missing as path (path)}<li class="file">{path}</li>{/each}</ul>
        <div><button type="button" class:busy={defenderBusy} disabled={defenderBusy} onclick={excludeFolders}>{defenderBusy ? 'Waiting for Windows' : 'Exclude folders'}</button></div>
        <p class="muted">You'll get a pop-up from Windows.</p>
      {:else}
        <p class="muted">The pack, CurseForge and packraven folders are excluded from Defender scans.</p>
      {/if}
      {#if defenderMessage}<p class="muted">{defenderMessage}</p>{/if}
      <label class="check">
        <input type="checkbox" checked={!settings.defenderDismissed} onchange={(e) => setDefenderPrompt(e.currentTarget.checked)} />
        Remind me when these folders aren't excluded
      </label>
    </section>
  {/if}

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
  input[type='color'] {
    width: 52px;
    height: 38px;
    padding: 3px;
    cursor: pointer;
  }

  .field {
    display: grid;
    gap: 6px;
  }

  .field > label {
    color: var(--text);
    font-size: var(--step-0);
  }

  .inline {
    display: flex;
    gap: 8px;
  }

  .inline input {
    flex: 1;
  }

  .hint {
    color: var(--muted);
    font-size: var(--step--1);
  }

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

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 14px 18px;
  }

  .block {
    display: grid;
    gap: 6px;
  }

  .block textarea {
    font-family: var(--font-log);
    font-size: var(--step--1);
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
