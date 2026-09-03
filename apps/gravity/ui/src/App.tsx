import { AutoControls, FathomProvider, Panel, SimViewport, Toolbar } from '@fathom/ui'

import './app.css'

/**
 * The generated wasm bindings for this app. Loaded lazily so the interface can render
 * its own failure state if WebGPU is missing, instead of dying on an import.
 *
 * On native this is never called: the Tauri host runs the same Rust natively.
 */
const wasm = () => import('../../pkg/gravity.js')

export function App() {
  return (
    <FathomProvider wasm={wasm as never}>
      <div className="fathom-app">
        <div className="fathom-stage">
          <SimViewport />
          <Toolbar />
        </div>
        <Panel>
          <AutoControls />
          <p className="gravity-hint">
            Drag inside the view to pull bodies toward the cursor. Shift-drag or
            middle-drag to pan, scroll to zoom, and press R to reseed.
          </p>
        </Panel>
      </div>
    </FathomProvider>
  )
}
