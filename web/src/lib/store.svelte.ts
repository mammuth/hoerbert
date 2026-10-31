import type { Snapshot } from './types'

class Store {
  snap = $state<Snapshot | null>(null)
  connected = $state(false)
  now = $state(Date.now())
  nick = $state<string>('')
  toasts = $state<{ id: number; text: string; kind: 'info' | 'error' }[]>([])
  private ws: WebSocket | null = null
  private retry = 1000
  private toastId = 0

  constructor() {
    try { this.nick = localStorage.getItem('hoerbert.nick') ?? '' } catch {}
    setInterval(() => { this.now = Date.now() }, 250)
  }

  setNick(n: string) {
    this.nick = n.trim()
    try { localStorage.setItem('hoerbert.nick', this.nick) } catch {}
  }

  /** Live playback position, extrapolated from the last snapshot. */
  get position(): number {
    const p = this.snap?.playback
    if (!p) return 0
    if (p.status !== 'playing') return p.position
    const elapsed = (this.now - p.position_at) / 1000
    const pos = p.position + Math.max(0, elapsed)
    const dur = p.item?.track.duration
    return dur ? Math.min(pos, dur) : pos
  }

  connect() {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    const ws = new WebSocket(`${proto}://${location.host}/api/ws`)
    this.ws = ws
    ws.onopen = () => { this.connected = true; this.retry = 1000 }
    ws.onmessage = (ev) => {
      try { this.snap = JSON.parse(ev.data) as Snapshot } catch {}
    }
    ws.onclose = () => {
      this.connected = false
      setTimeout(() => this.connect(), this.retry)
      this.retry = Math.min(this.retry * 1.7, 10000)
    }
    ws.onerror = () => ws.close()
  }

  toast(text: string, kind: 'info' | 'error' = 'info') {
    const id = ++this.toastId
    this.toasts = [...this.toasts, { id, text, kind }]
    setTimeout(() => { this.toasts = this.toasts.filter((t) => t.id !== id) }, kind === 'error' ? 6000 : 3000)
  }

  async run<T>(p: Promise<T>, okText?: string): Promise<T | undefined> {
    try {
      const r = await p
      if (okText) this.toast(okText)
      return r
    } catch (e: any) {
      this.toast(e?.message ?? String(e), 'error')
      return undefined
    }
  }

  /** run() for web adds: a name is required, otherwise point the user at the name field. */
  async runAs<T>(fn: (who: string) => Promise<T>, okText?: string): Promise<T | undefined> {
    if (!this.nick) {
      this.toast('Enter your name first', 'error')
      document.getElementById('nick')?.focus()
      return undefined
    }
    return this.run(fn(this.nick), okText)
  }
}

export const store = new Store()
