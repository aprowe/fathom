/**
 * Turning a laid-out box into the rect the host renders into.
 *
 * Pulled out of the component because this is the one piece of the "events tell it
 * where to render" path that can be wrong in a way nothing else catches: on native the
 * numbers here move a real window, and a device-pixel-ratio slip puts the simulation
 * half off the panel.
 */

import type { Rect } from './types'

export interface BoxLike {
  left: number
  top: number
  width: number
  height: number
}

/**
 * CSS pixels to device pixels. The result is in the window's coordinate space, because
 * that is the space the native child window lives in.
 */
export function deviceRect(box: BoxLike, dpr: number): Rect {
  const ratio = dpr > 0 ? dpr : 1
  return {
    x: Math.round(box.left * ratio),
    y: Math.round(box.top * ratio),
    width: Math.max(1, Math.round(box.width * ratio)),
    height: Math.max(1, Math.round(box.height * ratio)),
  }
}

/** A pointer position relative to the box, in device pixels. */
export function localPoint(
  box: BoxLike,
  client: { clientX: number; clientY: number },
  dpr: number,
): { x: number; y: number } {
  const ratio = dpr > 0 ? dpr : 1
  return { x: (client.clientX - box.left) * ratio, y: (client.clientY - box.top) * ratio }
}
