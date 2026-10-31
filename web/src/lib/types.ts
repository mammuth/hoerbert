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

export interface Segment { start: number; end: number; category: string }

export type LoopMode = 'off' | 'one' | 'all'
export type PlaybackStatus = 'idle' | 'loading' | 'playing' | 'paused'

export interface Playback {
  status: PlaybackStatus
  item?: QueueItem
  position: number
  position_at: number
  volume: number
  loop: LoopMode
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
  sponsorblock: boolean
}
