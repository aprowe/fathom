/**
 * The wire types shared with Rust.
 *
 * These mirror `fathom_core::app::AppDescriptor` and friends. Rust serialises them, so
 * this file is the one place the interface has to agree with the core by hand.
 */

export type ParamKind = 'float' | 'int' | 'toggle' | 'choice'

export interface ParamDef {
  name: string
  label: string
  kind: ParamKind
  default: number
  min: number
  max: number
  /** 0 means continuous. */
  step: number
  group: string
  options: string[]
}

export interface CommandDef {
  name: string
  label: string
  group: string
  /** Non-empty means "render a select, not a button". */
  options: string[]
  initial: number
}

export interface AppDescriptor {
  name: string
  params: ParamDef[]
  commands: CommandDef[]
}

export interface FrameStats {
  frame: number
  fps: number
  frameMs: number
  time: number
}

/** Viewport rect in device pixels, relative to the window. */
export interface Rect {
  x: number
  y: number
  width: number
  height: number
}

export type SimEvent =
  | { kind: 'mousePressed' | 'mouseMoved' | 'mouseDragged' | 'mouseReleased'; x: number; y: number; button: number; buttons: number; shift: boolean; ctrl: boolean; alt: boolean }
  | { kind: 'scrolled'; x: number; y: number; deltaY: number; shift: boolean; ctrl: boolean; alt: boolean }
  | { kind: 'keyPressed' | 'keyReleased'; key: string; shift: boolean; ctrl: boolean; alt: boolean }

/** Commands the framework handles itself, whatever the app is. */
export const FRAMEWORK_COMMANDS = {
  pause: 'fathom.pause',
  resume: 'fathom.resume',
  togglePause: 'fathom.toggle_pause',
  step: 'fathom.step',
  resetCamera: 'fathom.reset_camera',
} as const
