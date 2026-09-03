/**
 * The control kit.
 *
 * Controls bind to a parameter by name and are checked against the app's declared
 * schema, so a typo is an error at startup instead of a slider that silently does
 * nothing. Sliders write straight into the parameter mirror and update their own
 * readout through a ref — dragging one never re-renders React.
 */

import { useRef, useState } from 'react'

import { useFathom } from './context'
import type { CommandDef, ParamDef } from './types'

function formatValue(def: ParamDef, value: number): string {
  if (def.kind === 'float') {
    const span = def.max - def.min
    return value.toFixed(span >= 20 ? 0 : span >= 2 ? 2 : 3)
  }
  return String(Math.round(value))
}

export function Slider({ param }: { param: string }) {
  const { params } = useFathom()
  const def = params.definition(param)
  const readout = useRef<HTMLSpanElement>(null)
  const [initial] = useState(() => params.get(param))
  const step = def.step > 0 ? def.step : (def.max - def.min) / 1000

  const fill = (value: number) => `${((value - def.min) / (def.max - def.min)) * 100}%`

  return (
    <label className="fathom-control fathom-slider">
      <span className="fathom-control-head">
        <span className="fathom-control-label">{def.label}</span>
        <span className="fathom-readout" ref={readout}>
          {formatValue(def, initial)}
        </span>
      </span>
      <input
        type="range"
        min={def.min}
        max={def.max}
        step={step}
        defaultValue={initial}
        style={{ ['--fathom-fill' as string]: fill(initial) }}
        onInput={(e) => {
          const input = e.currentTarget
          const value = Number(input.value)
          params.set(param, value)
          input.style.setProperty('--fathom-fill', fill(value))
          if (readout.current) readout.current.textContent = formatValue(def, value)
        }}
      />
    </label>
  )
}

export function Toggle({ param }: { param: string }) {
  const { params } = useFathom()
  const def = params.definition(param)
  const [on, setOn] = useState(() => params.get(param) !== 0)

  return (
    <label className="fathom-control fathom-toggle">
      <span className="fathom-control-label">{def.label}</span>
      <button
        type="button"
        role="switch"
        aria-checked={on}
        className="fathom-switch"
        onClick={() => {
          const next = !on
          setOn(next)
          params.set(param, next ? 1 : 0)
        }}
      >
        <span className="fathom-switch-knob" />
      </button>
    </label>
  )
}

export function Choice({ param }: { param: string }) {
  const { params } = useFathom()
  const def = params.definition(param)
  const [value, setValue] = useState(() => params.get(param))

  return (
    <label className="fathom-control fathom-choice">
      <span className="fathom-control-label">{def.label}</span>
      <select
        value={value}
        onChange={(e) => {
          const next = Number(e.currentTarget.value)
          setValue(next)
          params.set(param, next)
        }}
      >
        {def.options.map((option, i) => (
          <option key={option} value={i}>
            {option}
          </option>
        ))}
      </select>
    </label>
  )
}

/** The right control for a parameter, chosen from its declared kind. */
export function Control({ param }: { param: string }) {
  const { params } = useFathom()
  const def = params.definition(param)
  if (def.kind === 'toggle') return <Toggle param={param} />
  if (def.kind === 'choice') return <Choice param={param} />
  return <Slider param={param} />
}

export function CommandButton({ command, label }: { command: string; label?: string }) {
  const { host, descriptor } = useFathom()
  const def = descriptor.commands.find((c) => c.name === command)
  return (
    <button type="button" className="fathom-button" onClick={() => host.command(command)}>
      {label ?? def?.label ?? command}
    </button>
  )
}

export function CommandChoice({ command }: { command: string }) {
  const { host, descriptor } = useFathom()
  const def = descriptor.commands.find((c) => c.name === command)
  const [value, setValue] = useState(def?.initial ?? 0)
  if (!def) throw new Error(`No command named "${command}"`)

  return (
    <label className="fathom-control fathom-choice">
      <span className="fathom-control-label">{def.label}</span>
      <select
        value={value}
        onChange={(e) => {
          const next = Number(e.currentTarget.value)
          setValue(next)
          host.command(command, { value: next })
        }}
      >
        {def.options.map((option, i) => (
          <option key={option} value={i}>
            {option}
          </option>
        ))}
      </select>
    </label>
  )
}

/**
 * Every declared parameter and command, grouped as the app declared them.
 *
 * This is the payoff of the schema: an app gets a complete, coherent panel for free, and
 * only writes components for the parts that need something the schema cannot express.
 */
export function AutoControls() {
  const { descriptor } = useFathom()

  const groups: { name: string; params: ParamDef[]; commands: CommandDef[] }[] = []
  const find = (name: string) => {
    let group = groups.find((g) => g.name === name)
    if (!group) {
      group = { name, params: [], commands: [] }
      groups.push(group)
    }
    return group
  }
  for (const p of descriptor.params) find(p.group).params.push(p)
  for (const c of descriptor.commands) find(c.group).commands.push(c)

  return (
    <>
      {groups.map((group) => (
        <section className="fathom-group" key={group.name}>
          <h2>{group.name}</h2>
          {group.params.map((p) => (
            <Control key={p.name} param={p.name} />
          ))}
          {group.commands
            .filter((c) => c.options.length > 0)
            .map((c) => (
              <CommandChoice key={c.name} command={c.name} />
            ))}
          {group.commands.some((c) => c.options.length === 0) && (
            <div className="fathom-button-row">
              {group.commands
                .filter((c) => c.options.length === 0)
                .map((c) => (
                  <CommandButton key={c.name} command={c.name} />
                ))}
            </div>
          )}
        </section>
      ))}
    </>
  )
}
