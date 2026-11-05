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
  /** When playback of the item last started (unix ms). */
  played_at?: number
}

export interface Sound {
  slug: string
  name: string
  emoji?: string
}

export type PlayerEvent = { id: string; at: number; by?: string } & (
  | { kind: 'pause' | 'resume' | 'auto_pause' | 'auto_resume' | 'stop' }
  | { kind: 'skip' | 'prev' | 'remove'; title: string }
  | { kind: 'clear'; count: number }
  | { kind: 'sound'; name: string; emoji?: string }
)

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
  users: string[]
  error?: string
}

export interface Snapshot {
  version: number
  ts: TsStatus
  playback: Playback
  queue: QueueItem[]
  history: QueueItem[]
  /** Who did what (pause, skip, soundboard, …), newest first. */
  events: PlayerEvent[]
  /** Server clock (unix ms) when this message was sent. */
  server_now: number
}
