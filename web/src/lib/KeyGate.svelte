<script lang="ts">
  import { api } from './api'
  import { store } from './store.svelte'

  let key = $state('')
  let error = $state('')
  let busy = $state(false)

  async function submit(e: SubmitEvent) {
    e.preventDefault()
    if (!key.trim() || busy) return
    busy = true
    error = ''
    try {
      await api.login(key.trim())
      store.connect()
    } catch (err: any) {
      error = err?.message ?? String(err)
    } finally {
      busy = false
    }
  }
</script>

<div class="gate">
  <form class="card rise" onsubmit={submit}>
    <span class="logo"><span class="disc"></span></span>
    <h1>Hörbert</h1>
    <p class="muted">Type <span class="mono">!help</span> in TeamSpeak for the link, or enter the key here.</p>
    <!-- svelte-ignore a11y_autofocus -->
    <input type="text" class="mono" bind:value={key} placeholder="key" autocomplete="off" autocapitalize="off" spellcheck="false" autofocus />
    <button class="primary" disabled={!key.trim() || busy}>Unlock</button>
    {#if error}<p class="error">{error}</p>{/if}
  </form>
</div>

<style>
  .gate { min-height: 100vh; display: grid; place-items: center; padding: 16px; }
  form { width: 100%; max-width: 340px; padding: 28px 24px; display: flex; flex-direction: column; align-items: center; gap: 14px; text-align: center; }
  .logo { width: 56px; height: 56px; border-radius: 50%; background: repeating-radial-gradient(circle, #1a1714 0 1.5px, #262119 1.5px 3px); display: inline-flex; align-items: center; justify-content: center; box-shadow: 0 10px 24px -8px rgba(0,0,0,0.8); animation: spin 6s linear infinite; }
  .logo .disc { width: 20px; height: 20px; border-radius: 50%; background: var(--accent); box-shadow: inset 0 0 0 4px var(--bg); }
  @keyframes spin { to { transform: rotate(360deg); } }
  h1 { font-size: 30px; font-weight: 800; letter-spacing: -0.03em; }
  p { margin: 0; font-size: 14px; }
  input { text-align: center; font-size: 18px; letter-spacing: 0.2em; }
  button { width: 100%; justify-content: center; height: 42px; }
  .error { color: var(--danger); }
</style>
