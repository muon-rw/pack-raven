<script lang="ts">
  import { motdLines } from '../lib/motd'

  let { icon, name, motd, maxPlayers }: { icon: string | null; name: string; motd: string; maxPlayers: string } = $props()

  // Vanilla wraps the MOTD to the row and shows the first two lines, so later lines never matter.
  let lines = $derived(motdLines(motd).slice(0, 2))
  let obfuscated = $derived(lines.some((runs) => runs.some((run) => run.obfuscated)))

  const noise = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789#$%&?'
  let tick = $state(0)
  $effect(() => {
    if (!obfuscated || matchMedia('(prefers-reduced-motion: reduce)').matches) return
    const timer = setInterval(() => tick++, 90)
    return () => clearInterval(timer)
  })

  function scramble(text: string, seed: number) {
    return [...text].map((c, i) => (c === ' ' ? c : noise[(i * 31 + seed * 17 + c.charCodeAt(0)) % noise.length])).join('')
  }
</script>

<div class="entry" role="img" aria-label="{name} in a player's server list: {motd.replace(/§./g, '').replace(/\n/g, ', ') || 'no message'}, 0 of {maxPlayers} players online">
  {#if icon}
    <img src={icon} alt="" width="64" height="64" />
  {:else}
    <svg class="blank" viewBox="0 0 64 64" aria-hidden="true">
      <rect x="0.5" y="0.5" width="63" height="63" rx="1" />
      <path d="M20 40 28 30l6 7 4-5 6 8z" />
      <circle cx="40" cy="24" r="3.5" />
    </svg>
  {/if}
  <div class="text">
    <div class="top">
      <span class="name">{name}</span>
      <span class="players">0<span class="slash">/</span>{maxPlayers || '20'}</span>
      <svg class="ping" viewBox="0 0 10 8" shape-rendering="crispEdges" aria-hidden="true">
        {#each [2, 3, 5, 6, 8] as height, bar (bar)}<rect x={bar * 2} y={8 - height} width="1" {height} />{/each}
      </svg>
    </div>
    <div class="motd">
      {#each lines as runs, index (index)}{#if index}{'\n'}{/if}{#each runs as run, i (i)}<span
            style:color={run.color}
            class:bold={run.bold}
            class:italic={run.italic}
            class:underline={run.underline}
            class:strike={run.strike}>{run.obfuscated ? scramble(run.text, tick) : run.text}</span
          >{/each}{/each}
    </div>
  </div>
</div>

<style>
  /* Idrees Hassan's OFL rendition of vanilla's font, since Mojang's own glyphs can't ship with us. */
  @font-face {
    font-family: 'packraven Minecraft';
    src: url('../assets/fonts/minecraft/Minecraft.otf') format('opentype');
    font-weight: 400;
  }

  @font-face {
    font-family: 'packraven Minecraft';
    src: url('../assets/fonts/minecraft/Minecraft-Bold.otf') format('opentype');
    font-weight: 700;
  }

  /* We draw vanilla's dark multiplayer list in both themes, since that's where players see this.
     Sizes follow vanilla's entry at GUI scale 2: the font's 18-unit em puts one game pixel on two CSS
     pixels at 18px, and vanilla sets the MOTD lines 12 and 21 game pixels below the name's row. */
  .entry {
    width: 612px;
    max-width: 100%;
    display: grid;
    grid-template-columns: 64px minmax(0, 1fr);
    gap: 6px;
    padding: 4px;
    background: #10120d;
    border: 1px solid #3e4637;
    border-radius: var(--radius-small);
    font-family: 'packraven Minecraft', var(--font-ui);
    font-size: 18px;
    line-height: 18px;
    font-synthesis: style;
  }

  img,
  .blank {
    width: 64px;
    height: 64px;
    image-rendering: pixelated;
  }

  .blank rect {
    fill: #22261e;
    stroke: #3e4637;
  }

  .blank path,
  .blank circle {
    fill: #595d4f;
  }

  .text {
    min-width: 0;
  }

  .top {
    display: flex;
    align-items: start;
    gap: 4px;
    margin: 2px 0 4px;
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #ffffff;
  }

  .players {
    color: #aaaaaa;
  }

  .slash {
    color: #555555;
  }

  .ping {
    flex: none;
    width: 20px;
    height: 16px;
    margin-left: 2px;
    fill: #55ff55;
  }

  /* Vanilla wraps at 271 game pixels and draws two lines. */
  .motd {
    max-width: 542px;
    height: 36px;
    overflow: hidden;
    white-space: pre-wrap;
  }

  .bold {
    font-weight: 700;
  }

  .italic {
    font-style: italic;
  }

  .underline {
    text-decoration: underline;
  }

  .strike {
    text-decoration: line-through;
  }

  .underline.strike {
    text-decoration: underline line-through;
  }
</style>
