<script module lang="ts">
  // Shared by every preview this session, so flipping through same-sized textures keeps the
  // zoom you picked. 'fit' remembers that you chose to fit the window.
  const rememberedZooms = new Map<string, number | 'fit'>()
</script>

<script lang="ts">
  import { untrack } from 'svelte'
  import { api, errorText } from '../lib/api'
  import { mediaUrl } from '../lib/viewers'

  let { path, archived }: { path: string; archived: boolean } = $props()

  const ZOOMS = [0.125, 0.25, 0.5, 1, 2, 3, 4, 6, 8, 12, 16, 24, 32]
  const TICK_MS = 50
  // Fitting a 16 px texture to the window blows it up past 3000%, so a fresh preview starts
  // no larger than this.
  const DEFAULT_ZOOM = 6

  type Frame = { index: number; time: number }
  type Animation = { width: number; height: number; columns: number; frames: Frame[]; interpolate: boolean }

  let url = $state<string | null>(null)
  let error = $state<string | null>(null)
  let natural = $state<{ width: number; height: number } | null>(null)
  let mcmeta = $state<Record<string, unknown> | null>(null)
  let chosenZoom = $state<number | 'fit' | null>(null)
  let playing = $state(true)
  let showSheet = $state(false)
  let frame = $state(0)
  let stageWidth = $state(0)
  let stageHeight = $state(0)
  let image = $state<HTMLImageElement>()
  let canvas = $state<HTMLCanvasElement>()

  $effect(() => {
    const current = path
    let cancelled = false
    let release = () => {}
    url = null
    error = null
    natural = null
    mcmeta = null
    frame = 0
    showSheet = false
    mediaUrl(current, archived).then(
      (loaded) => {
        if (cancelled) return loaded.release()
        release = loaded.release
        url = loaded.url
      },
      (e) => (error = errorText(e)),
    )
    // Minecraft animates a texture when a .mcmeta file with an "animation" section sits next to it.
    api.readFile(`${current}.mcmeta`).then(
      (content) => {
        if (cancelled || !content.text) return
        try {
          const parsed = JSON.parse(content.text)
          if (parsed?.animation) mcmeta = parsed.animation
        } catch {
          // A broken .mcmeta file doesn't stop the image from showing.
        }
      },
      () => {},
    )
    return () => {
      cancelled = true
      release()
    }
  })

  // Minecraft cuts frames as squares unless the .mcmeta file gives a size, and plays them in
  // order, one game tick (50 ms) per frametime, unless it lists frames of its own.
  let animation = $derived.by((): Animation | null => {
    if (!mcmeta || !natural) return null
    const given = (key: string) => (typeof mcmeta![key] === 'number' ? (mcmeta![key] as number) : null)
    const side = Math.min(natural.width, natural.height)
    const width = given('width') ?? (given('height') ? natural.width : side)
    const height = given('height') ?? (given('width') ? natural.height : side)
    const columns = Math.max(1, Math.floor(natural.width / width))
    const count = columns * Math.max(1, Math.floor(natural.height / height))
    const frametime = Math.max(1, given('frametime') ?? 1)
    const listed = Array.isArray(mcmeta.frames) ? (mcmeta.frames as unknown[]) : null
    const frames = (listed ?? [...Array(count).keys()])
      .map((f) => (typeof f === 'number' ? { index: f, time: frametime } : { index: Number((f as Frame).index), time: Math.max(1, Number((f as Frame).time ?? frametime)) }))
      .filter((f) => Number.isInteger(f.index) && f.index >= 0 && f.index < count)
    return frames.length > 1 ? { width, height, columns, frames, interpolate: mcmeta.interpolate === true } : null
  })

  let info = $derived(
    [natural && `${natural.width} × ${natural.height} px`, animation && `${animation.frames.length} frames of ${animation.width} × ${animation.height}`].filter(Boolean).join(', '),
  )
  let animating = $derived(!!animation && !showSheet)
  let shown = $derived(animating ? { width: animation!.width, height: animation!.height } : natural)

  // Pixel art looks best at whole-number zoom, so we fit small images with the largest whole
  // number that fits, and only shrink large ones smoothly.
  let fitZoom = $derived.by(() => {
    if (!shown || !stageWidth || !stageHeight) return 1
    const scale = Math.min((stageWidth - 32) / shown.width, (stageHeight - 32) / shown.height)
    return scale >= 1 ? Math.min(Math.floor(scale), 32) : scale
  })
  let zoom = $derived(chosenZoom === 'fit' ? fitZoom : (chosenZoom ?? Math.min(fitZoom, DEFAULT_ZOOM)))

  // We key the remembered zoom on the size on screen, so an animated 16 × 64 strip, which shows
  // 16 × 16 frames, shares its zoom with plain 16 × 16 textures.
  let sizeKey = $derived(shown ? `${shown.width}x${shown.height}` : null)

  $effect(() => {
    const key = sizeKey
    if (key) chosenZoom = rememberedZooms.get(key) ?? null
  })

  function choose(next: number | 'fit') {
    chosenZoom = next
    if (sizeKey) rememberedZooms.set(sizeKey, next)
  }

  function step(direction: 1 | -1) {
    const next = direction > 0 ? ZOOMS.find((z) => z > zoom + 1e-6) : ZOOMS.findLast((z) => z < zoom - 1e-6)
    if (next) choose(next)
  }

  function onWheel(event: WheelEvent) {
    if (!event.ctrlKey && !event.metaKey) return
    event.preventDefault()
    step(event.deltaY < 0 ? 1 : -1)
  }

  function draw(index: number, blend: number) {
    const context = canvas?.getContext('2d')
    if (!context || !image || !animation) return
    const { width, height, columns, frames } = animation
    const at = (i: number) => {
      const sheetIndex = frames[i].index
      return [(sheetIndex % columns) * width, Math.floor(sheetIndex / columns) * height] as const
    }
    context.clearRect(0, 0, width, height)
    context.globalAlpha = 1
    context.drawImage(image, ...at(index), width, height, 0, 0, width, height)
    if (blend > 0) {
      context.globalAlpha = blend
      context.drawImage(image, ...at((index + 1) % frames.length), width, height, 0, 0, width, height)
    }
  }

  $effect(() => {
    if (!animating || !animation) return
    const { frames, interpolate } = animation
    if (!playing) {
      draw(frame, 0)
      return
    }
    let index = untrack(() => frame)
    let elapsed = 0
    let last = performance.now()
    let handle = requestAnimationFrame(function tick(now) {
      elapsed += (now - last) / TICK_MS
      last = now
      while (elapsed >= frames[index].time) {
        elapsed -= frames[index].time
        index = (index + 1) % frames.length
      }
      frame = index
      draw(index, interpolate ? elapsed / frames[index].time : 0)
      handle = requestAnimationFrame(tick)
    })
    return () => cancelAnimationFrame(handle)
  })

  function stepFrame(direction: 1 | -1) {
    if (!animation) return
    playing = false
    frame = (frame + direction + animation.frames.length) % animation.frames.length
    draw(frame, 0)
  }
