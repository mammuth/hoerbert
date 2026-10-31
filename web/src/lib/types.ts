export interface Track {
  url: string
  title: string
  artist?: string
  duration?: number
  thumbnail?: string
  source?: string
  source_id?: string
  is_live?: boolean
}

export interface QueueItem {
  id: string
  track: Track
  requested_by?: string
  added_at: number
  request?: boolean
}

export interface Sound {
  slug: string
  name: string
  emoji?: string
}

export interface Segment { start: number; end: number; category: string }

export type PlaybackStatus = 'idle' | 'loading' | 'playing' | 'paused'

export interface Playback {
  status: PlaybackStatus
  item?: QueueItem
  position: number
  position_at: number
  segments: Segment[]
  error?: string
}

export interface TsStatus {
  connected: boolean
  server: string
  channel?: string
  listeners: number
  error?: string
}

export interface Snapshot {
  version: number
  ts: TsStatus
  playback: Playback
  queue: QueueItem[]
  history: QueueItem[]
  /** Server clock (unix ms) when this message was sent. */
  server_now: number
}
