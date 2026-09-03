/**
 * The hole in the interface where the simulation appears.
 *
 * This component is the whole "events tell it where to render" story: it measures its
 * own box and reports the rect. On web that sizes a canvas; on native it moves the wgpu
 * child window under the transparent webview. The rest of this file is input, which
 * goes through the DOM identically on both targets.
 */

import { useEffect, useRef, type PointerEvent as ReactPointerEvent, type WheelEvent } from 'react'

import { useFathom } from './context'
import { deviceRect, localPoint } from './rect'
import type { SimEvent } from './types'

export function SimViewport({ className }: { className?: string }) {
  const { host, surface } = useFathom()
  const ref = useRef<HTMLDivElement>(null)
  const dragging = useRef(false)

  // Adopt the host's surface element, which was created before this component existed.
  useEffect(() => {
    const node = ref.current
    if (!node) return
    node.appendChild(surface)
    return () => {
      if (surface.parentNode === node) node.removeChild(surface)
    }
  }, [surface])

  // Report the rect whenever anything could have moved it. Scroll and window resize are
  // included because a rect is only meaningful relative to the window the native child
  // lives in.
  useEffect(() => {
    const node = ref.current
    if (!node) return

    let pending = 0
    const report = () => {
      cancelAnimationFrame(pending)
      pending = requestAnimationFrame(() => {
        const dpr = window.devicePixelRatio || 1
        host.setViewport(deviceRect(node.getBoundingClientRect(), dpr), dpr)
      })
    }

    const observer = new ResizeObserver(report)
    observer.observe(node)
    window.addEventListener('resize', report)
    window.addEventListener('scroll', report, true)
    report()

    return () => {
      cancelAnimationFrame(pending)
      observer.disconnect()
      window.removeEventListener('resize', report)
      window.removeEventListener('scroll', report, true)
    }
  }, [host])

  // Keys are global: the viewport has no reason to steal focus from the panel, and a
  // shortcut should work wherever the pointer happens to be.
  useEffect(() => {
    const isTyping = (target: EventTarget | null) =>
      target instanceof HTMLElement &&
      (target.tagName === 'INPUT' || target.tagName === 'SELECT' || target.isContentEditable)

    const send = (kind: 'keyPressed' | 'keyReleased') => (e: KeyboardEvent) => {
      if (isTyping(e.target)) return
      host.sendEvent({ kind, key: e.key, shift: e.shiftKey, ctrl: e.ctrlKey, alt: e.altKey })
    }
    const down = send('keyPressed')
    const up = send('keyReleased')
    window.addEventListener('keydown', down)
    window.addEventListener('keyup', up)
    return () => {
      window.removeEventListener('keydown', down)
      window.removeEventListener('keyup', up)
    }
  }, [host])

  const local = (e: { clientX: number; clientY: number }) =>
    localPoint(ref.current!.getBoundingClientRect(), e, window.devicePixelRatio || 1)

  const mouse = (
    kind: 'mousePressed' | 'mouseMoved' | 'mouseDragged' | 'mouseReleased',
    e: ReactPointerEvent,
  ): SimEvent => ({
    kind,
    ...local(e),
    button: e.button,
    buttons: e.buttons,
    shift: e.shiftKey,
    ctrl: e.ctrlKey,
    alt: e.altKey,
  })

  return (
    <div
      ref={ref}
      className={className ? `fathom-viewport ${className}` : 'fathom-viewport'}
      onPointerDown={(e) => {
        dragging.current = true
        // Capture so a drag that leaves the viewport keeps steering the simulation.
        e.currentTarget.setPointerCapture(e.pointerId)
        host.sendEvent(mouse('mousePressed', e))
      }}
      onPointerMove={(e) => {
        host.sendEvent(mouse(dragging.current ? 'mouseDragged' : 'mouseMoved', e))
      }}
      onPointerUp={(e) => {
        dragging.current = false
        e.currentTarget.releasePointerCapture(e.pointerId)
        host.sendEvent(mouse('mouseReleased', e))
      }}
      onPointerCancel={(e) => {
        dragging.current = false
        host.sendEvent(mouse('mouseReleased', e))
      }}
      onWheel={(e: WheelEvent) => {
        host.sendEvent({
          kind: 'scrolled',
          ...local(e),
          deltaY: e.deltaY,
          shift: e.shiftKey,
          ctrl: e.ctrlKey,
          alt: e.altKey,
        })
      }}
      onContextMenu={(e) => e.preventDefault()}
    />
  )
}
