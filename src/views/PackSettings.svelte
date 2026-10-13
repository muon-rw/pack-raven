<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import { onMount } from 'svelte'
  import { ask } from '@tauri-apps/plugin-dialog'
  import { api, errorText, type PackConfig } from '../lib/api'

  let { dir, name, onchange, onremoved }: { dir: string; name: string; onchange: () => void; onremoved: () => void } = $props()

  let config: PackConfig | null = $state(null)
  let configError: string | null = $state(null)
  let webhook = $state('')
  let message: string | null = $state(null)
  let messageFailed = $state(false)
  let discordMessage = $state<string | null>(null)
  let discordFailed = $state(false)
  let testing = $state(false)
  let removeError: string | null = $state(null)

  async function load() {
    try {
      config = await api.getPackConfig()
      webhook = (await api.getWebhook()) ?? ''
      configError = null
    } catch (error) {
      config = null
      configError = errorText(error)
    }
  }

  onMount(load)

  async function save() {
    if (!config) return
    try {
      await api.savePackConfig(config)
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
    <p class="muted">For {name} only. packraven stores these in <span class="file">packraven.toml</span> in the pack repo, except the webhook.</p>
  </header>

  {#if configError}
    <Notice tone="error">
      <p>Couldn't read packraven.toml: {configError}</p>
      <p><button onclick={load}>Try again</button></p>
    </Notice>
  {:else if config}
    <form class="form" onsubmit={(e) => { e.preventDefault(); save() }}>
      <section>
        <h3>CurseForge</h3>
        <div class="grid">
          <label>Project ID <input type="number" bind:value={config.curseforge.project_id} /></label>
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
        <h3>Server and test run</h3>
        <div class="grid">
          <label>Server memory <input bind:value={config.server.memory} placeholder="4G" title="Java heap size, such as 4G or 6144M" /></label>
          <label>Test username <input bind:value={config.test.username} /></label>
          <label>Test server port <input type="number" bind:value={config.test.port} /></label>
        </div>
      </section>

      <section>
        <h3>Discord announcements</h3>
        <div class="field">
          <label for="webhook">Webhook URL</label>
          <div class="inline">
            <input id="webhook" type="password" autocomplete="off" bind:value={webhook} placeholder="https://discord.com/api/webhooks/..." aria-describedby="webhook-hint" />
            <button type="button" class:busy={testing} disabled={testing || !webhook.trim()} onclick={testWebhook}>{testing ? 'Sending' : 'Send test message'}</button>
          </div>
          <p class="hint" id="webhook-hint">Copy it from the channel's Settings, Integrations, Webhooks. packraven saves it only on this machine, never in the pack repo.</p>
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
        <button type="submit" class="primary">Save pack settings</button>
        <span class:muted={!messageFailed} class:failed={messageFailed} role="status">{message ?? ''}</span>
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
    margin-top: -8px;
    color: var(--muted);
    font-size: var(--step--1);
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