</script>

<div class="viewer">
  <div class="bar">
    <span class="muted">{info}</span>
    <span class="controls">
      {#if animation}
        <button class="ghost" onclick={() => (showSheet = !showSheet)}>{showSheet ? 'Show animation' : 'Show sheet'}</button>
        {#if !showSheet}
          <button class="ghost" aria-label="Previous frame" title="Previous frame" onclick={() => stepFrame(-1)}>‹</button>
          <button class="ghost play" onclick={() => (playing = !playing)}>{playing ? 'Pause' : 'Play'}</button>
          <button class="ghost" aria-label="Next frame" title="Next frame" onclick={() => stepFrame(1)}>›</button>
          <span class="muted frame">Frame {frame + 1} of {animation.frames.length}</span>
        {/if}
        <span class="divider" aria-hidden="true"></span>
      {/if}
      <button class="ghost" aria-label="Zoom out" title="Zoom out (Ctrl+scroll)" onclick={() => step(-1)}>−</button>
      <span class="zoom">{Math.round(zoom * 100)}%</span>
      <button class="ghost" aria-label="Zoom in" title="Zoom in (Ctrl+scroll)" onclick={() => step(1)}>+</button>
      <button class="ghost" aria-pressed={zoom === fitZoom} onclick={() => choose('fit')}>Fit</button>
      <button class="ghost" aria-pressed={zoom === 1} onclick={() => choose(1)}>1:1</button>
    </span>
  </div>
  <div class="stage" bind:clientWidth={stageWidth} bind:clientHeight={stageHeight} onwheel={onWheel}>
    {#if error}
      <p class="failed">{error}</p>
    {:else if url}
      <div class="frame-box" class:crisp={zoom >= 1} style:width="{(shown?.width ?? 0) * zoom}px" style:height="{(shown?.height ?? 0) * zoom}px">
        <img
          bind:this={image}
          src={url}
          alt=""
          class:hidden={animating}
          onload={() => (natural = { width: image!.naturalWidth || 300, height: image!.naturalHeight || 150 })}
          onerror={() => (error = "This image couldn't be shown. It may be damaged or in a format the webview can't read.")}
        />
        {#if animating && animation}
          <canvas bind:this={canvas} width={animation.width} height={animation.height}></canvas>
        {/if}
      </div>
    {:else}
      <p class="muted">Loading the image…</p>
    {/if}
  </div>
</div>

<style>
  .viewer {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .bar {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: 6px 12px;
    padding: 6px 10px 6px 14px;
    font-size: var(--step--1);
    border-bottom: 1px solid var(--line);
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .ghost {
    min-width: 28px;
    padding: 2px 8px;
    font-weight: 400;
    font-size: var(--step--1);
    background: transparent;
    border-color: transparent;
  }

  .ghost:hover:not(:disabled) {
    background: var(--control);
  }

  .ghost[aria-pressed='true'] {
    background: var(--here);
    border-color: var(--here-border);
  }

  .play {
    min-width: 54px;
  }

  .zoom,
  .frame {
    min-width: 4.5ch;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }

  .frame {
    min-width: 12ch;
  }

  .divider {
    width: 1px;
    height: 18px;
    margin: 0 6px;
    background: var(--line);
  }

  .stage {
    flex: 1;
    min-height: 0;
    display: grid;
    place-items: center;
    overflow: auto;
    padding: 16px;
  }

  /* A checkerboard shows which pixels are transparent. */
  .frame-box {
    position: relative;
    flex: none;
    background-color: var(--field);
    background-image:
      linear-gradient(45deg, var(--surface) 25%, transparent 25%, transparent 75%, var(--surface) 75%),
      linear-gradient(45deg, var(--surface) 25%, transparent 25%, transparent 75%, var(--surface) 75%);
    background-size: 16px 16px;
    background-position:
      0 0,
      8px 8px;
    box-shadow: 0 0 0 1px var(--line);
  }

  .frame-box img,
  .frame-box canvas {
    display: block;
    width: 100%;
    height: 100%;
  }

  .frame-box.crisp img,
  .frame-box.crisp canvas {
    image-rendering: pixelated;
  }

  .hidden {
    display: none !important;
  }
</style>
