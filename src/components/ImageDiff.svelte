<script lang="ts">
  import { api, errorText } from '../lib/api'
  import { blobUrl } from '../lib/viewers'
  import { shortcut } from '../lib/platform'

  let { path, origPath, staged, status, version }: { path: string; origPath: string | null; staged: boolean; status: string | null; version: string } = $props()

  const ZOOMS = [0.125, 0.25, 0.5, 1, 2, 3, 4, 6, 8, 12, 16, 24, 32]
  // Fitting a 16 px texture to half the pane blows it up past 2000%, so we stop fitting here.
  const MAX_FIT = 12
  // Pixels that didn't change stay faintly visible, so you can tell where the changed ones sit.
  const UNCHANGED_ALPHA = 0.18
  // We compare pixels on the UI thread, so past this size the comparison would freeze the window.
  const MAX_COMPARED_PIXELS = 4096 * 4096

  type Side = { url: string | null; error: string | null; size: number; width: number; height: number }
  type Mode = 'sides' | 'pixels'

  const empty = (): Side => ({ url: null, error: null, size: 0, width: 0, height: 0 })
  let before = $state<Side>(empty())
  let after = $state<Side>(empty())
  let chosenZoom = $state<number | null>(null)
  let mode = $state<Mode>('sides')
  let paneWidth = $state(0)
  let paneHeight = $state(0)
  let canvas = $state<HTMLCanvasElement>()
  let changedPixels = $state<number | null>(null)

  let isNew = $derived(status === '?' || status === 'A')
  let isDeleted = $derived(status === 'D')
  let single = $derived(isNew || isDeleted)

  // A staged diff compares the last commit with the index, and an unstaged one compares the index
  // with the file on disk.
  $effect(() => {
    void version
    const urls: string[] = []
    let cancelled = false
    const load = (side: 'before' | 'after', bytes: Promise<ArrayBuffer> | null) => {
      const set = (value: Side) => (side === 'before' ? (before = value) : (after = value))
      if (!bytes) return set(empty())
      bytes.then(
        (data) => {
          if (cancelled) return
          const url = blobUrl(path, data)
          urls.push(url)
          const previous = side === 'before' ? before : after
          set({ ...empty(), url, size: data.byteLength, width: previous.width, height: previous.height })
        },
        (e) => !cancelled && set({ ...empty(), error: errorText(e) }),
      )
    }
    load('before', isNew ? null : api.gitBlob(staged ? (origPath ?? path) : path, staged ? 'head' : 'index'))
    load('after', isDeleted ? null : staged ? api.gitBlob(path, 'index') : api.readBytes(path))
    return () => {
      cancelled = true
      for (const url of urls) URL.revokeObjectURL(url)
    }
  })

  let bothLoaded = $derived(!!before.width && !!after.width)
  let sizeChanged = $derived(bothLoaded && (before.width !== after.width || before.height !== after.height))
  let tooLarge = $derived(after.width * after.height > MAX_COMPARED_PIXELS)
  let canComparePixels = $derived(bothLoaded && !sizeChanged && !tooLarge)
  let showPixels = $derived(mode === 'pixels' && canComparePixels)

  let shown = $derived([before, after].filter((side) => side.url && side.width))
  let largest = $derived({ width: Math.max(1, ...shown.map((s) => s.width)), height: Math.max(1, ...shown.map((s) => s.height)) })

  // Both sides share one zoom, so a texture that changed size looks it. Like the image viewer, we
  // fit small images by whole numbers so pixel art stays crisp.
  let fitZoom = $derived.by(() => {
    if (!paneWidth || !paneHeight) return 1
    const scale = Math.min((paneWidth - 24) / largest.width, (paneHeight - 24) / largest.height)
    return scale >= 1 ? Math.min(Math.floor(scale), MAX_FIT) : scale
  })
  let zoom = $derived(chosenZoom ?? fitZoom)

  async function decoded(url: string, width: number, height: number) {
    const image = new Image()
    image.src = url
    await image.decode()
    const scratch = document.createElement('canvas')
    scratch.width = width
    scratch.height = height
    const context = scratch.getContext('2d', { willReadFrequently: true })!
    context.drawImage(image, 0, 0)
    return context.getImageData(0, 0, width, height)
  }

  $effect(() => {
    if (!showPixels || !canvas || !before.url || !after.url) return
    const { width, height } = after
    const target = canvas
    let cancelled = false
    Promise.all([decoded(before.url, width, height), decoded(after.url, width, height)]).then(([old, now]) => {
      if (cancelled) return
      const out = new ImageData(width, height)
      let changed = 0
      for (let i = 0; i < now.data.length; i += 4) {
        const same = now.data[i] === old.data[i] && now.data[i + 1] === old.data[i + 1] && now.data[i + 2] === old.data[i + 2] && now.data[i + 3] === old.data[i + 3]
        if (!same) changed++
        // A pixel the change made transparent shows its old color, so you can see what went.
        const source = !same && now.data[i + 3] === 0 ? old.data : now.data
        out.data.set(source.subarray(i, i + 4), i)
        if (same) out.data[i + 3] = Math.round(out.data[i + 3] * UNCHANGED_ALPHA)
      }
      target.getContext('2d')!.putImageData(out, 0, 0)
      changedPixels = changed
    })
    return () => {
      cancelled = true
    }
  })

  function step(direction: 1 | -1) {
    const next = direction > 0 ? ZOOMS.find((z) => z > zoom + 1e-6) : ZOOMS.findLast((z) => z < zoom - 1e-6)
    if (next) chosenZoom = next
  }

  function onWheel(event: WheelEvent) {
    if (!event.ctrlKey && !event.metaKey) return
    event.preventDefault()
    step(event.deltaY < 0 ? 1 : -1)
  }

  function formatSize(bytes: number) {
    if (bytes < 1024) return `${bytes} B`
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`
  }

  const dimensions = (side: Side) => `${side.width} × ${side.height}`

  let name = $derived(path.slice(path.lastIndexOf('/') + 1))
  let summary = $derived.by(() => {
    if (isNew) return 'New image'
    if (isDeleted) return 'Deleted image'
    if (sizeChanged) return `Resized from ${dimensions(before)} to ${dimensions(after)} px`
    if (showPixels && changedPixels !== null) {
      const share = (changedPixels / (after.width * after.height)) * 100
      return `${changedPixels} ${changedPixels === 1 ? 'pixel' : 'pixels'} changed (${share < 1 && changedPixels ? '<1' : Math.round(share)}%)`
    }
    return bothLoaded ? 'Same size, different pixels' : ''
  })
</script>

{#snippet frame(side: Side, label: string)}
  <div class="frame-box" class:crisp={zoom >= 1} style:width="{side.width * zoom}px" style:height="{side.height * zoom}px">
    <img
      src={side.url}
      alt="{name}, {label}"
      onload={(e) => {
        const image = e.currentTarget as HTMLImageElement
        side.width = image.naturalWidth || 300
        side.height = image.naturalHeight || 150
      }}
      onerror={() => (side.error = "This image couldn't be shown. It may be damaged or in a format the webview can't read.")}
    />
  </div>
{/snippet}

{#snippet pane(label: string, side: Side)}
  <figure class="pane">
    <figcaption>
      <span class="label">{label}</span>
      {#if side.url}
        <span class="info">
          {#if side.width}<span class:changed={sizeChanged}>{dimensions(side)} px</span>,{/if}
          {formatSize(side.size)}
        </span>
      {/if}
    </figcaption>
    <div class="stage" bind:clientWidth={paneWidth} bind:clientHeight={paneHeight} onwheel={onWheel}>
      {#if side.error}
        <p class="failed">{side.error}</p>
      {:else if side.url}
        {@render frame(side, label.toLowerCase())}
      {:else}
        <p class="muted">Loading…</p>
      {/if}
    </div>
  </figure>
{/snippet}

<div class="image-diff">
  <div class="bar">
    <span class="summary" aria-live="polite">{summary}</span>
    <span class="controls">
      {#if !single}
        <span class="modes" role="group" aria-label="Compare">
          <button class="ghost" aria-pressed={!showPixels} onclick={() => (mode = 'sides')}>Side by side</button>
          <button
            class="ghost"
            aria-pressed={showPixels}
            disabled={!canComparePixels}
            title={sizeChanged
              ? 'Only images of the same size can be compared pixel by pixel'
              : tooLarge
                ? 'This image is too large to compare pixel by pixel'
                : 'Fade the pixels that stayed the same'}
            onclick={() => (mode = 'pixels')}>Changed pixels</button
          >
        </span>
        <span class="divider" aria-hidden="true"></span>
      {/if}
      <button class="ghost" aria-label="Zoom out" title="Zoom out ({shortcut('Ctrl+scroll')})" onclick={() => step(-1)}>−</button>
      <span class="zoom" aria-live="polite"><span class="visually-hidden">Zoom </span>{Math.round(zoom * 100)}%</span>
      <button class="ghost" aria-label="Zoom in" title="Zoom in ({shortcut('Ctrl+scroll')})" onclick={() => step(1)}>+</button>
      <button class="ghost" aria-pressed={chosenZoom === null} onclick={() => (chosenZoom = null)}>Fit</button>
      <button class="ghost" aria-pressed={zoom === 1} onclick={() => (chosenZoom = 1)}>1:1</button>
    </span>
  </div>
  {#if showPixels}
    <figure class="pane">
      <figcaption><span class="label">Changed pixels</span><span class="info">Unchanged pixels are faded</span></figcaption>
      <div class="stage" bind:clientWidth={paneWidth} bind:clientHeight={paneHeight} onwheel={onWheel}>
        <div class="frame-box" class:crisp={zoom >= 1} style:width="{after.width * zoom}px" style:height="{after.height * zoom}px">
          <canvas bind:this={canvas} width={after.width} height={after.height}>{name}, changed pixels</canvas>
        </div>
      </div>
    </figure>
  {:else}
    <div class="panes" class:single>
      {#if !isNew}{@render pane('Before', before)}{/if}
      {#if !isDeleted}{@render pane('After', after)}{/if}
    </div>
  {/if}
</div>

<style>
  .image-diff {
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
    min-height: 44px;
    padding: 6px 10px 6px 14px;
    font-size: var(--step--1);
    border-bottom: 1px solid var(--line);
  }

  .summary {
    font-variant-numeric: tabular-nums;
  }

  .controls,
  .modes {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px;
  }

  .ghost {
    min-width: 28px;
  }

  .ghost[aria-pressed='true'] {
    background: var(--here);
    border-color: var(--here-border);
  }

  .divider {
    width: 1px;
    height: 18px;
    margin: 0 6px;
    background: var(--line);
  }

  .zoom {
    min-width: 5ch;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }

  .panes {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  }

  .panes.single {
    grid-template-columns: minmax(0, 1fr);
  }

  .pane {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    margin: 0;
  }

  .pane + .pane {
    border-left: 1px solid var(--line);
  }

  figcaption {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 2px 10px;
    padding: 8px 12px;
    font-size: var(--step--1);
  }

  .label {
    font-weight: 700;
  }

  .info {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .info .changed {
    color: var(--text);
    font-weight: 700;
  }

  .stage {
    flex: 1;
    min-height: 0;
    display: grid;
    place-items: center;
    overflow: auto;
    padding: 12px;
    text-align: center;
  }

  /* A checkerboard shows which pixels are transparent. */
  .frame-box {
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
</style>
