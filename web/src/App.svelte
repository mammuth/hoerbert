<script lang="ts">
  import { onMount } from 'svelte'
  import AddBar from './lib/AddBar.svelte'
  import History from './lib/History.svelte'
  import Icon from './lib/Icon.svelte'
  import NowPlaying from './lib/NowPlaying.svelte'
  import Queue from './lib/Queue.svelte'
  import Soundboard from './lib/Soundboard.svelte'
  import { store } from './lib/store.svelte'

  onMount(() => {
    store.connect()
  })

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
    </div>
  </header>

  <main>
    <div class="col main-col">
      <NowPlaying />
      <AddBar />
      <Queue />
    </div>
    <aside class="col side card rise" id="side" style="animation-delay: 180ms">
      <Soundboard />
      <h3 class="side-title"><Icon name="history" size={16} /> History</h3>
      <History />
    </aside>
  </main>

  <footer class="faint">
    chat commands in TeamSpeak: <span class="mono">!play &lt;link or search&gt;</span> · <span class="mono">!skip</span> · <span class="mono">!pause</span> · <span class="mono">!queue</span> · <span class="mono">!help</span>
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

  main { display: grid; grid-template-columns: minmax(0, 1.6fr) minmax(300px, 1fr); gap: 18px; align-items: start; }
  .col { display: flex; flex-direction: column; gap: 18px; min-width: 0; }
  .side { padding: 14px; position: sticky; top: 16px; max-height: calc(100vh - 32px); overflow: auto; }
  .side-title { display: flex; align-items: center; gap: 8px; font-family: var(--font-display); font-weight: 700; font-size: 15px; padding: 6px 10px 16px; border-bottom: 1px solid var(--border); margin-bottom: 12px; }
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
