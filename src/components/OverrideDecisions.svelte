<script lang="ts">
  import { tick } from 'svelte'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import Notice from './Notice.svelte'
  import Tip from './Tip.svelte'
  import { api, errorText, type OverrideCandidate, type OverrideChoice, type Platform, type Skew } from '../lib/api'
  import { notOnHostsSummary } from '../lib/modsync'
  import { changeFor, changeQueued, dropChange, startChange } from '../lib/modUpdates.svelte'

  let {
    platform,
    candidates,
    dual,
    checking,
    onchange,
    onsettings,
  }: {
    platform: Platform
    candidates: OverrideCandidate[]
    // The pack goes to both platforms, so the other one's pack keeps these files either way.
    dual: boolean
    // We're looking up the files' authors, licenses and other versions.
    checking: boolean
    onchange: () => void
    onsettings: () => void
  } = $props()

  const PLATFORM = {
    modrinth: { name: 'Modrinth', other: 'CurseForge', guide: 'https://support.modrinth.com/en/articles/8797527-obtaining-modpack-permissions', guideLabel: "Modrinth's guide to modpack permissions", how: 'How Modrinth checks permissions' },
    curseforge: { name: 'CurseForge', other: 'Modrinth', guide: 'https://docs.google.com/spreadsheets/d/176Wv-PZUo9hFxy6oC6N8tWdquBLPRtSuLbNK-r0_byM/htmlview', guideLabel: "CurseForge's list of allowed mods", how: 'How CurseForge checks jars' },
  } as const
  const EVIDENCE: Record<Skew['link']['evidence'], string> = { sameFiles: 'matching file names', sameSource: 'the same source repository', sameAuthor: 'the same author' }
  // Past a handful of rows, the notice would push the release notes out of view.
  const ROWS = 4
  // Long enough to catch the second press of a double click on a row that just moved up.
  const SETTLE_MS = 400

  type Undo = { kind: 'files'; paths: string[] } | { kind: 'author'; author: string }
  let copy = $derived(PLATFORM[platform])
  let showAll = $state(false)
  let choosing = $state<string | null>(null)
  let settling = $state(false)
  let result = $state<{ ok: boolean; text: string; undo?: Undo } | null>(null)
  // Where your place goes once the list catches up: the row that moved up after you include one, the
  // same button after you omit one, or the result after a choice about several files.
  let refocus = $state<{ paths: string[]; index: number; target: 'row' | 'leaveOut' | 'result' } | null>(null)
  let permittingAuthor = $state('')

  let shown = $derived(showAll ? candidates : candidates.slice(0, ROWS))
  let paths = $derived(candidates.map((file) => file.path))
  // Each way to decide several files at once rests on a reason you can check: a license, an author's
  // word, or CurseForge's own list. Without one, including a file rests on your word alone.
  const isVouched = (file: OverrideCandidate) => (platform === 'modrinth' ? file.openLicense : file.allowlisted)
  let vouched = $derived(candidates.filter(isVouched))
  let authors = $derived(platform === 'modrinth' ? [...new Set(candidates.flatMap((file) => file.authors))].sort((a, b) => a.localeCompare(b)) : [])
  let busy = $derived(!!choosing || settling)
  const filesBy = (author: string) => candidates.filter((file) => file.authors.includes(author)).map((file) => file.path)
  const id = (part: string) => `${platform}-${part}`

  // Platforms name jars and zips differently in their rules, so the button says which it is.
  const fileKind = (path: string) => (path.endsWith('.jar') ? 'jar' : path.endsWith('.zip') ? 'zip' : 'file')

  function includeTip(path: string) {
    const what = fileKind(path) === 'file' ? 'this file' : `this ${fileKind(path)}`
    const rule =
      platform === 'modrinth'
        ? "Modrinth's moderators may ask you for proof that you may share it."
        : "CurseForge automatically rejects the pack unless this mod is on its list of allowed mods, or CurseForge gave you permission to include it."
    return `Bundle ${what} inside the ${copy.name} pack's overrides, so players get it even though ${copy.name} doesn't host it. ${rule}`
  }

  let omitTip = $derived(
    `Leave this file out of the ${copy.name} pack. Make sure the pack still runs without it, or tell players to download it themselves, for example with an in-game notice.${dual ? ` The ${copy.other} pack still includes it.` : ''}`,
  )

  async function showMore() {
    showAll = true
    await tick()
    document.getElementById(id(`row-${ROWS}`))?.focus()
  }

  async function choose(chosen: string[], choice: OverrideChoice, index: number, label: string, target: 'row' | 'leaveOut' | 'result') {
    choosing = `${choice}:${chosen.length === 1 ? chosen[0] : 'all'}`
    try {
      await api.setOverrideChoice(platform, chosen, choice)
      const them = chosen.length === 1 ? 'it' : 'them'
      const text =
        choice === 'ship'
          ? `The ${copy.name} pack includes ${label}.`
          : `The ${copy.name} pack omits ${label}, so players who install it won't get ${them}.${dual ? ` The ${copy.other} pack still has ${them}.` : ''}`
      result = { ok: true, text, undo: { kind: 'files', paths: chosen } }
      refocus = { paths: chosen, index, target }
      onchange()
    } catch (error) {
      result = { ok: false, text: `Couldn't save the choice for ${chosen.length === 1 ? chosen[0] : 'those files'}: ${errorText(error)}` }
    } finally {
      choosing = null
    }
  }

  async function undo(undo: Undo) {
    choosing = 'undo'
    try {
      if (undo.kind === 'files') await api.setOverrideChoice(platform, undo.paths, null)
      else await api.setModrinthAuthor(undo.author, false)
      result = { ok: true, text: undo.kind === 'files' ? 'Undone. Decide again below.' : `Undone. The Modrinth pack no longer includes everything by ${undo.author}.` }
      onchange()
    } catch (error) {
      result = { ok: false, text: `Couldn't undo that: ${errorText(error)}` }
    } finally {
      choosing = null
    }
  }

  async function shipByAuthor(author: string, covered: string[]) {
    choosing = `author:${author}`
    try {
      await api.setModrinthAuthor(author, true)
      result = { ok: true, text: `The Modrinth pack includes everything by ${author}, including mods of theirs you add later.`, undo: { kind: 'author', author } }
      permittingAuthor = ''
      refocus = { paths: covered, index: 0, target: 'result' }
      onchange()
    } catch (error) {
      result = { ok: false, text: `Couldn't save ${author}'s permission: ${errorText(error)}` }
    } finally {
      choosing = null
    }
  }

  // The swap goes through the same queue as an update in Mods, and the next mod list refresh finds
  // Modrinth's copy of the new file.
  async function switchVersion(file: OverrideCandidate, fileId: number, version: string) {
    const name = file.name ?? file.path
    const serial = startChange([file.path], [name], 'update')
    result = { ok: true, text: `Switching ${name} to ${version}. We'll drop its row once Modrinth's copy matches.` }
    try {
      changeQueued(serial, await api.changeMods([{ file: `cf:${fileId}`, replaces: file.path }]))
    } catch (error) {
      dropChange(serial)
      result = { ok: false, text: `Couldn't switch ${name}: ${errorText(error)}` }
    }
  }

  $effect(() => {
    const current = candidates.map((file) => file.path)
    if (!refocus || current.some((path) => refocus!.paths.includes(path))) return
    const index = Math.min(refocus.index, current.length - 1)
    const target = refocus.target === 'result' || index < 0 ? id('choice-result') : refocus.target === 'row' ? id(`row-${index}`) : id(`leaveOut-${index}`)
    refocus = null
    settling = true
    setTimeout(() => (settling = false), SETTLE_MS)
    tick().then(() => document.getElementById(target)?.focus())
  })

  // What Modrinth has instead of this file, and the version to switch to when both platforms have one.
  function describeSkew(skew: Skew): { text: string; switchTo: { fileId: number; name: string; label: string } | null } {
    const { shared, modrinthNewest, modrinthNewer, newestOnCurseforge, sameNameDiffers } = skew
    if (shared) {
      const also = modrinthNewer && modrinthNewest && !newestOnCurseforge ? ` Modrinth also has ${modrinthNewest}, which CurseForge doesn't.` : ''
      return shared.newer
        ? { text: `Modrinth doesn't have this version. Both platforms have ${shared.name}.${also}`, switchTo: { ...shared, label: 'Switch to it on both' } }
        : { text: `Modrinth doesn't have this version. Both platforms have an older one, ${shared.name}.${also}`, switchTo: { ...shared, label: 'Switch back to it on both' } }
    }
    if (sameNameDiffers) return { text: "Modrinth has a file with this name, but what's in it differs from CurseForge's.", switchTo: null }
    if (!modrinthNewest) return { text: "Modrinth has no version of it for this pack's Minecraft and loader.", switchTo: null }
    return { text: `Modrinth only has ${modrinthNewer ? 'newer' : 'older'} versions, up to ${modrinthNewest}, and CurseForge has none of them.`, switchTo: null }
  }
