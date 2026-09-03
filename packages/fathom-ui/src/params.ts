/**
 * The interface's mirror of the parameter block.
 *
 * Controls write straight into a typed array and the host flushes the whole block once
 * per animation frame. That matters more than it looks: dragging a slider fires dozens
 * of events a second, and routing each one through React state (or, on native, through
 * its own IPC call) would cost more than the simulation does.
 */

import type { ParamDef } from './types'

export class ParamMirror {
  readonly bytes: Uint8Array
  private readonly floats: Float32Array
  private readonly ints: Uint32Array
  private readonly defs = new Map<string, { index: number; def: ParamDef }>()
  private dirty = true

  constructor(schema: ParamDef[], byteLength: number) {
    const buffer = new ArrayBuffer(Math.max(byteLength, schema.length * 4, 16))
    this.bytes = new Uint8Array(buffer)
    this.floats = new Float32Array(buffer)
    this.ints = new Uint32Array(buffer)

    schema.forEach((def, index) => {
      this.defs.set(def.name, { index, def })
      this.write(index, def, def.default)
    })
  }

  definition(name: string): ParamDef {
    const entry = this.defs.get(name)
    if (!entry) {
      throw new Error(
        `No parameter named "${name}". The app declares: ${[...this.defs.keys()].join(', ')}`,
      )
    }
    return entry.def
  }

  get(name: string): number {
    const { index, def } = this.entry(name)
    return def.kind === 'float' ? this.floats[index] : this.ints[index]
  }

  set(name: string, value: number): void {
    const { index, def } = this.entry(name)
    this.write(index, def, value)
    this.dirty = true
  }

  /** Reset every parameter to the value the app declared. */
  resetToDefaults(): void {
    for (const { index, def } of this.defs.values()) this.write(index, def, def.default)
    this.dirty = true
  }

  /** Returns the block if anything changed since the last flush, otherwise null. */
  takeDirty(): Uint8Array | null {
    if (!this.dirty) return null
    this.dirty = false
    return this.bytes
  }

  private entry(name: string) {
    const entry = this.defs.get(name)
    if (!entry) throw new Error(`No parameter named "${name}"`)
    return entry
  }

  private write(index: number, def: ParamDef, value: number): void {
    const clamped = Math.min(Math.max(value, def.min), def.max)
    if (def.kind === 'float') this.floats[index] = clamped
    else this.ints[index] = Math.round(clamped)
  }
}
