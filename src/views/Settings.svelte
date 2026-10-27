<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import CredentialField from '../components/CredentialField.svelte'
  import JavaPicker from '../components/JavaPicker.svelte'
  import ExtraLaunchArgs, { emptyDraft, fromDraft, toDraft } from '../components/ExtraLaunchArgs.svelte'
  import { onMount, tick } from 'svelte'
  import { open } from '@tauri-apps/plugin-dialog'
  import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { api, errorText, type CredentialsView, type DefenderView, type MinecraftAccount, type Settings, type UpdateView } from '../lib/api'
  import { installNow, selfUpdate, updateWaiting } from '../lib/selfUpdate.svelte'
  import { javas, scanJava } from '../lib/java.svelte'

  let { onchange, onaccount }: { onchange: () => void; onaccount: () => void } = $props()

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
  let runArgs = $state(emptyDraft())
  let runArgsEditor = $state<ExtraLaunchArgs>()

  async function load() {
    try {
      const loaded = await api.getSettings()
      runArgs = toDraft(loaded.runArgs)
      settings = loaded
      settingsError = null
    } catch (error) {
      settingsError = errorText(error)
      return
    }
    await Promise.all([loadCredentials(), loadAccount()])
  }

  let account = $state<MinecraftAccount | null>(null)
  let signingIn = $state(false)
  let accountError = $state<{ text: string; retry: () => void } | null>(null)
  // We keep one live region and change its text, since screen readers often miss a status that appears.
  let accountStatus = $state('')
  let accountButton: HTMLButtonElement | undefined = $state()
  let onlineError: string | null = $state(null)

  // Each state swaps the button you just pressed for another, so we move focus to the new one.
  async function focusAccountButton() {
    await tick()
    accountButton?.focus()
  }

  async function loadAccount() {
    try {
      account = await api.minecraftAccount()
      accountError = null
    } catch (error) {
      accountError = { text: `Couldn't read your Minecraft sign-in: ${errorText(error)}`, retry: loadAccount }
    }
  }

  async function signIn() {
    signingIn = true
    accountError = null
    accountStatus = 'Waiting for you to sign in with Microsoft in your browser.'
    focusAccountButton()
    try {
      const signedIn = await api.minecraftSignIn()
      account = signedIn
      accountStatus = signedIn ? `Signed in as ${signedIn.name}.` : 'Sign-in cancelled.'
      if (signedIn) onaccount()
    } catch (error) {
      accountStatus = ''
      accountError = { text: errorText(error), retry: signIn }
    } finally {
      signingIn = false
      focusAccountButton()
    }
  }

  async function signOut() {
    accountError = null
    try {
      await api.minecraftSignOut()
      account = null
      accountStatus = 'Signed out of Minecraft.'
      onaccount()
      focusAccountButton()
    } catch (error) {
      accountError = { text: errorText(error), retry: signOut }
    }
  }

  async function toggleOnlineMode() {
    if (!settings) return
    const wanted = !settings.minecraftOnlineMode
    settings.minecraftOnlineMode = wanted
    try {
      onlineError = null
      await api.setMinecraftOnlineMode(wanted)
    } catch (error) {
      settings.minecraftOnlineMode = !wanted
      onlineError = `Couldn't change online mode: ${errorText(error)}`
    }
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
    scanJava()
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

  async function save() {
    if (!settings) return
    const argsProblem = runArgsEditor?.revealProblem() ?? false
    saving = true
    try {
      const editorAskMb = Math.max(1, Math.round(settings.editorAskMb || 50))
      const editorMaxMb = Math.max(editorAskMb, Math.round(settings.editorMaxMb || 100))
      const adjusted = editorAskMb !== settings.editorAskMb || editorMaxMb !== settings.editorMaxMb
      const javaByMajor = Object.fromEntries(Object.entries(settings.javaByMajor).map(([major, java]) => [major, java.trim()]).filter(([, java]) => java))
      await api.saveSettings({ ...settings, javaByMajor, editorAskMb, editorMaxMb, runArgs: fromDraft(runArgs) })
      await load()
      scanJava()
      onchange()
      messageFailed = false
      identityMessage = null
      message = argsProblem
        ? 'Saved, but an extra launch argument will stop test runs from starting'
        : adjusted
          ? `Saved. The editor limits are now ${editorAskMb} MB and ${editorMaxMb} MB, since the second can't be lower than the first.` : 'Saved'
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

  const RELEASES_PAGE = 'https://github.com/muon-rw/packraven/releases'

  let autoUpdateError: string | null = $state(null)
  let checkingUpdate = $state(false)

  async function toggleAutoUpdate() {
    if (!settings) return
    const wanted = !settings.autoUpdate
    settings.autoUpdate = wanted
    try {
      autoUpdateError = null
      await api.setAutoUpdate(wanted)
    } catch (error) {
      settings.autoUpdate = !wanted
      autoUpdateError = `Couldn't change the update setting: ${errorText(error)}`
    }
  }

  async function checkForUpdate() {
    checkingUpdate = true
    try {
      selfUpdate.view = await api.checkForUpdate()
    } catch {
      // The backend reports the failure as the update status too, so the status line shows it.
    } finally {
      checkingUpdate = false
    }
  }

  function updateStatusText(view: UpdateView): string {
    if (!view.supported) return `This is a development build of packraven ${view.current}, so we don't update it.`
    switch (view.status.phase) {
      case 'unchecked':
        return `You have packraven ${view.current}.`
      case 'checking':
        return 'Checking for updates.'
      case 'upToDate':
        return `packraven ${view.current} is the latest version.`
      case 'available':
        return `packraven ${view.status.version} is available. You have ${view.current}.`
      case 'downloading':
        return `Downloading packraven ${view.status.version}.`
      case 'ready':
        return view.auto
          ? `packraven ${view.status.version} is ready. We'll install it when you quit packraven, or you can restart now.`
          : `packraven ${view.status.version} is ready. Restart packraven to install it.`
      case 'failed':
        if (view.status.stage === 'install') return "Couldn't install the update."
        // Background checks fail whenever you're offline, so we keep these calm and retry later.
        return `Couldn't ${view.status.stage === 'check' ? 'check for updates' : 'download the update'}, so we'll try again later. You have packraven ${view.current}.`
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

  <section class="account" id="minecraft-account">
    <h3>Minecraft account</h3>
    {#if account}
      <p>Signed in as <strong>{account.name}</strong>. Test clients launch as this account on this computer.</p>
    {:else if signingIn}
      <p class="muted">Finish signing in with Microsoft in your browser. packraven is waiting.</p>
    {:else}
      <p class="muted">
        Sign in with a Microsoft account that owns Minecraft: Java Edition, so test clients launch as you and test servers can run in online mode. Until then,
        test runs are offline, under each pack's offline username.
      </p>
    {/if}
    <div class="identity-row">
      {#if account}
        <button type="button" bind:this={accountButton} onclick={signOut}>Sign out</button>
      {:else if signingIn}
        <button type="button" bind:this={accountButton} onclick={() => api.minecraftSignInCancel()}>Cancel</button>
      {:else}
        <button type="button" class="primary" bind:this={accountButton} onclick={signIn}>Sign in with Microsoft</button>
      {/if}
    </div>
    <p class="visually-hidden" role="status">{accountStatus}</p>
    {#if accountError}
      <Notice tone="error">
        <p>{accountError.text}</p>
        <p><button type="button" onclick={accountError.retry}>Try again</button></p>
      </Notice>
    {/if}
    {#if account}
      <div class="online-row">
        <div>
          <div id="online-label">Run test servers in online mode</div>
          <p class="muted" id="online-hint">
            Online test servers check your account like a real server, so they need an internet connection. Turn this off to test offline; the client
            still launches as you.
          </p>
          {#if onlineError}<p class="failed" role="alert">{onlineError}</p>{/if}
        </div>
        <button
          type="button"
          role="switch"
          class="switch"
          aria-checked={settings.minecraftOnlineMode}
          aria-labelledby="online-label"
          aria-describedby="online-hint"
          onclick={toggleOnlineMode}
        ><span></span></button>
      </div>
      {#if credentials}<p class="muted hint">packraven keeps your sign-in in {credentials.keychain}.</p>{/if}
    {/if}
    <p class="muted hint">packraven is not an official Minecraft product, and is not approved by or associated with Mojang or Microsoft.</p>
  </section>

  <div class="app-switches">
  <div class="switch-row">
    <div>
      <div id="autostart-label">Start in the tray when I sign in to this computer</div>
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

  <div class="switch-row" id="updates">
    <div class="updates">
      <div id="auto-update-label">Keep packraven up to date automatically</div>
      {#if autoUpdateError}<p class="failed" role="alert">{autoUpdateError}</p>{/if}
      {#if selfUpdate.view}
        {@const view = selfUpdate.view}
        {@const installFailed = view.status.phase === 'failed' && view.status.stage === 'install'}
        <p class:muted={!installFailed} class:failed={installFailed} role="status">{updateStatusText(view)}</p>
        {#if view.status.phase === 'failed'}
          <details class="update-error">
            <summary>Details</summary>
            <p class="file">{view.status.error}</p>
          </details>
        {/if}
        {#if view.status.phase === 'downloading'}
          <progress max="100" value={view.status.percent ?? undefined} aria-labelledby="auto-update-label"></progress>
        {:else if view.supported}
          <div class="identity-row">
            {#if updateWaiting(view)}
              <button type="button" class="primary" disabled={!!view.blocked} aria-describedby={view.blocked ? 'update-blocked' : undefined} onclick={installNow}>
                {view.status.phase === 'available' ? 'Update and restart' : view.auto ? 'Restart now' : 'Restart to update'}
              </button>
              <a href="{RELEASES_PAGE}/tag/v{view.status.version}" onclick={(e) => { e.preventDefault(); openUrl(e.currentTarget.href) }}>What's new</a>
            {:else}
              {@const busy = checkingUpdate || view.status.phase === 'checking'}
              <button type="button" class:busy disabled={busy} onclick={checkForUpdate}>Check for updates</button>
            {/if}
          </div>
          {#if updateWaiting(view) && view.blocked}<p class="muted hint" id="update-blocked">{view.blocked}</p>{/if}
        {/if}
      {/if}
    </div>
    <button
      type="button"
      role="switch"
      class="switch"
      aria-checked={settings.autoUpdate}
      aria-labelledby="auto-update-label"
      onclick={toggleAutoUpdate}
    ><span></span></button>
  </div>
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
        packraven makes commits as this person when you publish or press commit. Leave a field empty to use your git config.
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
        On Automatic, we use CurseForge's Java for each version, or another Java of that version on this computer. A newer Java runs most packs from
        Minecraft 1.18 on, though some mods may break. To use a different Java for one pack, choose it in Pack settings.
      </p>
      {#each JAVA_SLOTS as slot (slot.major)}
        <JavaPicker
          id="java-{slot.major}"
          label={slot.versions}
          major={Number(slot.major)}
          bind:value={() => settings!.javaByMajor[slot.major] ?? '', (path) => (settings!.javaByMajor[slot.major] = path)}
          defaultName="Automatic"
          defaultPath={javas.scan?.automatic[slot.major] ?? null}
          needed={javas.scan?.needed.includes(Number(slot.major)) ?? false}
          ondownload={(java) => {
            // The backend only chooses a download for a slot when Automatic wouldn't pick it, like GraalVM 21 for Java 17.
            if (java.major !== Number(slot.major)) settings!.javaByMajor[slot.major] = java.path
          }}
        />
      {/each}
      <p class="muted hint java-scan" role="status">
        {#if javas.error}
          <span class="failed">Couldn't look for Java: {javas.error}</span>
        {:else if javas.scan}
          {javas.scan.installs.length === 1 ? '1 Java install' : `${javas.scan.installs.length} Java installs`} found on this computer.
        {/if}
        <button type="button" class="link" disabled={javas.scanning} onclick={scanJava}>{javas.scanning ? 'Looking' : 'Look again'}</button>
      </p>
      <ExtraLaunchArgs bind:this={runArgsEditor} bind:draft={runArgs} scope="app" />
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
        <p>Couldn't read your saved CurseForge and GitHub tokens: {credentialsError}</p>
        <p><button type="button" onclick={loadCredentials}>Try again</button></p>
      </Notice>
    </section>
  {:else if credentials}
    <section class="removal" id="curseforge-access">
      <h3>CurseForge access</h3>
      <p class="muted">Changes here take effect right away, including in the git hooks.</p>
      {#if !credentials.builtInApiKey}
        <Notice tone="warn">
          <p>This packraven build has no CurseForge API key, so we can't look up mods or keep jars you added by hand out of git. To fix this, build packraven with <code>PACKRAVEN_CF_API_KEY</code> set.</p>
        </Notice>
      {/if}
      <CredentialField credential="upload" label="CurseForge upload token" status={credentials.upload} keychain={credentials.keychain} {envNames} onchange={loadCredentials}>
        {#snippet help()}
          <p class="muted hint">
            You only need this to publish. <a href={UPLOAD_TOKEN_PAGE} onclick={(e) => { e.preventDefault(); openUrl(UPLOAD_TOKEN_PAGE) }}>Generate a token on CurseForge</a>, then paste it here.
          </p>
        {/snippet}
      </CredentialField>
    </section>

    <section class="removal" id="github-access">
      <h3>GitHub access</h3>
      <CredentialField credential="github" label="GitHub token" status={credentials.github} keychain={credentials.keychain} {envNames} onchange={loadCredentials}>
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

  .account {
    display: grid;
    gap: 12px;
    padding-bottom: 28px;
    border-bottom: 1px solid var(--line);
  }

  .account h3,
  .account p {
    margin: 0;
  }

  .online-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 24px;
    padding-top: 4px;
  }

  .app-switches {
    display: grid;
    gap: 24px;
    padding-bottom: 28px;
    border-bottom: 1px solid var(--line);
  }

  .switch-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 24px;
  }

  /* The status and buttons below the label would otherwise pull the switch down, away from it. */
  #updates {
    align-items: start;
  }

  .updates {
    display: grid;
    gap: 8px;
  }

  .updates p {
    margin: 0;
  }

  .updates .hint {
    margin-top: 0;
  }

  .updates progress {
    width: min(320px, 100%);
    accent-color: var(--forest);
  }

  .update-error summary {
    width: fit-content;
    color: var(--muted);
    font-size: var(--step--1);
    cursor: pointer;
  }

  .update-error p {
    margin-top: 4px;
    font-size: var(--step--1);
    overflow-wrap: anywhere;
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
