<script lang="ts">
  let { thumbnail, spinning, size = 220 }: { thumbnail?: string; spinning: boolean; size?: number } = $props()
</script>

<div class="vinyl" style:width="{size}px" style:height="{size}px" class:spinning>
  <div class="disc">
    <div class="grooves"></div>
    <div class="label">
      {#if thumbnail}
        <img src={thumbnail} alt="" loading="lazy" />
      {:else}
        <div class="label-empty"></div>
      {/if}
    </div>
    <div class="hole"></div>
    <div class="shine"></div>
  </div>
</div>

<style>
  .vinyl { position: relative; flex: none; filter: drop-shadow(0 24px 40px rgba(0, 0, 0, 0.6)); }
  .disc {
    position: absolute; inset: 0; border-radius: 50%;
    background:
      repeating-radial-gradient(circle at center, #151311 0 2px, #1d1a16 2px 3px, #121010 3px 5px),
      #141210;
    animation: spin 2.4s linear infinite;
    animation-play-state: paused;
    overflow: hidden;
  }
  .spinning .disc { animation-play-state: running; }
  .grooves {
    position: absolute; inset: 0; border-radius: 50%;
    background: conic-gradient(from 0deg, transparent 0 20%, rgba(255,255,255,0.05) 25%, transparent 30%, transparent 70%, rgba(255,255,255,0.04) 75%, transparent 80%);
  }
  .label {
    position: absolute; inset: 31%; border-radius: 50%; overflow: hidden;
    background: var(--accent); box-shadow: inset 0 0 0 2px rgba(0,0,0,0.3), 0 0 0 1px rgba(255,255,255,0.08);
  }
  .label img { width: 100%; height: 100%; object-fit: cover; display: block; transform: scale(1.3); }
  .label-empty {
    width: 100%; height: 100%;
    background: radial-gradient(circle at 30% 30%, var(--accent-2), var(--accent) 70%);
  }
  .hole { position: absolute; left: 50%; top: 50%; width: 7%; height: 7%; transform: translate(-50%, -50%); border-radius: 50%; background: var(--bg); box-shadow: inset 0 1px 2px rgba(0,0,0,0.9); }
  .shine {
    position: absolute; inset: 0; border-radius: 50%; pointer-events: none;
    background: linear-gradient(135deg, rgba(255,255,255,0.08), transparent 45%, transparent 55%, rgba(255,255,255,0.03));
  }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
