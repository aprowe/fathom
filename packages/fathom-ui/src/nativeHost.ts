/**
 * The native host: the same app, running on a render thread behind a transparent webview.
 *
 * Nothing here draws. The viewport rect is forwarded to a wgpu child window that the
 * Tauri window owns, so it moves and clips with the layout on its own. Input still
 * arrives through the DOM, because the webview is the topmost layer — which is why this
 * file has no input handling of its own beyond forwarding.
 */

import type { FathomHost } from './host'
import type { AppDescriptor, FrameStats, Rect, SimEvent } from './types'

type Invoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>

interface InitReply {
  descriptor: string
  paramByteLength: number
  adapterInfo: string
}

/** How often the toolbar's frame counters are refreshed. Faster would just be noise. */
const STATS_INTERVAL_MS = 250

export class NativeHost implements FathomHost {
  readonly kind = 'native' as const

  private invoke: Invoke
  private lastStats: FrameStats = { frame: 0, fps: 0, frameMs: 0, time: 0 }
  private info = ''
  private byteLength = 16
  private statsTimer: number | null = null
  private lastRect = ''

  constructor() {
    const internals = (window as unknown as { __TAURI_INTERNALS__?: { invoke: Invoke } })
      .__TAURI_INTERNALS__
    if (!internals) throw new Error('NativeHost was constructed outside the Tauri shell')
    this.invoke = internals.invoke
  }

  async init(_container: HTMLElement): Promise<AppDescriptor> {
    // Everything the panel does not cover has to be genuinely transparent, or the child
    // window behind the webview is never seen. The stylesheet keys off this attribute.
    document.documentElement.setAttribute('data-fathom-host', 'native')

    const reply = (await this.invoke('fathom_init')) as InitReply
    this.byteLength = reply.paramByteLength
    this.info = reply.adapterInfo

    this.statsTimer = window.setInterval(() => {
      void this.invoke('fathom_stats').then((s) => {
        this.lastStats = s as FrameStats
      })
    }, STATS_INTERVAL_MS)

    return JSON.parse(reply.descriptor) as AppDescriptor
  }

  paramByteLength(): number {
    return this.byteLength
  }

  setViewport(rect: Rect, dpr: number): void {
    // Moving a native window is not free, so only send a rect that actually changed.
    const key = `${rect.x}|${rect.y}|${rect.width}|${rect.height}`
    if (key === this.lastRect) return
    this.lastRect = key
    void this.invoke('fathom_set_viewport', {
      rect: {
        x: Math.round(rect.x),
        y: Math.round(rect.y),
        width: Math.max(1, Math.round(rect.width)),
        height: Math.max(1, Math.round(rect.height)),
        dpr,
      },
    })
  }

  writeParams(bytes: Uint8Array): void {
    // The block is a few dozen bytes; one small message a frame is cheaper than any
    // shared-memory scheme would be to maintain.
    void this.invoke('fathom_write_params', { bytes: Array.from(bytes) })
  }

  sendEvent(event: SimEvent): void {
    void this.invoke('fathom_input', { json: JSON.stringify(event) })
  }

  command(name: string, args: Record<string, unknown> = {}): void {
    void this.invoke('fathom_command', { name, args: JSON.stringify(args) })
  }

  frame(): void {
    // The native render thread runs its own loop.
  }

  stats(): FrameStats {
    return this.lastStats
  }

  adapterInfo(): string {
    return this.info
  }

  /**
   * Let go of this interface instance only.
   *
   * The renderer is a process-level resource on native: the webview reloads on every
   * hot update, and React mounts every component twice in development. Tearing the
   * render thread down here once left a live-looking app whose simulation had stopped,
   * still reporting the GPU it no longer had. So this releases the poll timer and
   * nothing else.
   */
  destroy(): void {
    if (this.statsTimer !== null) window.clearInterval(this.statsTimer)
    this.statsTimer = null
  }
}
