import type { Playlist, Track } from './types'

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
  volume: (volume: number) => req('POST', '/player/volume', { volume }),
  loop: (mode: string) => req('POST', '/player/loop', { mode }),
  sponsorblock: (enabled: boolean) => req('POST', '/player/sponsorblock', { enabled }),
  queueClear: () => req('POST', '/queue/clear'),
  queueShuffle: () => req('POST', '/queue/shuffle'),
  queueRemove: (id: string) => req('POST', `/queue/${id}/remove`),
  queueMove: (id: string, to: number) => req('POST', `/queue/${id}/move`, { to }),
  queuePlay: (id: string) => req('POST', `/queue/${id}/play`),
  playlist: (id: string) => req<Playlist>('GET', `/playlists/${id}`),
  playlistCreate: (body: { name: string; tracks?: Track[]; import?: string }) => req<Playlist>('POST', '/playlists', body),
  playlistFromQueue: (name: string) => req<Playlist>('POST', '/playlists/from-queue', { name }),
  playlistDelete: (id: string) => req('DELETE', `/playlists/${id}`),
  playlistRename: (id: string, name: string) => req('PATCH', `/playlists/${id}`, { name }),
  playlistAddTracks: (id: string, body: { track?: Track; tracks?: Track[]; input?: string }) => req<{ added: number }>('POST', `/playlists/${id}/tracks`, body),
  playlistRemoveTrack: (id: string, index: number) => req('DELETE', `/playlists/${id}/tracks/${index}`),
  playlistEnqueue: (id: string, shuffle: boolean, requested_by?: string) => req<{ queued: number }>('POST', `/playlists/${id}/enqueue`, { shuffle, requested_by }),
  playlistImport: (id: string, url: string) => req<{ added: number }>('POST', `/playlists/${id}/import`, { url }),
}
