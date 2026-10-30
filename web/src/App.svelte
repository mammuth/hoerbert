<script lang="ts">
  import { onMount } from 'svelte'
  import AddBar from './lib/AddBar.svelte'
  import History from './lib/History.svelte'
  import Icon from './lib/Icon.svelte'
  import NowPlaying from './lib/NowPlaying.svelte'
  import Playlists from './lib/Playlists.svelte'
  import Queue from './lib/Queue.svelte'
  import { api } from './lib/api'
  import { store } from './lib/store.svelte'
  import type { Track } from './lib/types'

  let tab = $state<'playlists' | 'history'>('playlists')
  let pendingTrack = $state<Track | null>(null)
  let editingNick = $state(false)
  let nickDraft = $state('')

  onMount(() => {
    store.connect()
    if (!store.nick) { editingNick = true }
  })

  function addToPlaylist(t: Track) {
    pendingTrack = t
    tab = 'playlists'
    document.getElementById('side')?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }

  async function saveQueue() {
    const name = prompt('Name for the new playlist')
    if (!name) return
    await store.run(api.playlistFromQueue(name), `Saved “${name}”`)
    tab = 'playlists'
  }

  function saveNick() {
    store.setNick(nickDraft)
    editingNick = false
  }

  const ts = $derived(store.snap?.ts)
</script>

<div class="shell">
  <header class="top">
    <div class="brand">
      <span class="logo"><span class="disc"></span></span>
      <h1>Hörbert</h1>
      <span class="tag">shared TeamSpeak jukebox</span>
    </div>
    <div class="status">
      {#if !store.connected}
        <span class="pill off"><span class="dot"></span> reconnecting…</span>
      {:else if ts?.connected}
        <span class="pill on" title={ts.server}>
          <span class="dot"></span>
          {ts.channel ?? 'connected'}
          <span class="sep">·</span>
          {ts.listeners} listening
        </span>
      {:else}
        <span class="pill off" title={ts?.error ?? ''}><span class="dot"></span> TeamSpeak offline</span>
      {/if}
      {#if editingNick}
        <form class="nick" onsubmit={(e) => { e.preventDefault(); saveNick() }}>
          <input type="text" bind:value={nickDraft} placeholder="your name" maxlength="24" />
          <button class="primary" type="submit">ok</button>
        </form>
      {:else}
        <button class="ghost nickbtn" onclick={() => { nickDraft = store.nick; editingNick = true }} title="change your name">
          <Icon name="spark" size={14} /> {store.nick || 'set your name'}
        </button>
      {/if}
    </div>
  </header>

  <main>
    <div class="col main-col">
      <NowPlaying />
      <AddBar onAddToPlaylist={addToPlaylist} />
      <Queue onAddToPlaylist={addToPlaylist} onSaveQueue={saveQueue} />
    </div>
    <aside class="col side card rise" id="side" style="animation-delay: 180ms">
      <nav class="tabs">
        <button class:active={tab === 'playlists'} onclick={() => (tab = 'playlists')}><Icon name="list" size={16} /> Playlists</button>
        <button class:active={tab === 'history'} onclick={() => (tab = 'history')}><Icon name="history" size={16} /> History</button>
      </nav>
      <div class="tab-body">
        {#if tab === 'playlists'}
          <Playlists bind:pendingTrack />
        {:else}
          <History onAddToPlaylist={addToPlaylist} />
        {/if}
      </div>
    </aside>
  </main>

  <footer class="faint">
    chat commands in TeamSpeak: <span class="mono">!play &lt;link or search&gt;</span> · <span class="mono">!skip</span> · <span class="mono">!queue</span> · <span class="mono">!vol 50</span> · <span class="mono">!help</span>
  </footer>
</div>

<div class="toasts">
  {#each store.toasts as t (t.id)}
    <div class="toast rise" class:error={t.kind === 'error'}>{t.text}</div>
  {/each}
</div>

<style>
  .shell { max-width: 1240px; margin: 0 auto; padding: 20px 16px 48px; display: flex; flex-direction: column; gap: 18px; min-height: 100vh; }
  .top { display: flex; justify-content: space-between; align-items: center; gap: 16px; flex-wrap: wrap; }
  .brand { display: flex; align-items: baseline; gap: 12px; }
  .logo { width: 30px; height: 30px; border-radius: 50%; background: repeating-radial-gradient(circle, #1a1714 0 1.5px, #262119 1.5px 3px); display: inline-flex; align-items: center; justify-content: center; align-self: center; box-shadow: 0 6px 16px -6px rgba(0,0,0,0.8); animation: spin 6s linear infinite; }
  .logo .disc { width: 11px; height: 11px; border-radius: 50%; background: var(--accent); box-shadow: inset 0 0 0 2.5px var(--bg); }
  @keyframes spin { to { transform: rotate(360deg); } }
  h1 { font-size: 30px; font-weight: 800; letter-spacing: -0.03em; }
  .tag { color: var(--faint); font-size: 13px; }
  .status { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .pill { display: inline-flex; align-items: center; gap: 8px; padding: 6px 12px; border-radius: 999px; font-size: 13px; border: 1px solid var(--border-strong); background: var(--surface); }
  .pill .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--faint); }
  .pill.on .dot { background: var(--ok); box-shadow: 0 0 10px var(--ok); }
  .pill.off { color: var(--muted); }
  .pill.off .dot { background: var(--danger); }
  .sep { color: var(--faint); }
  .nick { display: flex; gap: 6px; }
  .nick input { width: 150px; padding: 6px 10px; }
  .nickbtn { font-size: 13px; }

  main { display: grid; grid-template-columns: minmax(0, 1.6fr) minmax(300px, 1fr); gap: 18px; align-items: start; }
  .col { display: flex; flex-direction: column; gap: 18px; min-width: 0; }
  .side { padding: 14px; position: sticky; top: 16px; max-height: calc(100vh - 32px); overflow: auto; }
  .tabs { display: flex; gap: 4px; border-bottom: 1px solid var(--border); padding-bottom: 10px; margin-bottom: 12px; }
  .tabs button { background: none; border-color: transparent; color: var(--muted); font-family: var(--font-display); font-weight: 700; font-size: 15px; padding: 6px 10px; }
  .tabs button.active { color: var(--text); background: var(--surface-2); }
  footer { font-size: 12px; text-align: center; margin-top: auto; }

  .toasts { position: fixed; bottom: 18px; left: 50%; transform: translateX(-50%); display: flex; flex-direction: column; gap: 6px; z-index: 50; pointer-events: none; max-width: min(92vw, 520px); }
  .toast { background: var(--text); color: var(--bg); padding: 10px 14px; border-radius: 10px; font-size: 14px; box-shadow: 0 10px 30px rgba(0,0,0,0.5); font-weight: 500; }
  .toast.error { background: var(--danger); color: #fff; }

  @media (max-width: 960px) {
    main { grid-template-columns: 1fr; }
    .side { position: static; max-height: none; }
  }
  @media (max-width: 560px) {
    h1 { font-size: 24px; }
    .tag { display: none; }
  }
</style>
