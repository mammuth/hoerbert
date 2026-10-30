<script lang="ts">
  import { api } from './api'
  import { fmtTime, fmtTotal } from './format'
  import Icon from './Icon.svelte'
  import { store } from './store.svelte'
  import type { Playlist, Track } from './types'

  let { pendingTrack = $bindable(null) }: { pendingTrack?: Track | null } = $props()

  const summaries = $derived(store.snap?.playlists ?? [])
  let selectedId = $state<string | null>(null)
  let selected = $state<Playlist | null>(null)
  let creating = $state(false)
  let newName = $state('')
  let importUrl = $state('')
  let addInput = $state('')
  let busy = $state(false)
  let renaming = $state(false)
  let renameValue = $state('')

  // Reload the open playlist whenever the server state changes.
  $effect(() => {
    const id = selectedId
    const _v = store.snap?.version
    if (!id) { selected = null; return }
    if (!summaries.some((p) => p.id === id)) { selectedId = null; return }
    api.playlist(id).then((p) => { if (selectedId === id) selected = p }).catch(() => {})
  })

  async function create() {
    if (!newName.trim() && !importUrl.trim()) return
    busy = true
    const pl = await store.run(api.playlistCreate({ name: newName.trim(), import: importUrl.trim() || undefined }), 'Playlist created')
    busy = false
    if (pl) { creating = false; newName = ''; importUrl = ''; selectedId = pl.id }
  }

  async function addToSelected(input: string) {
    if (!selected || !input.trim()) return
    busy = true
    const r = await store.run(api.playlistAddTracks(selected.id, { input: input.trim() }))
    busy = false
    if (r) { store.toast(`Added ${r.added} track${r.added === 1 ? '' : 's'}`); addInput = '' }
  }

  async function putPending(plId: string) {
    if (!pendingTrack) return
    const t = pendingTrack
    pendingTrack = null
    await store.run(api.playlistAddTracks(plId, { track: t }), `Added to playlist: ${t.title}`)
  }

  async function rename() {
    if (!selected || !renameValue.trim()) return
    await store.run(api.playlistRename(selected.id, renameValue.trim()))
    renaming = false
  }

  async function remove(pl: Playlist) {
    if (!confirm(`Delete playlist “${pl.name}”?`)) return
    await store.run(api.playlistDelete(pl.id), 'Playlist deleted')
    selectedId = null
  }

  const selDuration = $derived(selected?.tracks.reduce((a, t) => a + (t.duration ?? 0), 0) ?? 0)
</script>

