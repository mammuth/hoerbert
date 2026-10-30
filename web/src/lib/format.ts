export function fmtTime(secs: number | undefined | null): string {
  if (secs == null || !isFinite(secs)) return '–:––'
  const s = Math.max(0, Math.round(secs))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const r = s % 60
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${String(r).padStart(2, '0')}` : `${m}:${String(r).padStart(2, '0')}`
}

export function fmtTotal(secs: number): string {
  const m = Math.round(secs / 60)
  if (m < 60) return `${m} min`
  const h = Math.floor(m / 60)
  return `${h} h ${m % 60} min`
}

export function sourceLabel(source?: string): string {
  if (!source) return ''
  if (source.startsWith('youtube')) return 'YouTube'
  if (source === 'direct') return 'Direct'
  return source.charAt(0).toUpperCase() + source.slice(1)
}
