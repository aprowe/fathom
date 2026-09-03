/**
 * The seam between the interface and whichever host is running the simulation.
 *
 * There are two implementations — wasm on web, Tauri IPC on native — and panel code is
 * written against this interface alone. That is the property the whole framework rests
 * on: the same panel runs on both targets because a type says it must, not because
 * someone remembered to keep two files in step.
 */

import type { AppDescriptor, FrameStats, Rect, SimEvent } from './types'

export interface FathomHost {
  readonly kind: 'web' | 'native'

  /**
   * Bring the app up. `container` is the element the simulation draws inside: on web the
   * host puts a canvas in it, on native it stays empty and the rect drives a child window.
   */
  init(container: HTMLElement): Promise<AppDescriptor>

  /** Tell the host where, and at what size, to render. */
  setViewport(rect: Rect, dpr: number): void

  writeParams(bytes: Uint8Array): void
  sendEvent(event: SimEvent): void
  command(name: string, args?: Record<string, unknown>): void

  /** Advance one frame. Native runs its own render thread, so this is a no-op there. */
  frame(nowMs: number): void

  stats(): FrameStats
  adapterInfo(): string
  destroy(): void
}

/** A loader for the app's generated wasm bindings; ignored on native. */
export type WasmLoader = () => Promise<{
  default: (options?: unknown) => Promise<unknown>
  FathomApp: {
    create(canvas: HTMLCanvasElement): Promise<WasmApp>
  }
}>

export interface WasmApp {
  descriptor(): string
  paramByteLength(): number
  adapterInfo(): string
  setViewport(width: number, height: number, dpr: number): void
  writeParams(bytes: Uint8Array): void
  input(json: string): void
  command(name: string, args: string): void
  frame(nowMs: number): void
  stats(): string
  free(): void
}

/** True when the page is running inside the Tauri shell rather than a browser tab. */
export function isNative(): boolean {
  if (typeof window === 'undefined') return false
  return '__TAURI_INTERNALS__' in window || '__TAURI__' in window
}

/**
 * Pick the host for wherever this page is running. This is the only place in the
 * interface that knows there is more than one target.
 */
export async function createHost(wasm: WasmLoader): Promise<FathomHost> {
  if (isNative()) {
    const { NativeHost } = await import('./nativeHost')
    return new NativeHost()
  }
  const { WebHost } = await import('./webHost')
  return new WebHost(wasm)
}
