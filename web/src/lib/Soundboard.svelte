<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from './api'
  import Icon from './Icon.svelte'
  import { store } from './store.svelte'
  import type { Sound, TtsInfo } from './types'

  let sounds = $state<Sound[]>([])
  let tts = $state<TtsInfo | null>(null)
  let hit = $state<string | null>(null)
  let text = $state('')
  let voice = $state<string | undefined>(undefined)
  let saying = $state(false)
  /** Play sounds in this browser instead of TeamSpeak. */
  let local = $state(false)
  try { local = localStorage.getItem('hoerbert.sbLocal') === '1' } catch {}

  function toggleLocal() {
    local = !local
    try { localStorage.setItem('hoerbert.sbLocal', local ? '1' : '0') } catch {}
  }

  /** Ungrouped sounds first, then groups in order of first appearance. */
  const groups = $derived.by(() => {
    const by = new Map<string, Sound[]>([['', []]])
    for (const s of sounds) {
      const g = s.group?.trim() ?? ''
      if (!by.has(g)) by.set(g, [])
      by.get(g)!.push(s)
    }
    return [...by].filter(([, list]) => list.length)
  })

  /** Voice languages besides the default one; none picked: the default voice. */
  const voices = [
    { id: 'de', label: 'Deutsch', emoji: '🇩🇪' },
  ]

  // The sounds file is re-read server-side, so refresh when the tab regains focus.
  const load = () => api.sounds().then((r) => { sounds = r.sounds; tts = r.tts ?? null }).catch(() => {})
  onMount(() => {
    load()
    window.addEventListener('focus', load)
    return () => window.removeEventListener('focus', load)
  })

  async function play(s: Sound) {
    hit = null
    requestAnimationFrame(() => { hit = s.slug })
    if (!local) return void await store.run(api.playSound(s.slug))
    const audio = new Audio(api.soundUrl(s.slug))
    audio.volume = Math.min(1, Math.max(0, s.volume ?? 1))
    await store.run(audio.play().catch(() => { throw new Error(`Cannot play ${s.name} here`) }))
  }

  async function say(e: SubmitEvent) {
    e.preventDefault()
    if (!text.trim() || saying) return
    saying = true
    const ok = await store.runAs((who) => api.say(text, voice, who).then(() => true))
    saying = false
    if (ok) {
      text = ''
      voice = undefined
    }
  }
</script>

{#if sounds.length || tts}
  <section class="card sb rise" style="animation-delay: 180ms">
    <h3 class="side-title">
      <Icon name="volume" size={16} /> Soundboard
      <button class="ghost chip local" class:on={local} onclick={toggleLocal} aria-pressed={local} title={local ? 'Sounds play in this browser only' : 'Sounds play in TeamSpeak'}>
        🎧 browser only
      </button>
    </h3>
    {#if tts}
      <form class="say" onsubmit={say}>
        <div class="row">
          <input type="text" bind:value={text} maxlength={tts.max_chars} placeholder="Make Hörbert say something…" aria-label="Text to say" />
          <button class="primary" disabled={!text.trim() || saying}>🗣️ Say</button>
        </div>
        <div class="voices" role="group" aria-label="Voice">
          {#each voices as v (v.label)}
            <button type="button" aria-pressed={voice === v.id} class="chip" class:active={voice === v.id} onclick={() => { voice = voice === v.id ? undefined : v.id }}>
              {v.emoji} {v.label}
            </button>
          {/each}
        </div>
      </form>
    {/if}
    {#each groups as [group, list] (group)}
      {#if group}<h4 class="group">{group}</h4>{/if}
      <div class="pads">
        {#each list as s (s.slug)}
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
    {/each}
  </section>
{/if}

<style>
  .side-title { display: flex; align-items: center; gap: 8px; font-family: var(--font-display); font-weight: 700; font-size: 15px; padding: 6px 10px 16px; border-bottom: 1px solid var(--border); margin-bottom: 12px; }
  .sb { padding: 14px; flex-shrink: 0; }
  .pads { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; }
  .pads + .pads, .pads + .group { margin-top: 14px; }
  .group { margin: 0; font-size: 10px; font-weight: 600; letter-spacing: 0.12em; text-transform: uppercase; color: var(--muted); padding: 0 2px 6px; }
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
  .say { display: flex; flex-direction: column; gap: 8px; margin-bottom: 12px; }
  .say:last-child { margin-bottom: 0; }
  .row { display: flex; gap: 8px; }
  .row input { min-width: 0; }
  .voices { display: flex; gap: 6px; overflow-x: auto; scrollbar-width: none; }
  .chip { flex-shrink: 0; padding: 5px 9px; font-size: 12px; border-radius: 999px; background: transparent; color: var(--muted); }
  .local { margin-left: auto; font-family: var(--font-body); font-weight: 500; padding: 4px 9px; }
  .local.on { color: var(--accent-ink); background: var(--accent); border-color: var(--accent); }
  .chip.active { color: var(--accent); background: rgba(255, 122, 26, 0.1); }
  @keyframes thump {
    0% { transform: scale(0.9); border-color: var(--accent); box-shadow: 0 0 0 0 rgba(255, 122, 26, 0.55); }
    100% { transform: scale(1); box-shadow: 0 0 0 14px rgba(255, 122, 26, 0); }
  }
</style>
