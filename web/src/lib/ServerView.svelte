<script lang="ts">
  import Icon from './Icon.svelte'
  import { store } from './store.svelte'
  import type { TsChannel } from './types'

  let hideEmpty = $state(false)
  try { hideEmpty = localStorage.getItem('hoerbert.hideEmpty') === '1' } catch {}

  function toggleEmpty() {
    hideEmpty = !hideEmpty
    try { localStorage.setItem('hoerbert.hideEmpty', hideEmpty ? '1' : '0') } catch {}
  }

  const ts = $derived(store.snap?.ts)
  const channels = $derived(ts?.connected ? ts.channels : [])
  const online = $derived(channels.reduce((n, c) => n + c.clients.filter((u) => !u.me).length, 0))

  /** TeamSpeak spacers: top level channels named `[spacer]…`, `[cspacer]…`, `[rspacer]…` or `[*spacer]…`. */
  const SPACER = /^\[([lrc*]?)spacer[^\]]*\](.*)$/i
  type Spacer = { align: 'left' | 'center' | 'right'; text: string; rule: boolean }

  function spacer(ch: TsChannel): Spacer | null {
    const m = ch.depth === 0 ? SPACER.exec(ch.name) : null
    if (!m) return null
    const kind = m[1].toLowerCase()
    const text = m[2].trim()
    // `[*spacer]-` repeats its text into a line; so does plain `---`, `___`, `===`…
    const rule = kind === '*' || !text || /^(.)\1*$/u.test(text)
    return { align: kind === 'c' ? 'center' : kind === 'r' ? 'right' : 'left', text, rule }
  }

  /** Hiding empty channels keeps the parents of occupied ones, so the tree still reads. */
  const visible = $derived.by(() => {
    if (!hideEmpty) return channels
    const keep = new Set<number>()
    const path: TsChannel[] = []
    for (const ch of channels) {
      path.length = ch.depth
      path.push(ch)
      if (ch.clients.length) for (const p of path) keep.add(p.id)
    }
    return channels.filter((c) => keep.has(c.id))
  })
</script>

{#if channels.length}
  <section class="card server rise" style="animation-delay: 320ms">
    <header>
      <h3><Icon name="list" size={16} /> Server</h3>
      <span class="name muted ellipsis">{ts?.server}</span>
      <span class="count mono">{online} online</span>
      <button class="ghost chip" class:on={hideEmpty} onclick={toggleEmpty} aria-pressed={hideEmpty}>hide empty</button>
    </header>

    <div class="tree">
      {#each visible as ch, i (ch.id)}
        {@const sp = spacer(ch)}
        {@const here = ch.clients.some((c) => c.me)}
        {#if sp}
          <div class="spacer {sp.align}" class:rule={sp.rule}>
            {#if !sp.rule}<span>{sp.text}</span>{/if}
          </div>
        {:else}
          <div class="chan rise" class:here class:empty={!ch.clients.length} style="--depth: {ch.depth}; animation-delay: {360 + Math.min(i, 24) * 18}ms">
            <div class="cname">
              <span class="glyph" aria-hidden="true"></span>
              <span class="ellipsis" title={ch.name}>{ch.name}</span>
              {#if ch.clients.length}<span class="n mono">{ch.clients.length}</span>{/if}
            </div>
            {#if ch.clients.length}
              <ul>
                {#each ch.clients as c (c.name)}
                  <li class:me={c.me} class:away={c.away}>
                    <span class="avatar" aria-hidden="true">{c.name.slice(0, 1).toUpperCase()}</span>
                    <span class="ellipsis" title={c.away ? `${c.name} (away)` : c.name}>{c.name}</span>
                    <span class="flags">
                      {#if c.muted}<span title="microphone muted"><Icon name="micOff" size={13} /></span>{/if}
                      {#if c.deaf}<span title="sound muted"><Icon name="headsetOff" size={13} /></span>{/if}
                    </span>
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
        {/if}
      {/each}
    </div>
  </section>
{/if}

<style>
  .server { padding: 14px 18px 18px; }
  header { display: flex; align-items: center; gap: 12px; padding: 6px 0 14px; border-bottom: 1px solid var(--border); margin-bottom: 14px; min-width: 0; }
  h3 { display: flex; align-items: center; gap: 8px; font-weight: 700; font-size: 15px; flex: none; }
  .name { font-size: 13px; min-width: 0; }
  .count { margin-left: auto; flex: none; font-size: 12px; color: var(--ok); }
  .chip { flex: none; padding: 5px 10px; font-size: 12px; border-radius: 999px; color: var(--muted); }
  .chip.on { color: var(--accent-ink); background: var(--accent); border-color: var(--accent); }

  .chan { padding-left: calc(var(--depth) * 16px); margin-bottom: 4px; }
  .cname { display: flex; align-items: center; gap: 8px; padding: 4px 8px; border-radius: 8px; font-size: 13.5px; font-weight: 600; }
  .glyph { flex: none; width: 8px; height: 8px; border-radius: 2px; border: 1.5px solid var(--faint); transform: rotate(45deg); }
  .n { margin-left: auto; font-size: 11px; color: var(--faint); }
  .chan.empty .cname { font-weight: 400; color: var(--faint); }
  .chan.here .cname { background: linear-gradient(90deg, rgba(255, 122, 26, 0.16), transparent 85%); box-shadow: inset 2px 0 0 var(--accent); color: var(--text); }
  .chan.here .glyph { border-color: var(--accent); background: var(--accent); }
  .chan:not(.empty):not(.here) .glyph { border-color: var(--ok); }

  ul { list-style: none; margin: 2px 0 6px; padding: 0 0 0 14px; border-left: 1px dashed var(--border-strong); margin-left: 12px; }
  li { display: flex; align-items: center; gap: 8px; padding: 3px 6px; font-size: 13px; border-radius: 6px; }
  li:hover { background: var(--surface-2); }
  .avatar { flex: none; width: 18px; height: 18px; border-radius: 50%; display: inline-flex; align-items: center; justify-content: center; font-family: var(--font-display); font-size: 10px; font-weight: 700; background: var(--surface-2); border: 1px solid var(--border-strong); color: var(--muted); }
  li.me .avatar { background: var(--accent); border-color: var(--accent); color: var(--accent-ink); }
  li.me { color: var(--accent-2); font-weight: 600; }
  li.away { color: var(--faint); font-style: italic; }
  .flags { margin-left: auto; display: flex; gap: 4px; color: var(--danger); opacity: 0.8; }
  .flags span { display: flex; }

  .spacer { display: flex; align-items: center; min-height: 20px; margin: 6px 0; padding: 0 8px; font-family: var(--font-mono); font-size: 10.5px; letter-spacing: 0.12em; text-transform: uppercase; color: var(--faint); }
  .spacer.center { justify-content: center; }
  .spacer.right { justify-content: flex-end; }
  .spacer.rule::before { content: ''; flex: 1; border-top: 1px solid var(--border); }

  @media (max-width: 560px) {
    .name { display: none; }
  }
</style>
