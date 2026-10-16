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
  let credentialsError: string | null = $state(null)
  let saving = $state(false)

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
    try {
      ;[credentials, envNames] = await Promise.all([api.credentialsStatus(), api.envVarNames()])
      credentialsError = null
    } catch (error) {
      credentials = null
      credentialsError = errorText(error)
    }
  }

  const UPLOAD_TOKEN_PAGE = 'https://legacy.curseforge.com/account/api-tokens'
  const API_KEY_PAGE = 'https://console.curseforge.com/'
  const GITHUB_TOKEN_PAGE = 'https://github.com/settings/personal-access-tokens/new'

  let identityBusy = $state(false)
  let identityMessage = $state<{ ok: boolean; text: string } | null>(null)

  async function fillIdentityFromGithub() {
    if (!settings) return
    identityBusy = true
    try {
      const identity = await api.githubIdentity()
      settings.gitName = identity.name
      settings.gitEmail = identity.email
      identityMessage = { ok: true, text: 'Filled in from your GitHub account. Save app settings to keep it.' }
    } catch (error) {
      identityMessage = { ok: false, text: `Couldn't read your GitHub account: ${errorText(error)}` }
    } finally {
      identityBusy = false
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

  const JAVA_SLOTS = [
    { major: '17', versions: 'Minecraft 1.18 to 1.20.4 (Java 17)' },
    { major: '21', versions: 'Minecraft 1.20.5 to 1.21 (Java 21)' },
    { major: '25', versions: 'Minecraft 26.1 and later (Java 25)' },
  ]

  async function pickJava(major: string) {
    const chosen = await open({ title: `Choose a Java ${major} executable`, defaultPath: settings?.javaByMajor[major] || undefined })
    if (typeof chosen === 'string' && settings) settings.javaByMajor[major] = chosen
  }

  async function save() {
    if (!settings) return
    saving = true
    try {
      const editorAskMb = Math.max(1, Math.round(settings.editorAskMb || 50))
      const editorMaxMb = Math.max(editorAskMb, Math.round(settings.editorMaxMb || 100))
      const adjusted = editorAskMb !== settings.editorAskMb || editorMaxMb !== settings.editorMaxMb
      const javaByMajor = Object.fromEntries(Object.entries(settings.javaByMajor).map(([major, java]) => [major, java.trim()]).filter(([, java]) => java))
      await api.saveSettings({ ...settings, javaByMajor, editorAskMb, editorMaxMb })
      await load()
      onchange()
      messageFailed = false
      identityMessage = null
      message = adjusted ? `Saved. The editor limits are now ${editorAskMb} MB and ${editorMaxMb} MB, since the second can't be lower than the first.` : 'Saved'
    } catch (error) {
      messageFailed = true
      message = `Couldn't save: ${errorText(error)}`
    } finally {
      saving = false
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
  let defenderFailed = $state(false)

  onMount(async () => {
    defender = await api.defenderStatus().catch(() => null)
  })

  async function excludeFolders() {
    if (!defender) return
    defenderBusy = true
    try {
      await api.defenderExclude(defender.missing)
      defender = await api.defenderStatus()
      defenderFailed = false
      defenderMessage = 'Folders excluded.'
    } catch (error) {
      defenderFailed = true
      defenderMessage = `Couldn't exclude the folders: ${errorText(error)}`
    } finally {
      defenderBusy = false
    }
  }

  async function setDefenderPrompt(enabled: boolean, box: HTMLInputElement) {
    try {
      await api.defenderDismiss(!enabled)
      if (settings) settings.defenderDismissed = !enabled
    } catch (error) {
      box.checked = !enabled
      defenderFailed = true
      defenderMessage = `Couldn't change the reminder: ${errorText(error)}`
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
  <header>
    <h2>App settings</h2>
    <p class="muted">Configure general packraven settings. For individual packs, see Pack settings.</p>
  </header>

  <div class="switch-row">
    <div>
      <div id="autostart-label">Start in the tray when I sign in</div>
      <p class="muted" id="autostart-hint">Keep packraven running so builds and test servers are a click away.</p>
      {#if autostartError}<p class="failed" role="alert">{autostartError}</p>{/if}
    </div>
    <button
      type="button"
      role="switch"
      class="switch"
      aria-checked={autostart}
      aria-labelledby="autostart-label"
      aria-describedby="autostart-hint"
      onclick={toggleAutostart}
    ><span></span></button>
  </div>

  <form class="form" onsubmit={(e) => { e.preventDefault(); save() }} oninput={() => !messageFailed && (message = null)}>
    <section>
      <h3>This machine</h3>
      <div class="path">
        <label>CurseForge install folder <input bind:value={settings.installDir} /></label>
        <button type="button" onclick={pickInstall}>Choose</button>
      </div>
    </section>

    <section>
      <h3>Commits</h3>
      <label>Name <input bind:value={settings.gitName} placeholder="From your git config" autocomplete="name" /></label>
      <label>
        Email
        <input bind:value={settings.gitEmail} placeholder="From your git config" autocomplete="email" aria-describedby="commit-identity-hint" />
      </label>
      <p class="muted hint" id="commit-identity-hint">
        packraven makes commits as this person when you publish or commit in Git. Leave a field empty to use your git config.
      </p>
      {#if credentials?.github.present}
        <div class="identity-row">
          <button type="button" class:busy={identityBusy} disabled={identityBusy} onclick={fillIdentityFromGithub}>Fill in from GitHub</button>
          {#if identityMessage}<span class:muted={identityMessage.ok} class:failed={!identityMessage.ok} role="status">{identityMessage.text}</span>{/if}
        </div>
      {/if}
    </section>

    <section>
      <h3>Java</h3>
      <p class="muted">
        Default: Use CurseForge's Java version for your pack. You can pick a different Java than Minecraft officially
        supports, like Java 25 for every version, but some mods may not work. Also overrideable in Pack settings.
      </p>
      {#each JAVA_SLOTS as slot (slot.major)}
        <div class="path">
          <label>
            {slot.versions}
            <input
              value={settings.javaByMajor[slot.major] ?? ''}
              oninput={(e) => (settings!.javaByMajor[slot.major] = e.currentTarget.value)}
              placeholder="CurseForge's Java {slot.major}"
            />
          </label>
          <button type="button" onclick={() => pickJava(slot.major)}>Choose</button>
        </div>
      {/each}
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
      <button type="submit" class="primary" class:busy={saving} disabled={saving}>{saving ? 'Saving' : 'Save app settings'}</button>
      <span class:muted={!messageFailed} class:failed={messageFailed} role="status">{message ?? ''}</span>
    </div>
  </form>

  {#if credentialsError}
    <section class="removal">
      <h3>CurseForge access</h3>
      <Notice tone="error">
        <p>Couldn't read the saved CurseForge keys: {credentialsError}</p>
        <p><button type="button" onclick={loadCredentials}>Try again</button></p>
      </Notice>
    </section>
  {:else if credentials}
    <section class="removal" id="curseforge-access">
      <h3>CurseForge access</h3>
      <p class="muted">Changes here take effect right away, including in the git hooks.</p>
      <CredentialField credential="download" label="CurseForge API key (a.k.a. Launcher Token)" status={credentials.download} keychain={credentials.keychain} {envNames} onchange={loadCredentials}>
        {#snippet help()}
          <p class="muted hint">
            packraven uses this to look up and download mods, and to keep jars you added by hand out of git. For now, you'll need your own:
            <a href={API_KEY_PAGE} onclick={(e) => { e.preventDefault(); openUrl(API_KEY_PAGE) }}>create one in the CurseForge for Studios console</a>, then paste it here.
          </p>
        {/snippet}
      </CredentialField>
      <CredentialField credential="upload" noun="token" label="CurseForge upload token" status={credentials.upload} keychain={credentials.keychain} {envNames} onchange={loadCredentials}>
        {#snippet help()}
          <p class="muted hint">
            You only need this to publish. <a href={UPLOAD_TOKEN_PAGE} onclick={(e) => { e.preventDefault(); openUrl(UPLOAD_TOKEN_PAGE) }}>Generate a token on CurseForge</a>, then paste it here.
          </p>
        {/snippet}
      </CredentialField>
    </section>

    <section class="removal" id="github-access">
      <h3>GitHub access</h3>
      <CredentialField credential="github" noun="token" label="GitHub token" status={credentials.github} keychain={credentials.keychain} {envNames} onchange={loadCredentials}>
        {#snippet help()}
          <p class="muted hint">
            packraven uses this to pull and push when your pack's remote is an HTTPS URL on github.com. Other remotes keep using your git setup.
            <a href={GITHUB_TOKEN_PAGE} onclick={(e) => { e.preventDefault(); openUrl(GITHUB_TOKEN_PAGE) }}>Create a fine-grained token</a> for the pack's
            repository with Contents: Read and write, then paste it here.
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
        <p class="muted">Windows Defender will skip the pack, CurseForge and packraven folders. Only install projects you trust.</p>
      {/if}
      <p class:muted={!defenderFailed} class:failed={defenderFailed} role="status">{defenderMessage ?? ''}</p>
      <label class="check">
        <input type="checkbox" checked={!settings.defenderDismissed} onchange={(e) => setDefenderPrompt(e.currentTarget.checked, e.currentTarget)} />
        Remind me when these folders aren't excluded
      </label>
    </section>
  {/if}
  </div>
{/if}

<style>
  .identity-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 12px;
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
    background: var(--control);
    border-color: var(--edge);
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
