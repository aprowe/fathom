/**
 * Brings the app up, owns the frame loop, and hands the rest of the interface a host,
 * a descriptor and a parameter mirror.
 *
 * The loop lives here rather than in Rust so the interface can pause, throttle or tear
 * the app down without fighting a loop it does not own.
 */

import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'

import { createHost, type FathomHost, type WasmLoader } from './host'
import { ParamMirror } from './params'
import type { AppDescriptor, FrameStats } from './types'

interface FathomValue {
  host: FathomHost
  descriptor: AppDescriptor
  params: ParamMirror
  /** The element the host renders into; `SimViewport` adopts it. */
  surface: HTMLElement
  stats: FrameStats
  paused: boolean
  setPaused(paused: boolean): void
}

const FathomContext = createContext<FathomValue | null>(null)

export function useFathom(): FathomValue {
  const value = useContext(FathomContext)
  if (!value) throw new Error('useFathom must be used inside <FathomProvider>')
  return value
}

type Phase =
  | { status: 'loading' }
  | { status: 'error'; message: string }
  | { status: 'ready'; host: FathomHost; descriptor: AppDescriptor; params: ParamMirror }

/** How often the frame counters are re-rendered. Every frame would be unreadable. */
const STATS_REFRESH_MS = 400

export function FathomProvider({ wasm, children }: { wasm: WasmLoader; children: ReactNode }) {
  const [phase, setPhase] = useState<Phase>({ status: 'loading' })
  const [stats, setStats] = useState<FrameStats>({ frame: 0, fps: 0, frameMs: 0, time: 0 })
  const [paused, setPausedState] = useState(false)

  // Created once, detached, and adopted by <SimViewport>. The host needs an element to
  // attach to before the descriptor exists, so it cannot come from a child's ref.
  const surface = useMemo(() => {
    const el = document.createElement('div')
    el.className = 'fathom-surface'
    return el
  }, [])

  useEffect(() => {
    let disposed = false
    let host: FathomHost | null = null

    const start = async () => {
      try {
        host = await createHost(wasm)
        const descriptor = await host.init(surface)
        if (disposed) {
          host.destroy()
          return
        }
        const params = new ParamMirror(descriptor.params, host.paramByteLength())
        setPhase({ status: 'ready', host, descriptor, params })
      } catch (error) {
        if (!disposed) setPhase({ status: 'error', message: describe(error) })
      }
    }
    void start()

    return () => {
      disposed = true
      host?.destroy()
    }
  }, [surface, wasm])

  useEffect(() => {
    if (phase.status !== 'ready') return
    const { host, params } = phase
    let raf = 0
    let lastStatsAt = 0

    const tick = (now: number) => {
      raf = requestAnimationFrame(tick)
      const dirty = params.takeDirty()
      if (dirty) host.writeParams(dirty)
      host.frame(now)
      if (now - lastStatsAt > STATS_REFRESH_MS) {
        lastStatsAt = now
        setStats(host.stats())
      }
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [phase])

  if (phase.status === 'loading') return <Splash />
  if (phase.status === 'error') return <Failure message={phase.message} />

  const setPaused = (next: boolean) => {
    setPausedState(next)
    phase.host.command(next ? 'fathom.pause' : 'fathom.resume')
  }

  return (
    <FathomContext.Provider
      value={{
        host: phase.host,
        descriptor: phase.descriptor,
        params: phase.params,
        surface,
        stats,
        paused,
        setPaused,
      }}
    >
      {children}
    </FathomContext.Provider>
  )
}

function describe(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return String(error)
}

function Splash() {
  return (
    <div className="fathom-splash">
      <span className="fathom-splash-mark" aria-hidden />
      <p>Starting the simulation</p>
    </div>
  )
}

function Failure({ message }: { message: string }) {
  return (
    <div className="fathom-failure" role="alert">
      <h1>This browser can&rsquo;t reach a GPU</h1>
      <p className="fathom-failure-reason">{message}</p>
      <p>
        fathom draws through WebGPU. Chrome and Edge 113 or newer support it, as does
        Safari 26. In Firefox, enable <code>dom.webgpu.enabled</code> and restart.
      </p>
    </div>
  )
}
