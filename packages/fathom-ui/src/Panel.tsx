/** The docked control panel and the floating transport bar. */

import type { ReactNode } from 'react'

import { useFathom } from './context'

export function Panel({ children }: { children: ReactNode }) {
  const { descriptor, host } = useFathom()

  return (
    <aside className="fathom-panel">
      <header className="fathom-panel-head">
        <h1>{descriptor.name}</h1>
        <p title={host.adapterInfo()}>{host.adapterInfo() || 'GPU'}</p>
      </header>
      <div className="fathom-panel-body">{children}</div>
    </aside>
  )
}

/**
 * Play, step, recentre, and the frame counters.
 *
 * It floats over the viewport rather than sitting above it, which is the same
 * arrangement the native target uses for real: interface over simulation.
 */
export function Toolbar() {
  const { host, stats, paused, setPaused } = useFathom()

  return (
    <div className="fathom-toolbar">
      <button
        type="button"
        className="fathom-transport"
        onClick={() => setPaused(!paused)}
        aria-label={paused ? 'Run' : 'Pause'}
      >
        {paused ? <PlayIcon /> : <PauseIcon />}
      </button>
      <button
        type="button"
        className="fathom-transport"
        onClick={() => host.command('fathom.step')}
        disabled={!paused}
        aria-label="Step one frame"
      >
        <StepIcon />
      </button>
      <button
        type="button"
        className="fathom-transport"
        onClick={() => host.command('fathom.reset_camera')}
        aria-label="Recentre the view"
      >
        <RecentreIcon />
      </button>

      <span className="fathom-toolbar-rule" aria-hidden />

      <span className="fathom-stat">
        <b>{stats.fps.toFixed(0)}</b> fps
      </span>
      <span className="fathom-stat">
        <b>{stats.frameMs.toFixed(1)}</b> ms
      </span>
      <span className={paused ? 'fathom-state is-paused' : 'fathom-state'}>
        {paused ? 'Paused' : 'Running'}
      </span>
    </div>
  )
}

const iconProps = {
  width: 14,
  height: 14,
  viewBox: '0 0 14 14',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 1.6,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
}

function PlayIcon() {
  return (
    <svg {...iconProps} aria-hidden>
      <path d="M4 2.6 11 7l-7 4.4z" fill="currentColor" stroke="none" />
    </svg>
  )
}

function PauseIcon() {
  return (
    <svg {...iconProps} aria-hidden>
      <path d="M5 2.5v9M9 2.5v9" />
    </svg>
  )
}

function StepIcon() {
  return (
    <svg {...iconProps} aria-hidden>
      <path d="M3 2.6 9 7l-6 4.4z" fill="currentColor" stroke="none" />
      <path d="M11 2.6v8.8" />
    </svg>
  )
}

function RecentreIcon() {
  return (
    <svg {...iconProps} aria-hidden>
      <circle cx="7" cy="7" r="3.2" />
      <path d="M7 1v1.8M7 11.2V13M1 7h1.8M11.2 7H13" />
    </svg>
  )
}
