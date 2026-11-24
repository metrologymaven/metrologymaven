```toml
format = "component-spec@1"
id = "comp.button"
title = "Button"

[[parts]]
name = "container"
actionable = true
description = "The area that takes the click and the keyboard focus."

[[parts]]
name = "label"

[[parts]]
name = "reason"
description = "Visible text that says why the button is disabled."

[states]
default = { applies = true }
hover = { applies = true }
focus = { applies = true }
active = { applies = true }
disabled = { applies = true }
read-only = { applies = false, reason = "A button holds no value that could be read-only." }
error = { applies = false, reason = "A button validates nothing; the action reports its own errors." }
loading = { applies = true }

[tokens.container.default]
background = "sem.surface.base"
border = "sem.text.muted"

[tokens.container.hover]
border = "sem.focus.ring"

[tokens.container.focus]
outline = "sem.focus.ring"

[tokens.label.default]
text = "sem.text.nonexistent"

[tokens.reason.default]
text = "sem.text.muted"

[density.container.compact]
padding = "space.1"
gap = "space.1"

[density.container.comfortable]
padding = "space.2"
gap = "space.2"

[[keyboard]]
key = "Enter"
context = "The button has focus."
action = "activate"

[[keyboard]]
key = "Space"
context = "The button has focus."
action = "activate"

[[pointer_actions]]
gesture = "click"
part = "container"
action = "activate"

[a11y]
role = "button"
name_source = "The text of the label part."
states = ["busy", "disabled"]

[focus]
token = "sem.focus.ring"
against = ["sem.surface.base"]
width_px = 2
offset_px = 2

[disabled_reason]
shown_as = "visible"
part = "reason"
```

# Button (synthetic fixture)

## Anatomy

A container holding a label, and a reason line shown while the button is disabled.

## Behaviour

Activating the button runs its action once. While the action runs the button shows loading.

## Keyboard

Enter and Space activate the focused button.

## Accessibility

Role button; the label text is the accessible name.

## Do and don't

Do keep the label short. Don't hide the disabled reason behind a hover.
