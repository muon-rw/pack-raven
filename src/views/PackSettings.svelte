<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import JavaPicker from '../components/JavaPicker.svelte'
  import { onMount } from 'svelte'
  import { ask } from '@tauri-apps/plugin-dialog'
  import { api, errorText, type PackConfig, type PackJavaDefault } from '../lib/api'
  import { scanJava } from '../lib/java.svelte'
  import { serverView } from '../lib/nav.svelte'

  let {
    dir,
    name,
    onserver,
    onappsettings,
    onchange,
    onremoved,
  }: { dir: string; name: string; onserver: () => void; onappsettings: () => void; onchange: () => void; onremoved: () => void } = $props()

  let config: PackConfig | null = $state(null)
  let configError: string | null = $state(null)
  let webhook = $state('')
  let packJava = $state('')
  let javaDefault = $state<PackJavaDefault | null>(null)
  let message: string | null = $state(null)
  let messageFailed = $state(false)
  let discordMessage = $state<string | null>(null)
  let discordFailed = $state(false)
  let testing = $state(false)
  let removeError: string | null = $state(null)
  let saving = $state(false)
  let portMissing = $state(false)
  let portField = $state<HTMLInputElement>()

  async function load() {
    try {
      config = await api.getPackConfig()
      webhook = (await api.getWebhook()) ?? ''
      packJava = (await api.getPackJava()) ?? ''
      configError = null
      await loadJavaDefault()
    } catch (error) {
      config = null
      configError = errorText(error)
    }
  }

  async function loadJavaDefault() {
    javaDefault = await api.packJavaDefault().catch(() => null)
  }

  onMount(() => {
    scanJava()
    load()
  })

  async function save() {
    if (!config) return
    // Svelte binds a cleared number field to null, which packraven.toml can't hold. We treat an
    // empty project ID as not set, but the test server needs a port.
    config.curseforge.project_id = Number(config.curseforge.project_id) || 0
    portMissing = !config.test.port
    if (portMissing) {
      messageFailed = true
      message = 'Enter a test server port, such as 25565.'
      portField?.focus()
      return
    }
    saving = true
    try {
      // Server saves its own settings as you change them, so we keep whatever it wrote since we loaded.
      const fresh = await api.getPackConfig()
      await api.savePackConfig({ ...config, server: fresh.server, updates: fresh.updates })
      await api.setWebhook(webhook)
      await api.setPackJava(packJava.trim() || null)
      await load()
      onchange()
      messageFailed = false
      message = 'Saved'
    } catch (error) {
      messageFailed = true
      message = `Couldn't save: ${errorText(error)}`
    } finally {
      saving = false
    }
  }

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

  async function removePack() {
    const confirmed = await ask(`Remove ${name} from packraven? The instance folder, its files and its git repository stay as they are.`, {
      title: 'Remove pack',
      kind: 'warning',
      okLabel: 'Remove pack',
      cancelLabel: 'Cancel',
    })
    if (!confirmed) return
    try {
      removeError = null
      await api.removePack(dir)
      onremoved()
    } catch (error) {
      removeError = `Couldn't remove the pack: ${errorText(error)}`
    }
  }
</script>

