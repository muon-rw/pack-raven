<script lang="ts">
  import { tick } from 'svelte'
  import type { PackList } from '../lib/api'

  let {
    list,
    name,
    onswitch,
    onadd,
  }: { list: PackList | null; name: string; onswitch: (dir: string) => void; onadd: () => void } = $props()

  let open = $state(false)
  let trigger: HTMLButtonElement
  let menu: HTMLDivElement = $state()!

  function items(): HTMLElement[] {
    return Array.from(menu?.querySelectorAll<HTMLElement>('[role="menuitem"], [role="menuitemradio"]') ?? [])
  }

  async function show() {
    open = true
    await tick()
    const all = items()
    ;(all.find((el) => el.getAttribute('aria-checked') === 'true') ?? all[0])?.focus()
  }

  function close(refocus = true) {
    open = false
    if (refocus) trigger.focus()
  }

  function onKey(event: KeyboardEvent) {
    const all = items()
    const index = all.indexOf(document.activeElement as HTMLElement)
    if (event.key === 'ArrowDown') all[(index + 1) % all.length]?.focus()
    else if (event.key === 'ArrowUp') all[(index - 1 + all.length) % all.length]?.focus()
    else if (event.key === 'Home') all[0]?.focus()
    else if (event.key === 'End') all[all.length - 1]?.focus()
    else if (event.key === 'Escape') close()
    else if (event.key === 'Tab') close(false)
    else return
    event.preventDefault()
  }

  function choose(action: () => void) {
    close()
    action()
  }
</script>

<svelte:window onclick={(e) => open && !menu?.contains(e.target as Node) && !trigger.contains(e.target as Node) && close(false)} />

<div class="switcher">
  <h1>
    <button
      bind:this={trigger}
      class="trigger"
      aria-haspopup="menu"
      aria-expanded={open}
      onclick={() => (open ? close() : show())}
      onkeydown={(e) => e.key === 'ArrowDown' && (e.preventDefault(), show())}
    >
      <span class="name">{name}</span>
      <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 4.5l3 3 3-3" /></svg>
    </button>
  </h1>

  {#if open}
    <div class="menu" role="menu" aria-label="Packs" tabindex="-1" bind:this={menu} onkeydown={onKey}>
      {#each list?.packs ?? [] as pack (pack.dir)}
        <button
          role="menuitemradio"
          aria-checked={pack.dir === list?.active}
          tabindex="-1"
          onclick={() => choose(() => onswitch(pack.dir))}
        >
          <span class="name">{pack.name}</span>
          <span class="meta">{pack.minecraft} {pack.loader ?? ''}</span>
        </button>
      {/each}
      {#if list?.packs.length}<div class="separator" role="separator"></div>{/if}
      <button role="menuitem" tabindex="-1" onclick={() => choose(onadd)}>Add a pack</button>
    </div>
  {/if}
</div>

<style>
  .switcher {
    position: relative;
  }

  .trigger {
    display: flex;
    align-items: start;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    padding: 4px 6px;
    margin: -4px -6px;
    text-align: left;
    background: transparent;
    border-color: transparent;
    font: inherit;
  }

  h1 {
    font-weight: 800;
    font-size: var(--step-2);
    line-height: 1.15;
    letter-spacing: -0.01em;
  }

  .trigger:hover {
    background: var(--rail-hi);
  }

  .trigger .name {
    overflow-wrap: anywhere;
  }

  svg {
    flex: none;
    width: 14px;
    height: 14px;
    margin-top: 10px;
    fill: none;
    stroke: var(--ash);
    stroke-width: 1.6;
  }

  .menu {
    position: absolute;
    z-index: 10;
    top: calc(100% + 6px);
    left: -6px;
    right: -6px;
    display: grid;
    padding: 6px;
    background: var(--quill);
    border: 1px solid var(--quill-hi);
    border-radius: var(--radius-panel);
    box-shadow: 0 12px 32px rgb(5 8 16 / 0.55);
  }

  .menu button {
    display: grid;
    gap: 1px;
    text-align: left;
    font-weight: 400;
    background: transparent;
    border-color: transparent;
  }

  .menu button:hover {
    background: var(--quill-hi);
  }

  .menu button:focus-visible {
    background: var(--quill-hi);
    outline: 2px solid var(--teal);
    outline-offset: -2px;
  }

  .menu [aria-checked='true'] .name {
    font-weight: 700;
    color: var(--teal);
  }

  .meta {
    font-size: var(--step--1);
    color: var(--ash);
  }

  .separator {
    height: 1px;
    margin: 6px 4px;
    background: var(--quill-hi);
  }
</style>
