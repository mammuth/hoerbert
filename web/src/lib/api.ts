import type { Track } from './types'

async function req<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api${path}`, {
    method,
    headers: body !== undefined ? { 'content-type': 'application/json' } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
  const text = await res.text()
  let data: any = null
  try { data = text ? JSON.parse(text) : null } catch { data = { error: text } }
  if (!res.ok) throw new Error(data?.error ?? `${res.status} ${res.statusText}`)
  return data as T
}

export const api = {
  search: (q: string, limit = 10) => req<{ results: Track[] }>('GET', `/search?q=${encodeURIComponent(q)}&limit=${limit}`),
  add: (body: { input?: string; track?: Track; tracks?: Track[]; requested_by?: string; next?: boolean; play_now?: boolean }) =>
    req<{ added: Track[]; playlist?: string }>('POST', '/add', body),
  player: (action: 'skip' | 'prev' | 'pause' | 'resume' | 'toggle' | 'stop') => req('POST', `/player/${action}`),
  seek: (position: number) => req('POST', '/player/seek', { position }),
  queueClear: () => req('POST', '/queue/clear'),
  queueRemove: (id: string) => req('POST', `/queue/${id}/remove`),
  queueMove: (id: string, to: number) => req('POST', `/queue/${id}/move`, { to }),
  queuePlay: (id: string) => req('POST', `/queue/${id}/play`),
}