{#if pendingTrack}
  <div class="pick rise">
    <div class="pick-head">
      <span class="ellipsis">Add <strong>{pendingTrack.title}</strong> to…</span>
      <button class="ghost icon" onclick={() => (pendingTrack = null)}><Icon name="x" size={14} /></button>
    </div>
    <div class="pick-list">
      {#each summaries as p (p.id)}
        <button onclick={() => putPending(p.id)}><Icon name="heart" size={14} /> {p.name} <span class="muted">· {p.count}</span></button>
      {/each}
      <button class="primary" onclick={async () => { const name = prompt('New playlist name'); if (!name) return; const pl = await store.run(api.playlistCreate({ name, tracks: [pendingTrack!] }), 'Playlist created'); if (pl) { pendingTrack = null; selectedId = pl.id } }}><Icon name="plus" size={14} /> New playlist</button>
    </div>
  </div>
{/if}

{#if selected}
  <div class="detail rise">
    <div class="detail-head">
      <button class="ghost" onclick={() => (selectedId = null)}>← all playlists</button>
      <div class="detail-acts">
        <button class="primary" disabled={!selected.tracks.length} onclick={() => store.run(api.playlistEnqueue(selected!.id, false, store.nick || undefined), `Queued ${selected!.tracks.length} tracks`)}><Icon name="play" /> Play all</button>
        <button disabled={!selected.tracks.length} onclick={() => store.run(api.playlistEnqueue(selected!.id, true, store.nick || undefined), 'Queued shuffled')}><Icon name="shuffle" /> Shuffle</button>
        <button class="icon ghost" title="rename" onclick={() => { renaming = true; renameValue = selected!.name }}><Icon name="edit" /></button>
        <button class="icon ghost danger" title="delete" onclick={() => remove(selected!)}><Icon name="trash" /></button>
      </div>
    </div>
    {#if renaming}
      <form class="inline" onsubmit={(e) => { e.preventDefault(); rename() }}>
        <input type="text" bind:value={renameValue} />
        <button class="primary" type="submit">Save</button>
        <button type="button" class="ghost" onclick={() => (renaming = false)}>Cancel</button>
      </form>
    {:else}
      <h3 class="pl-title">{selected.name} <span class="muted count mono">{selected.tracks.length} · {fmtTotal(selDuration)}</span></h3>
    {/if}
    <form class="inline" onsubmit={(e) => { e.preventDefault(); addToSelected(addInput) }}>
      <input type="text" bind:value={addInput} placeholder="Add a link, a whole playlist URL, or search terms…" />
      <button class="primary" type="submit" disabled={busy || !addInput.trim()}><Icon name="plus" /> Add</button>
    </form>
    {#if !selected.tracks.length}
      <div class="empty faint">Empty playlist.</div>
    {:else}
      <ol>
        {#each selected.tracks as t, i (t.url + i)}
          <li class="rise" style="animation-delay: {Math.min(i, 15) * 18}ms">
            <span class="n mono muted">{i + 1}</span>
            {#if t.thumbnail}<img src={t.thumbnail} alt="" loading="lazy" />{:else}<span class="ph"><Icon name="music" /></span>{/if}
            <div class="meta">
              <div class="t ellipsis" title={t.title}>{t.title}</div>
              <div class="s muted ellipsis">{t.artist ?? ''} · {fmtTime(t.duration)}</div>
            </div>
            <div class="acts">
              <button class="icon ghost" title="queue" onclick={() => store.run(api.add({ track: t, requested_by: store.nick || undefined }), `Queued: ${t.title}`)}><Icon name="plus" size={16} /></button>
              <button class="icon ghost danger" title="remove from playlist" onclick={() => store.run(api.playlistRemoveTrack(selected!.id, i))}><Icon name="x" size={16} /></button>
            </div>
          </li>
        {/each}
      </ol>
    {/if}
  </div>
{:else}
  <div class="list">
    {#if creating}
      <form class="create rise" onsubmit={(e) => { e.preventDefault(); create() }}>
        <input type="text" bind:value={newName} placeholder="Playlist name" />
        <input type="url" bind:value={importUrl} placeholder="Optional: import from a YouTube / SoundCloud playlist URL" />
        <div class="row">
          <button class="primary" type="submit" disabled={busy || (!newName.trim() && !importUrl.trim())}>{busy ? 'Importing…' : 'Create'}</button>
          <button type="button" class="ghost" onclick={() => (creating = false)}>Cancel</button>
        </div>
      </form>
    {:else}
      <button class="new" onclick={() => (creating = true)}><Icon name="plus" /> New playlist</button>
    {/if}
    {#if !summaries.length && !creating}
      <div class="empty faint">No playlists yet. Save the queue or import one from YouTube.</div>
    {/if}
    {#each summaries as p, i (p.id)}
      <div class="pl rise" style="animation-delay: {i * 40}ms">
        <button class="pl-main" onclick={() => (selectedId = p.id)}>
          <span class="pl-icon"><Icon name="list" /></span>
          <span class="pl-meta">
            <span class="t ellipsis">{p.name}</span>
            <span class="s muted">{p.count} track{p.count === 1 ? '' : 's'}{p.duration ? ` · ${fmtTotal(p.duration)}` : ''}</span>
          </span>
        </button>
        <button class="icon" title="play all" disabled={!p.count} onclick={() => store.run(api.playlistEnqueue(p.id, false, store.nick || undefined), `Queued ${p.count} tracks`)}><Icon name="play" /></button>
        <button class="icon ghost" title="shuffle play" disabled={!p.count} onclick={() => store.run(api.playlistEnqueue(p.id, true, store.nick || undefined), 'Queued shuffled')}><Icon name="shuffle" /></button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .list { display: flex; flex-direction: column; gap: 6px; }
  .new { justify-content: center; border-style: dashed; padding: 12px; color: var(--muted); }
  .new:hover { color: var(--text); border-color: var(--accent); }
  .create { display: flex; flex-direction: column; gap: 8px; padding: 12px; background: var(--bg); border-radius: var(--radius-sm); border: 1px solid var(--border); }
  .row { display: flex; gap: 6px; }
  .empty { padding: 24px 8px; text-align: center; font-size: 14px; }
  .pl { display: grid; grid-template-columns: 1fr auto auto; gap: 4px; align-items: center; padding: 4px; border-radius: 10px; }
  .pl:hover { background: var(--surface-2); }
  .pl-main { display: flex; align-items: center; gap: 12px; background: none; border: none; padding: 6px; text-align: left; min-width: 0; }
  .pl-main:hover { background: none; }
  .pl-icon { width: 40px; height: 40px; border-radius: 8px; background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: var(--accent-ink); display: flex; align-items: center; justify-content: center; flex: none; }
  .pl-meta { display: flex; flex-direction: column; min-width: 0; }
  .t { font-weight: 600; font-size: 14px; }
  .s { font-size: 12px; }

  .detail { display: flex; flex-direction: column; gap: 10px; }
  .detail-head { display: flex; justify-content: space-between; align-items: center; gap: 6px; flex-wrap: wrap; }
  .detail-acts { display: flex; gap: 4px; }
  .pl-title { font-size: 20px; display: flex; gap: 10px; align-items: baseline; }
  .count { font-size: 12px; font-weight: 400; }
  .inline { display: flex; gap: 6px; }
  ol { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
  li { display: grid; grid-template-columns: 22px 48px 1fr auto; gap: 10px; align-items: center; padding: 5px; border-radius: 8px; }
  li:hover { background: var(--surface-2); }
  .n { font-size: 12px; text-align: right; }
  img, .ph { width: 48px; height: 32px; object-fit: cover; border-radius: 5px; background: var(--surface-2); display: flex; align-items: center; justify-content: center; color: var(--muted); }
  .meta { min-width: 0; }
  .acts { display: flex; }
  .acts .icon { width: 30px; height: 30px; }

  .pick { background: var(--bg); border: 1px solid var(--accent); border-radius: var(--radius-sm); padding: 10px; margin-bottom: 10px; display: flex; flex-direction: column; gap: 8px; }
  .pick-head { display: flex; justify-content: space-between; align-items: center; gap: 8px; font-size: 13px; }
  .pick-list { display: flex; flex-wrap: wrap; gap: 6px; }
</style>
