<script lang="ts">
  import { api } from './api'
  import { fmtTime } from './format'
  import Icon from './Icon.svelte'
  import { store } from './store.svelte'
  import type { PlayerEvent, QueueItem } from './types'

  type Entry =
    | { type: 'track'; id: string; at: number; item: QueueItem }
    | { type: 'event'; id: string; at: number; ev: PlayerEvent; count: number }

  /** Same action by the same person (e.g. soundboard spam) collapses into one line. */
  const sameAction = (a: PlayerEvent, b: PlayerEvent) => a.by === b.by && a.kind === b.kind && describe(a) === describe(b)

  const entries = $derived.by(() => {
    const all: Entry[] = [
      ...(store.snap?.history ?? []).map((item): Entry => ({ type: 'track', id: item.id, at: item.played_at ?? item.added_at, item })),
      ...(store.snap?.events ?? []).map((ev): Entry => ({ type: 'event', id: ev.id, at: ev.at, ev, count: 1 })),
    ].sort((a, b) => b.at - a.at)
    const out: Entry[] = []
    for (const e of all) {
      const prev = out[out.length - 1]
      if (e.type === 'event' && prev?.type === 'event' && sameAction(prev.ev, e.ev)) prev.count++
      else out.push(e)
    }
    return out
  })

  const icons: Record<PlayerEvent['kind'], string> = {
    pause: 'pause', resume: 'play', auto_pause: 'pause', auto_resume: 'play',
    skip: 'skip', prev: 'prev', stop: 'stop', clear: 'trash', remove: 'x', sound: 'volume',
  }

  function describe(ev: PlayerEvent): string {
    switch (ev.kind) {
      case 'pause': return 'paused'
      case 'resume': return 'resumed'
      case 'auto_pause': return 'Paused, nobody listening'
      case 'auto_resume': return 'Resumed, listeners are back'
      case 'skip': return `skipped ${ev.title}`
      case 'prev': return `went back to ${ev.title}`
      case 'stop': return 'stopped playback'
      case 'clear': return `cleared the queue (${ev.count})`
      case 'remove': return `removed ${ev.title}`
      case 'sound': return `played ${ev.name}`
    }
  }

  const clock = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  const stamp = (ms: number) => new Date(ms).toLocaleString()
</script>

{#if !entries.length}
  <div class="empty faint">Nothing played yet.</div>
{:else}
  <ul>
    {#each entries as e, i (e.id)}
      {#if e.type === 'track'}
        {@const item = e.item}
        <li class="track rise" style="animation-delay: {Math.min(i, 12) * 20}ms" title={stamp(e.at)}>
          {#if item.track.thumbnail}<img src={item.track.thumbnail} alt="" loading="lazy" />{:else}<span class="ph"><Icon name="music" /></span>{/if}
          <div class="meta">
            <div class="t ellipsis" title={item.track.title}>{item.track.title}</div>
            <div class="s muted ellipsis">{item.track.artist ?? ''} · {fmtTime(item.track.duration)}{item.requested_by ? ` · ${item.requested_by}` : ''}</div>
          </div>
          <div class="acts">
            <button class="icon ghost" title="queue again" onclick={() => store.runAs((who) => api.add({ track: item.track, requested_by: who }), 'Queued again')}><Icon name="plus" size={16} /></button>
          </div>
        </li>
      {:else if e.ev.kind === 'sound'}
        <li class="ev sound rise" style="animation-delay: {Math.min(i, 12) * 20}ms" title={stamp(e.at)}>
          <span class="emoji">{e.ev.emoji ?? '🔈'}</span>
          <div class="line ellipsis"><b>{e.ev.by ?? 'Someone'}</b> played <span class="sname">{e.ev.name}</span>{#if e.count > 1}<span class="times">×{e.count}</span>{/if}</div>
          <span class="when">{clock(e.at)}</span>
        </li>
      {:else}
        {@const auto = e.ev.kind === 'auto_pause' || e.ev.kind === 'auto_resume'}
        <li class="ev rise" class:auto style="animation-delay: {Math.min(i, 12) * 20}ms" title={stamp(e.at)}>
          <span class="glyph"><Icon name={icons[e.ev.kind]} size={13} /></span>
          <div class="line ellipsis">{#if !auto}<b>{e.ev.by ?? 'Someone'}</b> {/if}{describe(e.ev)}{#if e.count > 1}<span class="times">×{e.count}</span>{/if}</div>
          <span class="when">{clock(e.at)}</span>
        </li>
      {/if}
    {/each}
  </ul>
{/if}

<style>
  .empty { padding: 24px 8px; text-align: center; font-size: 14px; }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
  li { display: grid; grid-template-columns: 48px 1fr auto; gap: 10px; align-items: center; padding: 6px; border-radius: 10px; }
  li.track:hover { background: var(--surface-2); }
  img, .ph { width: 48px; height: 32px; object-fit: cover; border-radius: 5px; background: var(--surface-2); display: flex; align-items: center; justify-content: center; color: var(--muted); }
  .meta { min-width: 0; }
  .t { font-size: 14px; font-weight: 500; }
  .s { font-size: 12px; }
  .acts { display: flex; }
  .acts .icon { width: 30px; height: 30px; }

  /* actions: quiet one-liners hanging off a thin timeline rule */
  .ev { padding: 2px 6px; font-size: 12.5px; color: var(--muted); }
  .glyph { justify-self: center; display: flex; align-items: center; justify-content: center; width: 22px; height: 22px; border-radius: 50%; color: var(--faint); border: 1px dashed var(--border-strong); }
  .line { min-width: 0; }
  .line b { color: var(--text); font-weight: 600; }
  .ev.auto { font-style: italic; color: var(--faint); }
  .when { font-family: var(--font-mono); font-size: 10.5px; color: var(--faint); }
  .times { margin-left: 6px; font-family: var(--font-mono); font-size: 11px; color: var(--accent); font-style: normal; }

  /* soundboard: a little louder */
  .ev.sound { padding: 5px 6px; margin: 2px 0; background: linear-gradient(90deg, rgba(255, 122, 26, 0.12), transparent 75%); border-left: 2px solid var(--accent); border-radius: 4px 10px 10px 4px; }
  .sound .emoji { justify-self: center; font-size: 20px; line-height: 1; transform: rotate(-6deg); }
  .sname { color: var(--accent-2); font-weight: 600; }
</style>
