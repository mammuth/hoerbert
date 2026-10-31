<script lang="ts">
  import { api } from './api'
  import { fmtTime, sourceLabel } from './format'
  import Icon from './Icon.svelte'
  import Vinyl from './Vinyl.svelte'
  import { store } from './store.svelte'

  const pb = $derived(store.snap?.playback)
  const item = $derived(pb?.item)
  const track = $derived(item?.track)
  const duration = $derived(track?.duration)
  const position = $derived(store.position)
  const progress = $derived(duration ? Math.min(100, (position / duration) * 100) : 0)
  const playing = $derived(pb?.status === 'playing')

  let scrubbing = $state(false)
  let scrubPos = $state(0)

  function barPos(e: PointerEvent, el: HTMLElement): number {
    const r = el.getBoundingClientRect()
    const f = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width))
    return f * (duration ?? 0)
  }

  function onBarDown(e: PointerEvent) {
    if (!duration) return
    const el = e.currentTarget as HTMLElement
    el.setPointerCapture(e.pointerId)
    scrubbing = true
    scrubPos = barPos(e, el)
  }
  function onBarMove(e: PointerEvent) {
    if (!scrubbing) return
    scrubPos = barPos(e, e.currentTarget as HTMLElement)
  }
  async function onBarUp(e: PointerEvent) {
    if (!scrubbing) return
    scrubbing = false
    const pos = barPos(e, e.currentTarget as HTMLElement)
    await store.run(api.seek(pos))
  }

  const shownPos = $derived(scrubbing ? scrubPos : position)
</script>