</script>

{#if candidates.length}
  <Notice tone="warn" live={false}>
    <div class="mod-list-notice">
      <p>
        <strong>{notOnHostsSummary(paths, copy.name)}.</strong>
        Include the ones {platform === 'modrinth' ? 'you may share' : 'CurseForge allows'}, and omit the rest. Players who install the {copy.name} pack won't get the
        ones you omit{#if dual}, though the {copy.other} pack keeps them{/if}.
      </p>
      <details class="how">
        <summary>{copy.how}</summary>
        <p>
          {#if platform === 'modrinth'}
            Modrinth's moderators check that you may share each file you include inside a pack, through its license, its project page or its author, and they
            may ask you for proof. Including a file without an open license rests on your word.
          {:else}
            CurseForge automatically rejects a pack carrying a mod jar it doesn't host, unless the mod is on its list of allowed mods or CurseForge gave you
            permission to include it.
          {/if}
          <button class="link" onclick={() => openUrl(copy.guide)}>{copy.guideLabel}</button>
        </p>
      </details>
      {#if vouched.length > 1 || authors.length || candidates.length > 1}
        <div class="reasons">
          {#if vouched.length > 1}
            {@const names = vouched.map((file) => file.name ?? file.path)}
            <button
              class:busy={choosing === 'ship:all'}
              disabled={busy}
              onclick={() => choose(vouched.map((file) => file.path), 'ship', 0, names.length <= 3 ? names.join(', ') : `the ${names.length} files ${platform === 'modrinth' ? 'with open licenses' : "on CurseForge's list"}`, 'result')}
            >
              Include the {vouched.length} {platform === 'modrinth' ? 'with open licenses' : "on CurseForge's list"}
            </button>
          {/if}
          {#if authors.length}
            {@const covered = permittingAuthor ? filesBy(permittingAuthor) : []}
            <span class="by-author">
              <label for={id('permitting-author')}>An author's permission</label>
              <select id={id('permitting-author')} bind:value={permittingAuthor}>
                <option value="" disabled>Choose an author</option>
                {#each authors as author (author)}
                  {@const count = filesBy(author).length}
                  <option value={author}>{author} ({count} {count === 1 ? 'file' : 'files'})</option>
                {/each}
              </select>
              <button class:busy={choosing === `author:${permittingAuthor}`} disabled={busy || !permittingAuthor} onclick={() => shipByAuthor(permittingAuthor, covered)}>
                {permittingAuthor ? `Include ${permittingAuthor}'s ${covered.length === 1 ? 'file' : `${covered.length} files`} and future ones` : 'Include their files and future ones'}
              </button>
            </span>
          {/if}
          {#if candidates.length > 1}
            <button class:busy={choosing === 'leaveOut:all'} disabled={busy} onclick={() => choose(paths, 'leaveOut', 0, `${paths.length} files`, 'result')}>Omit all {paths.length}</button>
          {/if}
        </div>
      {/if}
      <ul class="decisions" class:all={showAll} aria-label="Files not on {copy.name}">
        {#each shown as file, index (file.path)}
          {@const label = file.name ?? file.path}
          {@const skew = file.skew ? describeSkew(file.skew) : null}
          {@const switching = changeFor(file.path)}
          <li id={id(`row-${index}`)} tabindex="-1">
            <span class="what">
              {#if file.name && file.url}
                <a href={file.url} onclick={(e) => { e.preventDefault(); openUrl(file.url!) }}>{file.name}<span class="visually-hidden"> on {copy.other}</span></a>
              {:else if file.name}
                <span>{file.name}</span>
              {/if}
              <span class="facts muted">
                {#if file.name}
                  {#if file.authors.length}by {file.authors.join(', ')} ·{/if}
                  {#if file.license}{file.license}{:else if file.known || !checking}No license listed{:else}Looking up its license{/if}
                  {#if platform === 'modrinth' && file.openLicense}{' '}<span class="chip ok">open license</span>{/if}
                  {#if platform === 'curseforge' && file.allowlisted}{' '}<span class="chip ok">on CurseForge's list</span>{/if}
                {:else}
                  packraven doesn't know which project this file is from.
                {/if}
              </span>
              {#if file.skew && skew}
                <span class="skew">
                  {skew.text}{#if skew.switchTo && file.locked}{' '}You locked its version in Mods.{/if}
                  <span class="muted">
                    On Modrinth as <a href={file.skew.link.url} onclick={(e) => { e.preventDefault(); openUrl(file.skew!.link.url) }}>{file.skew.link.title}</a>, matched by
                    {EVIDENCE[file.skew.link.evidence]}.
                  </span>
                </span>
              {:else if platform === 'modrinth' && !file.versionsKnown && checking}
                <span class="skew muted">Checking which versions Modrinth has</span>
              {/if}
              <span class="file" class:muted={!!file.name}>{file.path}</span>
            </span>
            <span class="choices">
              {#if skew?.switchTo}
                {@const target = skew.switchTo}
                <button class="primary" class:busy={!!switching && switching.stage !== 'finished'} disabled={busy || !!switching} onclick={() => switchVersion(file, target.fileId, target.name)}>
                  {switching ? 'Switching' : target.label}<span class="visually-hidden">: {label} to {target.name}</span>
                </button>
              {/if}
              <Tip tip={includeTip(file.path)}>
                {#snippet children(why)}
                  <button id={id(`ship-${index}`)} class:busy={choosing === `ship:${file.path}`} disabled={busy} aria-describedby={why} onclick={() => choose([file.path], 'ship', index, label, 'row')}>
                    Include {fileKind(file.path)}<span class="visually-hidden">: {label} in the {copy.name} pack</span>
                  </button>
                {/snippet}
              </Tip>
              <Tip tip={omitTip}>
                {#snippet children(why)}
                  <button id={id(`leaveOut-${index}`)} class:busy={choosing === `leaveOut:${file.path}`} disabled={busy} aria-describedby={why} onclick={() => choose([file.path], 'leaveOut', index, label, 'leaveOut')}>
                    Omit<span class="visually-hidden"> {label} from the {copy.name} pack</span>
                  </button>
                {/snippet}
              </Tip>
            </span>
          </li>
        {/each}
      </ul>
      {#if candidates.length > shown.length}
        <button class="link show-all" onclick={showMore}>Show {candidates.length - shown.length} more</button>
      {/if}
    </div>
  </Notice>
{/if}
<!-- Mounted all along, so screen readers announce each result as it changes. Hidden while empty, so it
     doesn't add a gap between the notices. -->
<p id={id('choice-result')} tabindex="-1" class:visually-hidden={!result} class:muted={result?.ok} class:failed={result && !result.ok} role="status">
  {#if result}
    {result.text}
    {#if result.undo}
      {@const last = result.undo}
      {' '}<button class="link" disabled={!!choosing} onclick={() => undo(last)}>Undo</button>, or change it later in
      <button class="link" onclick={onsettings}>Pack settings</button>.
    {/if}
  {/if}
</p>

<style>
  .mod-list-notice {
    display: grid;
    gap: 10px;
  }

  /* A cap keeps each file within reach of its buttons in a wide window. Notice caps the height of its
     lists for plain file names, but we show at most a few of these taller rows until you ask for all. */
  .mod-list-notice .decisions {
    display: grid;
    gap: 8px;
    max-width: 840px;
    max-height: none;
    overflow: visible;
    margin: 6px 0 0;
    padding: 0;
    list-style: none;
  }

  .mod-list-notice .decisions.all {
    max-height: 50vh;
    overflow: auto;
  }

  .decisions li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 4px 12px;
  }

  .decisions li:focus-visible {
    outline-offset: 2px;
  }

  .what {
    display: grid;
    gap: 1px;
    min-width: 0;
    flex: 1 1 320px;
  }

  .what .file,
  .facts,
  .skew {
    overflow-wrap: anywhere;
    font-size: var(--step--1);
  }

  .skew {
    max-width: 65ch;
    margin: 2px 0;
  }

  .how summary {
    width: fit-content;
    color: var(--text);
    font-size: var(--step--1);
    cursor: pointer;
  }

  .how p {
    max-width: 65ch;
    margin-top: 4px;
    font-size: var(--step--1);
  }

  .reasons {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 14px;
  }

  .by-author {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }

  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .show-all {
    justify-self: start;
  }
</style>
