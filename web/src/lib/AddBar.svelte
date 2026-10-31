<script lang="ts">
  import { api } from './api'
  import { fmtTime, sourceLabel } from './format'
  import Icon from './Icon.svelte'
  import { store } from './store.svelte'
  import type { Track } from './types'

  let nick = $state(store.nick)
  let query = $state('')
  let results = $state<Track[]>([])
  let busy = $state(false)
  let open = $state(false)
  let inputEl: HTMLInputElement | undefined = $state()
  let timer: ReturnType<typeof setTimeout> | undefined

  const isUrl = $derived(/^https?:\/\/\S+$/i.test(query.trim()))

  function onInput() {
    clearTimeout(timer)
    const q = query.trim()
    if (!q || isUrl) { results = []; return }
    timer = setTimeout(search, 450)
  }

  async function search() {
    const q = query.trim()
    if (!q) return
    busy = true
    const r = await store.run(api.search(q, 8))
    busy = false
    if (r) { results = r.results; open = true }
  }

  async function submit(opts: { next?: boolean; play_now?: boolean } = {}) {
    const q = query.trim()
    if (!q) return
    busy = true
    const r = await store.runAs((who) => api.add({ input: q, requested_by: who, ...opts }))
    busy = false
    if (r) {
      store.toast(r.added.length === 1 ? `Queued: ${r.added[0].title}` : `Queued ${r.added.length} tracks${r.playlist ? ` from “${r.playlist}”` : ''}`)
      query = ''; results = []; open = false
    }
  }

  async function addTrack(t: Track, opts: { next?: boolean; play_now?: boolean } = {}) {
    const r = await store.runAs((who) => api.add({ track: t, requested_by: who, ...opts }))
    if (r) store.toast(`Queued: ${t.title}`)
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter') { e.preventDefault(); submit({ next: e.shiftKey }) }
    if (e.key === 'Escape') { open = false }
  }
</script>

<section class="add card rise" style="animation-delay: 60ms">
  <div class="row">
    <div class="field name" class:missing={!store.nick}>
      <span class="lead"><Icon name="spark" /></span>
      <input
        id="nick"
        bind:value={nick}
        oninput={() => store.setNick(nick)}
        type="text"
        placeholder="Your name"
        maxlength="24"
        autocomplete="nickname"
        spellcheck="false"
        required
      />
    </div>
    <div class="field">
      <span class="lead"><Icon name={isUrl ? 'link' : 'search'} /></span>
      <input
        bind:this={inputEl}
        bind:value={query}
        type="text"
        placeholder="Paste a YouTube / SoundCloud / playlist link or search…"
        oninput={onInput}
        onkeydown={onKey}
        onfocus={() => { if (results.length) open = true }}
        autocomplete="off"
        spellcheck="false"
      />
      {#if query}
        <button class="ghost icon clear" title="clear" onclick={() => { query = ''; results = []; open = false; inputEl?.focus() }}><Icon name="x" size={14} /></button>
      {/if}
    </div>
    <button class="primary" disabled={!query.trim() || !store.nick || busy} onclick={() => submit()}>
      <Icon name="plus" /> Add
    </button>
    <button disabled={!query.trim() || !store.nick || busy} title="play right after the current track" onclick={() => submit({ next: true })}>
      <Icon name="next" /> Next
    </button>
  </div>
  <div class="hint faint">
    {#if store.nick}Enter adds · Shift+Enter plays next · tracks are requested as <strong>{store.nick}</strong>
    {:else}<span class="need">Enter your name to add songs</span>{/if}
  </div>

  {#if open && results.length}
    <ul class="results">
      {#each results as t, i (t.url)}
        <li class="rise" style="animation-delay: {i * 30}ms">
          <img src={t.thumbnail} alt="" loading="lazy" />
          <div class="meta">
            <div class="t ellipsis" title={t.title}>{t.title}</div>
            <div class="s muted ellipsis">{t.artist ?? ''} · {fmtTime(t.duration)} · {sourceLabel(t.source)}</div>
          </div>
          <div class="acts">
            <button class="icon" title="queue" onclick={() => addTrack(t)}><Icon name="plus" /></button>
            <button class="icon ghost" title="play next" onclick={() => addTrack(t, { next: true })}><Icon name="next" /></button>
          </div>
        </li>
      {/each}
      <li class="close"><button class="ghost" onclick={() => (open = false)}>close results</button></li>
    </ul>
  {/if}
</section>

<style>
  .add { padding: 16px; display: flex; flex-direction: column; gap: 8px; }
  .row { display: flex; gap: 8px; align-items: stretch; }
  .field { position: relative; flex: 1; display: flex; align-items: center; }
  .field input { padding-left: 38px; padding-right: 36px; height: 42px; }
  .lead { position: absolute; left: 12px; color: var(--faint); display: flex; }
  .name { flex: 0 0 160px; }
  .name input { padding-right: 12px; }
  .name.missing input { border-color: var(--accent); }
  .name.missing .lead, .need { color: var(--accent); }
  .clear { position: absolute; right: 4px; width: 30px; height: 30px; }
  .hint { font-size: 12px; }
  .hint strong { color: var(--muted); font-weight: 500; }
  .results { list-style: none; margin: 4px 0 0; padding: 6px; display: flex; flex-direction: column; gap: 2px; background: var(--bg); border: 1px solid var(--border); border-radius: var(--radius-sm); max-height: 420px; overflow: auto; }
  .results li { display: grid; grid-template-columns: 64px 1fr auto; gap: 12px; align-items: center; padding: 6px; border-radius: 8px; }
  .results li:hover { background: var(--surface); }
  .results img { width: 64px; height: 40px; object-fit: cover; border-radius: 6px; background: var(--surface-2); }
  .meta { min-width: 0; }
  .t { font-weight: 500; }
  .s { font-size: 12px; }
  .acts { display: flex; gap: 2px; }
  .close { grid-template-columns: 1fr !important; justify-items: center; }
  @media (max-width: 560px) {
    .row { flex-wrap: wrap; }
    .field, .name { flex-basis: 100%; }
    .row > button { flex: 1; justify-content: center; }
    .acts .ghost { display: none; }
  }
</style>
