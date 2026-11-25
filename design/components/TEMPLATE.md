```toml
# component-spec@1 template (spec node UX.COMP.SCHEMA). Licence: CC-BY-4.0.
# Copy this file to components/<name>.md, set id = "comp.<name>" and replace every value.
# `fid-design comp-check --root design/components --package design` checks this file like any
# other spec, so the example values below are real tokens of the shipped package.
format = "component-spec@1"
id = "comp.template"
title = "Component spec template"

# Anatomy: every part the component is drawn from. A part a person can click, drag or focus is
# `actionable`; a spec with an actionable part is interactive and needs keyboard entries.
[[parts]]
name = "container"
actionable = true
description = "The area that takes the pointer and the keyboard focus."

[[parts]]
name = "label"
description = "The visible name of the control."

[[parts]]
name = "reason"
description = "Visible text that says why the control is disabled."

# All eight states. A state that does not apply says so, with the reason.
[states]
default = { applies = true }
hover = { applies = true }
focus = { applies = true }
active = { applies = true }
disabled = { applies = true }
read-only = { applies = false, reason = "Replace with why the component has no read-only state." }
error = { applies = false, reason = "Replace with why the component has no error state." }
loading = { applies = true }

# Tokens per part and state: <property> = "<token name>". Every part has default tokens; only
# applying states may be listed. Every name must resolve in the package.
[tokens.container.default]
background = "sem.surface.raised"
border = "sem.border.strong"

[tokens.container.hover]
border = "sem.border.strong"
background = "sem.surface.sunken"

[tokens.container.focus]
outline = "sem.focus.ring"

[tokens.container.disabled]
border = "sem.border.subtle"

[tokens.label.default]
text = "sem.text.primary"

[tokens.label.disabled]
text = "sem.text.muted"

[tokens.reason.default]
text = "sem.text.muted"

# Density: compact and comfortable values per part, with the same properties in both modes.
[density.container.compact]
padding-inline = "sem.space.inset.sm"
gap = "sem.space.gap.xs"
min-height = "size.target.min"

[density.container.comfortable]
padding-inline = "sem.space.inset.md"
gap = "sem.space.gap.sm"
min-height = "size.target.min"

# Keyboard map: key, the context in which it works, and the action it performs.
[[keyboard]]
key = "Enter"
context = "The container has focus."
action = "activate"

[[keyboard]]
key = "Space"
context = "The container has focus."
action = "activate"

# Every pointer action needs a keyboard entry with the same action.
[[pointer_actions]]
gesture = "click"
part = "container"
action = "activate"

# Accessible role (closed list of WAI-ARIA 1.2 role names), where the name comes from, and the
# states and properties the implementation exposes (names without the aria- prefix).
[a11y]
role = "button"
name_source = "The text of the label part."
states = ["busy", "disabled"]

# Focus indicator: its token, every surface it is drawn on (each pair is declared non-text in
# tokens/pairs.json), and its geometry in logical pixels (width at least 2).
[focus]
token = "sem.focus.ring"
against = ["sem.surface.base", "sem.surface.raised"]
width_px = 2
offset_px = 2

# Disabled reason: shown as visible text or as text reachable by keyboard focus, never by
# pointer hover alone, in the named part.
[disabled_reason]
shown_as = "visible"
part = "reason"
note = "Replace with where the reason text appears and who writes it."
```

# Component spec template

Write each section in your own words. Keep it toolkit-neutral: name parts, states and tokens,
never a widget class of one toolkit.

## Anatomy

List each part from the front block and say what it shows. Add a simple diagram made for this
project if it helps.

## Behaviour

Describe what each state looks like and when the component enters it, including loading and
the disabled reason.

## Keyboard

Explain the keyboard map in sentences: focus order, the keys from the front block, and what
happens at the edges (first and last item, empty content).

## Accessibility

Say what the role, name and exposed states mean for this component, and what is announced
when the state changes.

## Do and don't

Give short do and don't pairs for using the component in a screen.
