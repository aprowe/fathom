/** @fathom/ui — the React runtime for fathom apps. */

import './styles.css'

export { FathomProvider, useFathom } from './context'
export { SimViewport } from './SimViewport'
export { Panel, Toolbar } from './Panel'
export {
  AutoControls,
  Choice,
  CommandButton,
  CommandChoice,
  Control,
  Slider,
  Toggle,
} from './controls'
export { ParamMirror } from './params'
export { createHost, isNative, type FathomHost, type WasmLoader } from './host'
export { deviceRect, localPoint } from './rect'
export type {
  AppDescriptor,
  CommandDef,
  FrameStats,
  ParamDef,
  ParamKind,
  Rect,
  SimEvent,
} from './types'