<section class="card now rise">
  <div class="art">
    <Vinyl thumbnail={track?.thumbnail} spinning={playing} />
  </div>
  <div class="info">
    <div class="eyebrow">
      {#if pb?.status === 'loading'}
        <span class="dot loading"></span> loading
      {:else if pb?.status === 'playing'}
        <span class="dot live"></span> now playing
      {:else if pb?.status === 'paused'}
        <span class="dot"></span> paused
      {:else}
        <span class="dot"></span> idle
      {/if}
      {#if track?.source}<span class="faint">· {sourceLabel(track.source)}</span>{/if}
      {#if item?.requested_by}<span class="faint">· by {item.requested_by}</span>{/if}
    </div>
    {#if track}
      <h2 class="title" title={track.title}>
        <a href={track.url} target="_blank" rel="noreferrer">{track.title}</a>
      </h2>
      <div class="artist muted ellipsis">{track.artist ?? ''}</div>
    {:else}
      <h2 class="title muted">Nothing on the turntable</h2>
      <div class="artist faint">Add a link or search below — the whole channel listens together.</div>
    {/if}

    {#if pb?.error}
      <div class="error">⚠ {pb.error}</div>
    {/if}

    <div class="bar-wrap">
      <div
        class="bar"
        class:disabled={!duration}
        role="slider"
        aria-label="seek"
        aria-valuemin="0"
        aria-valuemax={duration ?? 0}
        aria-valuenow={shownPos}
        tabindex="0"
        onpointerdown={onBarDown}
        onpointermove={onBarMove}
        onpointerup={onBarUp}
        onpointercancel={() => (scrubbing = false)}
      >
        {#if duration && pb}
          {#each pb.segments as seg (seg.start)}
            <div class="seg" title="{seg.category}: {fmtTime(seg.start)}–{fmtTime(seg.end)}" style:left="{(seg.start / duration) * 100}%" style:width="{((seg.end - seg.start) / duration) * 100}%"></div>
          {/each}
        {/if}
        <div class="fill" style:width="{scrubbing && duration ? (scrubPos / duration) * 100 : progress}%"></div>
        <div class="knob" style:left="{scrubbing && duration ? (scrubPos / duration) * 100 : progress}%"></div>
      </div>
      <div class="times mono">
        <span>{fmtTime(shownPos)}</span>
        <span>{track?.is_live ? 'LIVE' : duration ? fmtTime(duration) : '–:––'}</span>
      </div>
    </div>

    <div class="controls">
      <button class="icon ghost" title="previous" onclick={() => store.run(api.player('prev'))}><Icon name="prev" /></button>
      <button class="icon big primary" title={playing ? 'pause' : 'play'} onclick={() => store.run(api.player('toggle'))}>
        <Icon name={playing ? 'pause' : 'play'} size={26} />
      </button>
      <button class="icon ghost" title="skip" onclick={() => store.run(api.player('skip'))}><Icon name="skip" /></button>
      <button class="icon ghost" title="stop" onclick={() => store.run(api.player('stop'))}><Icon name="stop" /></button>
    </div>
  </div>
</section>

<style>
  .now { display: grid; grid-template-columns: 220px 1fr; gap: 28px; padding: 28px; align-items: center; overflow: hidden; position: relative; }
  .now::after {
    content: ''; position: absolute; inset: auto -20% -60% auto; width: 60%; height: 120%; pointer-events: none;
    background: radial-gradient(closest-side, rgba(255, 122, 26, 0.12), transparent); filter: blur(10px);
  }
  .art { display: flex; justify-content: center; }
  .info { min-width: 0; display: flex; flex-direction: column; gap: 6px; position: relative; }
  .eyebrow { font-size: 12px; text-transform: uppercase; letter-spacing: 0.14em; color: var(--muted); display: flex; gap: 8px; align-items: center; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--faint); display: inline-block; }
  .dot.live { background: var(--accent); box-shadow: 0 0 0 0 rgba(255, 122, 26, 0.6); animation: pulse 1.6s ease-out infinite; }
  .dot.loading { background: var(--accent-2); animation: blink 0.8s ease-in-out infinite alternate; }
  @keyframes pulse { to { box-shadow: 0 0 0 10px rgba(255, 122, 26, 0); } }
  @keyframes blink { to { opacity: 0.3; } }
  .title { font-size: clamp(20px, 2.6vw, 30px); line-height: 1.15; font-weight: 800; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .title a { color: inherit; text-decoration: none; }
  .title a:hover { color: var(--accent-2); }
  .artist { font-size: 15px; }
  .error { color: var(--danger); font-size: 13px; background: rgba(255, 92, 92, 0.08); border: 1px solid rgba(255, 92, 92, 0.25); padding: 6px 10px; border-radius: 8px; }

  .bar-wrap { margin-top: 10px; }
  .bar { position: relative; height: 22px; cursor: pointer; display: flex; align-items: center; touch-action: none; }
  .bar.disabled { cursor: default; }
  .bar::before { content: ''; position: absolute; left: 0; right: 0; height: 6px; border-radius: 6px; background: var(--border-strong); }
  .fill { position: absolute; left: 0; height: 6px; border-radius: 6px; background: linear-gradient(90deg, var(--accent), var(--accent-2)); }
  .seg { position: absolute; height: 6px; background: repeating-linear-gradient(135deg, rgba(255, 92, 92, 0.8) 0 3px, rgba(255, 92, 92, 0.3) 3px 6px); border-radius: 2px; }
  .knob { position: absolute; width: 14px; height: 14px; border-radius: 50%; background: var(--text); transform: translateX(-50%); box-shadow: 0 2px 8px rgba(0, 0, 0, 0.5); opacity: 0; transition: opacity 0.15s; }
  .bar:hover .knob, .bar:focus-visible .knob { opacity: 1; }
  .times { display: flex; justify-content: space-between; font-size: 12px; color: var(--muted); margin-top: 2px; }

  .controls { display: flex; align-items: center; gap: 6px; margin-top: 8px; flex-wrap: wrap; }
  .big { width: 52px; height: 52px; border-radius: 50%; box-shadow: 0 10px 30px -10px rgba(255, 122, 26, 0.8); }

  @media (max-width: 720px) {
    .now { grid-template-columns: 1fr; padding: 22px; gap: 18px; }
    .art { order: -1; }
  }
</style>
