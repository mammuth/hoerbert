<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from './api'
  import Icon from './Icon.svelte'
  import { store } from './store.svelte'
  import type { Sound } from './types'

  let sounds = $state<Sound[]>([])
  let hit = $state<string | null>(null)

  // The sounds file is re-read server-side, so refresh when the tab regains focus.
  const load = () => api.sounds().then((r) => { sounds = r.sounds }).catch(() => {})
  onMount(() => {
    load()
    window.addEventListener('focus', load)
    return () => window.removeEventListener('focus', load)
  })

  async function play(s: Sound) {
    hit = null
    requestAnimationFrame(() => { hit = s.slug })
    await store.run(api.playSound(s.slug))
  }
</script>

{#if sounds.length}
  <section class="card sb rise" style="animation-delay: 180ms">
    <h3 class="side-title"><Icon name="volume" size={16} /> Soundboard</h3>
    <div class="pads">
      {#each sounds as s (s.slug)}
        <button
          class="pad"
          class:hit={hit === s.slug}
          title="!{s.slug}"
          onclick={() => play(s)}
          onanimationend={() => { hit = null }}
        >
          <span class="emoji">{s.emoji ?? '🔈'}</span>
          <span class="name ellipsis">{s.name}</span>
        </button>
      {/each}
    </div>
  </section>
{/if}

<style>
  .side-title { display: flex; align-items: center; gap: 8px; font-family: var(--font-display); font-weight: 700; font-size: 15px; padding: 6px 10px 16px; border-bottom: 1px solid var(--border); margin-bottom: 12px; }
  .sb { padding: 14px; flex-shrink: 0; }
  .pads { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; }
  .pad {
    flex-direction: column; justify-content: center; gap: 6px; padding: 12px 6px 10px; min-width: 0;
    border-radius: var(--radius-sm);
    background: linear-gradient(180deg, var(--surface-2), var(--surface));
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.04), 0 4px 0 -1px var(--bg);
  }
  .pad:hover { border-color: var(--accent); }
  .pad:hover .emoji { transform: scale(1.12) rotate(-6deg); }
  .pad.hit { animation: thump 0.45s cubic-bezier(.2, .9, .3, 1.4); }
  .emoji { font-size: 26px; line-height: 1; transition: transform 0.15s; }
  .name { font-size: 12px; color: var(--muted); max-width: 100%; }
  @keyframes thump {
    0% { transform: scale(0.9); border-color: var(--accent); box-shadow: 0 0 0 0 rgba(255, 122, 26, 0.55); }
    100% { transform: scale(1); box-shadow: 0 0 0 14px rgba(255, 122, 26, 0); }
  }
</style>
