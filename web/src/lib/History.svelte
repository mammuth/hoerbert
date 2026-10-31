<script lang="ts">
  import { api } from './api'
  import { fmtTime } from './format'
  import Icon from './Icon.svelte'
  import { store } from './store.svelte'
  const history = $derived(store.snap?.history ?? [])
</script>

{#if !history.length}
  <div class="empty faint">Nothing played yet.</div>
{:else}
  <ul>
    {#each history as item, i (item.id)}
      <li class="rise" style="animation-delay: {Math.min(i, 12) * 20}ms">
        {#if item.track.thumbnail}<img src={item.track.thumbnail} alt="" loading="lazy" />{:else}<span class="ph"><Icon name="music" /></span>{/if}
        <div class="meta">
          <div class="t ellipsis" title={item.track.title}>{item.track.title}</div>
          <div class="s muted ellipsis">{item.track.artist ?? ''} · {fmtTime(item.track.duration)}{item.requested_by ? ` · ${item.requested_by}` : ''}</div>
        </div>
        <div class="acts">
          <button class="icon ghost" title="queue again" onclick={() => store.runAs((who) => api.add({ track: item.track, requested_by: who }), 'Queued again')}><Icon name="plus" size={16} /></button>
        </div>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .empty { padding: 24px 8px; text-align: center; font-size: 14px; }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
  li { display: grid; grid-template-columns: 48px 1fr auto; gap: 10px; align-items: center; padding: 6px; border-radius: 10px; }
  li:hover { background: var(--surface-2); }
  img, .ph { width: 48px; height: 32px; object-fit: cover; border-radius: 5px; background: var(--surface-2); display: flex; align-items: center; justify-content: center; color: var(--muted); }
  .meta { min-width: 0; }
  .t { font-size: 14px; font-weight: 500; }
  .s { font-size: 12px; }
  .acts { display: flex; }
  .acts .icon { width: 30px; height: 30px; }
</style>
