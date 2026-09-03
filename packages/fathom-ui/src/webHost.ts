/** The web host: the app's wasm bindings, driven from a canvas in the viewport. */

import type { FathomHost, WasmApp, WasmLoader } from './host'
import type { AppDescriptor, FrameStats, Rect, SimEvent } from './types'

type WasmModule = Awaited<ReturnType<WasmLoader>>

/**
 * A wasm module can only be started once. React StrictMode mounts every component
 * twice in development, so without this the second start tears down the first one's
 * closures and the app dies with "closure invoked after being dropped".
 */
const started = new Map<WasmLoader, Promise<WasmModule>>()

function loadOnce(loader: WasmLoader): Promise<WasmModule> {
  let pending = started.get(loader)
  if (!pending) {
    pending = loader().then(async (module) => {
      await module.default()
      return module
    })
    started.set(loader, pending)
  }
  return pending
}

export class WebHost implements FathomHost {
  readonly kind = 'web' as const

  private app: WasmApp | null = null
  private canvas: HTMLCanvasElement | null = null
  private lastStats: FrameStats = { frame: 0, fps: 0, frameMs: 0, time: 0 }
  private info = ''

  constructor(private readonly loadWasm: WasmLoader) {}

  async init(container: HTMLElement): Promise<AppDescriptor> {
    const module = await loadOnce(this.loadWasm)

    const canvas = document.createElement('canvas')
    canvas.className = 'fathom-canvas'
    // A size is needed before the surface is configured; the first setViewport
    // corrects it a moment later.
    canvas.width = Math.max(1, Math.round(container.clientWidth * devicePixelRatio))
    canvas.height = Math.max(1, Math.round(container.clientHeight * devicePixelRatio))
    container.appendChild(canvas)
    this.canvas = canvas

    this.app = await module.FathomApp.create(canvas)
    this.info = this.app.adapterInfo()
    return JSON.parse(this.app.descriptor()) as AppDescriptor
  }

  paramByteLength(): number {
    return this.app?.paramByteLength() ?? 16
  }

  setViewport(rect: Rect, dpr: number): void {
    if (!this.app || !this.canvas) return
    const width = Math.max(1, Math.round(rect.width))
    const height = Math.max(1, Math.round(rect.height))
    if (this.canvas.width !== width || this.canvas.height !== height) {
      this.canvas.width = width
      this.canvas.height = height
    }
    this.app.setViewport(width, height, dpr)
  }

  writeParams(bytes: Uint8Array): void {
    this.app?.writeParams(bytes)
  }

  sendEvent(event: SimEvent): void {
    this.app?.input(JSON.stringify(event))
  }

  command(name: string, args: Record<string, unknown> = {}): void {
    this.app?.command(name, JSON.stringify(args))
  }

  frame(nowMs: number): void {
    this.app?.frame(nowMs)
  }

  stats(): FrameStats {
    if (this.app) this.lastStats = JSON.parse(this.app.stats()) as FrameStats
    return this.lastStats
  }

  adapterInfo(): string {
    return this.info
  }

  destroy(): void {
    this.app?.free()
    this.app = null
    this.canvas?.remove()
    this.canvas = null
  }
}
