<script lang="ts">
  let { name, size = 18 }: { name: string; size?: number } = $props()
  const paths: Record<string, string> = {
    play: 'M8 5v14l11-7z',
    pause: 'M6 5h4v14H6zM14 5h4v14h-4z',
    skip: 'M6 6l8.5 6L6 18V6zM16 6h2v12h-2z',
    prev: 'M18 6l-8.5 6 8.5 6V6zM6 6h2v12H6z',
    stop: 'M6 6h12v12H6z',
    shuffle: 'M16 3h5v5l-1.8-1.8L15 10.4 13.6 9l4.2-4.2L16 3zM3 6h4.5l2.7 3-1.4 1.4L6.6 8H3V6zm14.2 10.2L21 20v-5h-5l1.8 1.8L15 14.6 13.6 16l3.6 0.2zM3 16h3.6l9.6-9.6L17.6 8 7.4 18H3v-2z',
    repeat: 'M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4z',
    repeatOne: 'M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4zm-4-2V9h-1l-2 1v1h1.5v4H13z',
    trash: 'M6 7h12l-1 14H7L6 7zm3-4h6l1 2h4v2H4V5h4l1-2z',
    plus: 'M11 5h2v6h6v2h-6v6h-2v-6H5v-2h6z',
    up: 'M12 6l7 7-1.4 1.4L12 8.8l-5.6 5.6L5 13z',
    down: 'M12 18l-7-7 1.4-1.4L12 15.2l5.6-5.6L19 11z',
    volume: 'M4 9v6h4l5 4V5L8 9H4zm11.5 3A4.5 4.5 0 0 0 14 8.2v7.6a4.5 4.5 0 0 0 1.5-3.8z',
    mute: 'M4 9v6h4l5 4V5L8 9H4zm12.6 3l2.9-2.9-1.4-1.4L15.2 10.6 12.3 7.7l-1.4 1.4L13.8 12l-2.9 2.9 1.4 1.4 2.9-2.9 2.9 2.9 1.4-1.4L16.6 12z',
    search: 'M10 4a6 6 0 1 1 0 12 6 6 0 0 1 0-12zm0-2a8 8 0 1 0 4.9 14.3l4.4 4.4 1.4-1.4-4.4-4.4A8 8 0 0 0 10 2z',
    link: 'M10.6 13.4a1 1 0 0 0 1.4 0l3-3a3 3 0 0 0-4.2-4.2l-1 1 1.4 1.4 1-1a1 1 0 1 1 1.4 1.4l-3 3a1 1 0 0 0 0 1.4zm2.8-2.8a1 1 0 0 0-1.4 0l-3 3a3 3 0 0 0 4.2 4.2l1-1-1.4-1.4-1 1a1 1 0 0 1-1.4-1.4l3-3a1 1 0 0 0 0-1.4z',
    list: 'M4 6h16v2H4zm0 5h16v2H4zm0 5h16v2H4z',
    history: 'M13 3a9 9 0 0 0-9 9H1l3.9 3.9L9 12H6a7 7 0 1 1 7 7v2a9 9 0 0 0 0-18zm-1 5v5l4.3 2.5.7-1.2-3.5-2V8H12z',
    heart: 'M12 21s-7.5-4.6-9.5-9.3C1 8 3.5 4.5 7 4.5c2 0 3.5 1 5 2.8 1.5-1.8 3-2.8 5-2.8 3.5 0 6 3.5 4.5 7.2C19.5 16.4 12 21 12 21z',
    x: 'M6.4 5l12.6 12.6L17.6 19 5 6.4z M17.6 5L19 6.4 6.4 19 5 17.6z',
    next: 'M4 6h2v12H4zm4 6l8-6v12z M18 6h2v12h-2z',
    save: 'M5 3h11l3 3v15H5V3zm2 2v4h8V5H7zm1 8v5h8v-5H8z',
    edit: 'M3 17.3V21h3.7L17.8 9.9l-3.7-3.7L3 17.3zM20.7 7a1 1 0 0 0 0-1.4l-2.3-2.3a1 1 0 0 0-1.4 0l-1.8 1.8 3.7 3.7L20.7 7z',
    music: 'M12 3v10.6A4 4 0 1 0 14 17V7h5V3h-7z',
    spark: 'M12 2l2.2 6.3L21 10l-6.8 1.7L12 18l-2.2-6.3L3 10l6.8-1.7z',
    shield: 'M12 2l8 3v6c0 5-3.4 9.4-8 11-4.6-1.6-8-6-8-11V5l8-3zm-1 13l6-6-1.4-1.4L11 12.2 8.4 9.6 7 11l4 4z',
  }
</script>

<svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
  <path d={paths[name] ?? ''} />
</svg>
