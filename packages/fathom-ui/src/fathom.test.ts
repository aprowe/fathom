import { afterEach, describe, expect, it } from 'vitest'

import { createHost, isNative } from './host'
import { ParamMirror } from './params'
import { deviceRect, localPoint } from './rect'
import type { ParamDef } from './types'

const def = (over: Partial<ParamDef> & Pick<ParamDef, 'name' | 'kind'>): ParamDef => ({
  label: over.name,
  default: 0,
  min: 0,
  max: 1,
  step: 0,
  group: 'General',
  options: [],
  ...over,
})

const SCHEMA: ParamDef[] = [
  def({ name: 'g', kind: 'float', default: 1.5, min: 0, max: 4 }),
  def({ name: 'trails', kind: 'toggle', default: 1, min: 0, max: 1 }),
  def({ name: 'mode', kind: 'choice', default: 2, min: 0, max: 2, options: ['a', 'b', 'c'] }),
]

describe('ParamMirror', () => {
  it('starts every parameter at the value the app declared', () => {
    const mirror = new ParamMirror(SCHEMA, 16)
    expect(mirror.get('g')).toBe(1.5)
    expect(mirror.get('trails')).toBe(1)
    expect(mirror.get('mode')).toBe(2)
  })

  it('writes each parameter at its own offset, in the right representation', () => {
    const mirror = new ParamMirror(SCHEMA, 16)
    mirror.set('g', 2.25)
    mirror.set('trails', 0)

    const view = new DataView(mirror.bytes.buffer)
    expect(view.getFloat32(0, true)).toBe(2.25)
    expect(view.getUint32(4, true)).toBe(0)
    // Ints are stored as integers, not as float bits.
    expect(view.getUint32(8, true)).toBe(2)
  })

  it('flushes once however many times a control was dragged', () => {
    const mirror = new ParamMirror(SCHEMA, 16)
    expect(mirror.takeDirty()).not.toBeNull() // defaults still need sending once

    for (let i = 0; i < 50; i += 1) mirror.set('g', i / 50)
    expect(mirror.takeDirty()).not.toBeNull()
    expect(mirror.takeDirty()).toBeNull()
  })

  it('clamps a value to the declared range instead of sending nonsense to the GPU', () => {
    const mirror = new ParamMirror(SCHEMA, 16)
    mirror.set('g', 99)
    expect(mirror.get('g')).toBe(4)
    mirror.set('g', -99)
    expect(mirror.get('g')).toBe(0)
  })

  it('names the available parameters when a control binds to one that does not exist', () => {
    const mirror = new ParamMirror(SCHEMA, 16)
    expect(() => mirror.definition('gravity')).toThrowError(/g, trails, mode/)
  })

  it('restores declared defaults', () => {
    const mirror = new ParamMirror(SCHEMA, 16)
    mirror.set('g', 0.1)
    mirror.resetToDefaults()
    expect(mirror.get('g')).toBe(1.5)
  })
})

describe('viewport rect', () => {
  const box = { left: 12.4, top: 60.2, width: 800.6, height: 400.4 }

  it('converts CSS pixels to device pixels', () => {
    expect(deviceRect(box, 2)).toEqual({ x: 25, y: 120, width: 1601, height: 801 })
  })

  it('is unchanged at a device pixel ratio of one', () => {
    expect(deviceRect(box, 1)).toEqual({ x: 12, y: 60, width: 801, height: 400 })
  })

  it('follows the box when the page scrolls', () => {
    const scrolled = { ...box, top: box.top - 200 }
    expect(deviceRect(scrolled, 1).y).toBe(-140)
  })

  it('never reports a zero-sized target', () => {
    const collapsed = deviceRect({ left: 0, top: 0, width: 0, height: 0 }, 2)
    expect(collapsed.width).toBe(1)
    expect(collapsed.height).toBe(1)
  })

  it('treats a nonsense pixel ratio as one rather than collapsing the rect', () => {
    expect(deviceRect(box, 0).width).toBe(801)
  })

  it('measures pointer positions from the top left of the viewport', () => {
    const point = localPoint(box, { clientX: 112.4, clientY: 160.2 }, 2)
    expect(point.x).toBeCloseTo(200, 6)
    expect(point.y).toBeCloseTo(200, 6)
  })
})

describe('host selection', () => {
  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__
  })

  const wasm = (() => Promise.reject(new Error('not loaded'))) as never

  it('uses the web host in a plain browser tab', async () => {
    expect(isNative()).toBe(false)
    const host = await createHost(wasm)
    expect(host.kind).toBe('web')
  })

  it('uses the native host inside the Tauri shell', async () => {
    ;(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = { invoke: () => Promise.resolve({}) }
    expect(isNative()).toBe(true)
    const host = await createHost(wasm)
    expect(host.kind).toBe('native')
  })
})