<div class="settings">
  <header>
    <h2>Pack settings</h2>
    <p class="muted">For {name} only. packraven stores these in <span class="file">packraven.toml</span> in the pack repo, except the Java and the webhook.</p>
  </header>

  {#if configError}
    <Notice tone="error">
      <p>Couldn't read packraven.toml: {configError}</p>
      <p><button onclick={load}>Try again</button></p>
    </Notice>
  {:else if config}
    <form class="form" onsubmit={(e) => { e.preventDefault(); save() }} oninput={() => !messageFailed && (message = null)}>
      <section>
        <h3>CurseForge</h3>
        <div class="grid">
          <label>Project ID <input type="number" min="0" bind:value={config.curseforge.project_id} placeholder="Not set" /></label>
          <label>
            Release type
            <select bind:value={config.curseforge.release_type}>
              <option value="release">Release</option>
              <option value="beta">Beta</option>
              <option value="alpha">Alpha</option>
            </select>
          </label>
          <label>Author <input bind:value={config.author} /></label>
        </div>
        <label class="check">
          <input type="checkbox" bind:checked={config.curseforge.manual_release} /> Hold approved files until I release them on CurseForge
        </label>
      </section>

      <section>
        <h3>Test runs</h3>
        <div class="grid">
          <label>Offline username <input bind:value={config.test.username} /></label>
          <label>
            Server port
            <input type="number" min="1" max="65535" bind:this={portField} bind:value={config.test.port} aria-invalid={portMissing} aria-describedby={portMissing ? 'pack-save-status' : undefined} />
          </label>
          <label>Client memory <input bind:value={config.test.client_memory} placeholder="8G" aria-describedby="test-memory-hint" /></label>
        </div>
        <p class="hint" id="test-memory-hint">
          These stay in the test loop. The test client gets the client memory as its Java heap size, such as 8G or 8192M. For the memory, Java and flags
          that ship in the server pack, open <button type="button" class="link" onclick={() => ((serverView.tab = 'startup'), onserver())}>Server, Startup</button>.
        </p>
        <p class="hint">
          While you're signed out of Minecraft, test runs are offline and the client joins under the offline username.
          <button type="button" class="link" onclick={onappsettings}>Sign in under App settings</button> to play as yourself.
        </p>
        {#if javaDefault}
          <JavaPicker
            id="pack-java"
            label="Java on this machine (this pack needs Java {javaDefault.major})"
            major={javaDefault.major}
            bind:value={packJava}
            defaultName="Same as App settings"
            defaultPath={javaDefault.path}
            needed
            hint="pack-java-hint"
            ondownload={loadJavaDefault}
          />
          <p class="hint" id="pack-java-hint">
            packraven uses this Java for the pack's test runs and server builds, and saves your choice only on this machine, never in the pack repo. To choose a Java for every pack, open <button type="button" class="link" onclick={onappsettings}>App settings</button>.
          </p>
        {/if}
      </section>

      <section>
        <h3>Discord announcements</h3>
        <div class="field">
          <label for="webhook">Webhook URL</label>
          <div class="inline">
            <input id="webhook" type="password" autocomplete="off" bind:value={webhook} placeholder="https://discord.com/api/webhooks/..." aria-describedby="webhook-hint" />
            <button type="button" class:busy={testing} disabled={testing || !webhook.trim()} onclick={testWebhook}>{testing ? 'Sending' : 'Send test message'}</button>
          </div>
          <p class="hint" id="webhook-hint">
            Copy it from the channel's Settings, Integrations, Webhooks. packraven saves it only on this machine, never in the pack repo. Sending a test
            message saves it right away.
          </p>
          <p class:muted={!discordFailed} class:failed={discordFailed} role="status">{discordMessage ?? ''}</p>
        </div>
        <div class="grid">
          <label>Role to ping <input bind:value={config.discord.role_id} placeholder="Role ID" aria-describedby="role-hint" /></label>
          <label>Embed color <input type="color" bind:value={config.discord.color} /></label>
        </div>
        <p class="hint" id="role-hint">
          To get a role ID, turn on Developer Mode in Discord's Advanced settings, then right-click the role and choose Copy Role ID. Leave it empty
          for no ping.
        </p>
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
          These use Discord markdown and appear in every announcement. <code>{'{file_url}'}</code> links to the released file, <code>{'{project_url}'}</code> to the project page. <code>{'{project_name}'}</code> is the CurseForge project's name, <code>{'{name}'}</code> the instance name, and <code>{'{version}'}</code> the version. packraven uses the project logo as the thumbnail.
        </p>
        <label class="check">
          <input type="checkbox" bind:checked={config.discord.show_author} /> Show the project author, with their CurseForge avatar and a link to their profile
        </label>
        <label class="check">
          <input type="checkbox" bind:checked={config.discord.announce_on_approval} /> Announce each release once CurseForge approves it
        </label>
        <p class="hint">packraven checks every few minutes while it's running in the tray. You can also announce a release by hand from Releases.</p>
      </section>

      <div class="actions">
        <button type="submit" class="primary" class:busy={saving} disabled={saving}>{saving ? 'Saving' : 'Save pack settings'}</button>
        <span id="pack-save-status" class:muted={!messageFailed} class:failed={messageFailed} role="status">{message ?? ''}</span>
      </div>
    </form>
  {/if}

  <section class="removal">
    <h3>Remove pack</h3>
    <p class="muted">Stop showing <span class="file">{dir}</span> in packraven, without deleting anything on disk.</p>
    <div><button type="button" class="danger" onclick={removePack}>Remove pack</button></div>
    {#if removeError}<p class="failed" role="alert">{removeError}</p>{/if}
  </section>
</div>

<style>
  .settings {
    display: grid;
    gap: 36px;
    max-width: 760px;
  }

  header p {
    margin-top: 4px;
  }

  .form {
    display: grid;
    gap: 36px;
  }

  section {
    display: grid;
    gap: 14px;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 14px 18px;
  }

  .field {
    display: grid;
    gap: 6px;
  }

  .field > label,
  .grid > label,
  .block {
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

  .grid + .hint,
  .block + .hint {
    margin-top: -6px;
  }

  input[type='color'] {
    width: 52px;
    height: 38px;
    padding: 3px;
    cursor: pointer;
  }

  .block {
    display: grid;
    gap: 6px;
  }

  .block textarea {
    font-family: var(--font-log);
    font-size: var(--step--1);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .removal {
    padding-top: 28px;
    border-top: 1px solid var(--line);
  }
</style>
