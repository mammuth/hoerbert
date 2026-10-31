<script lang="ts">
  import { api } from './api'
  import { fmtTime, fmtTotal } from './format'
  import Icon from './Icon.svelte'
  import { store } from './store.svelte'
  import type { QueueItem } from './types'

  const queue = $derived(store.snap?.queue ?? [])
  const total = $derived(queue.reduce((a, i) => a + (i.track.duration ?? 0), 0))

  let dragId = $state<string | null>(null)
  let overIndex = $state<number | null>(null)

  function onDragStart(e: DragEvent, item: QueueItem) {
    dragId = item.id
    e.dataTransfer?.setData('text/plain', item.id)
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
  }
  function onDragOver(e: DragEvent, index: number) {
    e.preventDefault()
    overIndex = index
  }
  async function onDrop(e: DragEvent, index: number) {
    e.preventDefault()
    const id = dragId ?? e.dataTransfer?.getData('text/plain')
    dragId = null; overIndex = null
    if (!id) return
    const from = queue.findIndex((q) => q.id === id)
    if (from === -1 || from === index) return
    await store.run(api.queueMove(id, index > from ? index - 1 : index))
  }
  function move(item: QueueItem, dir: -1 | 1) {
    const i = queue.findIndex((q) => q.id === item.id)
    const to = Math.max(0, Math.min(queue.length - 1, i + dir))
    if (to !== i) store.run(api.queueMove(item.id, to))
  }
</script>

<section class="queue card rise" style="animation-delay: 120ms">
  <header>
    <h3>Up next <span class="count mono muted">{queue.length}{total ? ` · ${fmtTotal(total)}` : ''}</span></h3>
    <div class="acts">
      <button class="ghost danger" title="clear queue" disabled={!queue.length} onclick={() => { if (confirm('Clear the whole queue?')) store.run(api.queueClear()) }}><Icon name="trash" /> Clear</button>
    </div>
  </header>

  {#if !queue.length}
    <div class="empty faint">Queue is empty. Add something above — or type <span class="mono">!play &lt;song&gt;</span> in TeamSpeak.</div>
  {:else}
    <ol>
      {#each queue as item, i (item.id)}
        {#if i > 0 && !item.request && queue[i - 1].request}
          <li class="divider mono faint" aria-hidden="true">then from playlists &amp; mixes</li>
        {/if}
        <li
          class="rise"
          class:over={overIndex === i}
          class:dragging={dragId === item.id}
          style="animation-delay: {Math.min(i, 12) * 25}ms"
          draggable="true"
          ondragstart={(e) => onDragStart(e, item)}
          ondragover={(e) => onDragOver(e, i)}
          ondragleave={() => (overIndex = null)}
          ondrop={(e) => onDrop(e, i)}
          ondragend={() => { dragId = null; overIndex = null }}
        >
          <span class="n mono muted">{i + 1}</span>
          <button class="thumb" title="play now" onclick={() => store.run(api.queuePlay(item.id))}>
            {#if item.track.thumbnail}<img src={item.track.thumbnail} alt="" loading="lazy" />{:else}<Icon name="music" />{/if}
            <span class="play"><Icon name="play" /></span>
          </button>
          <div class="meta">
            <div class="t ellipsis" title={item.track.title}>{item.track.title}</div>
            <div class="s muted ellipsis">{item.track.artist ?? ''}{item.requested_by ? ` · ${item.requested_by}` : ''}</div>
          </div>
          <span class="dur mono muted">{fmtTime(item.track.duration)}</span>
          <div class="row-acts">
            <button class="icon ghost" title="move up" disabled={i === 0} onclick={() => move(item, -1)}><Icon name="up" size={16} /></button>
            <button class="icon ghost" title="move down" disabled={i === queue.length - 1} onclick={() => move(item, 1)}><Icon name="down" size={16} /></button>
            <button class="icon ghost danger" title="remove" onclick={() => store.run(api.queueRemove(item.id))}><Icon name="x" size={16} /></button>
          </div>
        </li>
      {/each}
    </ol>
  {/if}
</section>

<style>
  .queue { padding: 16px; display: flex; flex-direction: column; gap: 10px; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 8px; flex-wrap: wrap; }
  h3 { font-size: 18px; display: flex; align-items: baseline; gap: 10px; }
  .count { font-size: 12px; font-weight: 400; }
  .acts { display: flex; gap: 2px; }
  .empty { padding: 24px 8px; text-align: center; font-size: 14px; }
  ol { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
  li { display: grid; grid-template-columns: 24px 56px 1fr auto auto; gap: 10px; align-items: center; padding: 6px 8px 6px 4px; border-radius: 10px; border: 1px solid transparent; }
  li:hover { background: var(--surface-2); }
  li.over { border-color: var(--accent); background: rgba(255, 122, 26, 0.06); }
  li.dragging { opacity: 0.4; }
  li.divider { display: block; margin: 6px 0 2px; padding: 8px 8px 2px 38px; border-top: 1px dashed var(--border-strong); border-radius: 0; font-size: 11px; letter-spacing: 0.08em; text-transform: uppercase; }
  li.divider:hover { background: none; }
  .n { text-align: right; font-size: 12px; }
  .thumb { position: relative; width: 56px; height: 36px; padding: 0; border: none; border-radius: 6px; overflow: hidden; background: var(--surface-2); color: var(--muted); justify-content: center; }
  .thumb img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .thumb .play { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; background: rgba(0, 0, 0, 0.55); color: #fff; opacity: 0; transition: opacity 0.15s; }
  .thumb:hover .play { opacity: 1; }
  .meta { min-width: 0; }
  .t { font-weight: 500; font-size: 14px; }
  .s { font-size: 12px; }
  .dur { font-size: 12px; }
  .row-acts { display: flex; gap: 0; opacity: 0.35; transition: opacity 0.15s; }
  li:hover .row-acts { opacity: 1; }
  .row-acts .icon { width: 30px; height: 30px; }
  @media (max-width: 560px) {
    li { grid-template-columns: 56px 1fr auto; }
    .n, .dur { display: none; }
    .row-acts { opacity: 1; grid-column: 1 / -1; justify-content: flex-end; }
  }
</style>
